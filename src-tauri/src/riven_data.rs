//! Keeps the riven data (stat values, dispositions, weapon → riven type) current while the app
//! runs. See `docs/plans/riven-data-updates.md`.
//!
//! The data comes from Warframe Public Export Plus, the same three files `scripts/gen_rivens.py`
//! uses for the bundled copy. They're checked at startup when the last check is over a day old,
//! after a riven card that doesn't fit (at most hourly), and from the Tools page. Each file is
//! only downloaded again when its ETag changed. The rebuilt data is cached in
//! `<app data>/riven-data/rivens.json` and used when it's at least as new as the bundled copy.
//!
//! Dispositions change on DE's servers every few months without the game files (and so the
//! export) changing. The Warframe wiki's weapon modules (`Module:Weapons/data/*`, CC BY-SA 3.0)
//! are updated with each change, so their values replace the export's. They're downloaded at
//! every check; the last good copy is kept in `riven-data/wiki-dispositions.json`. Weapons the
//! export doesn't have yet (Corufell Prime, October 2026) come from the wiki too, as their
//! family's riven weapon.
//!
//! Public Export Plus also lags DE's own Public Export for riven stats (October 2026: the spliced
//! traits, Zoom on shotgun rivens), so DE's `ExportUpgrades_en.json` is checked too and any riven
//! stat Public Export Plus lacks is added from it. DE's index (LZMA) names each file by its
//! content hash, which stands in for an ETag.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::rivens::{self, RivenData};

const BASE_URL: &str = "https://cdn.jsdelivr.net/gh/calamity-inc/warframe-public-export-plus@senpai/";
const FILES: [&str; 3] = ["ExportUpgrades.json", "ExportWeapons.json", "dict.en.json"];
const SOURCE: &str = "Warframe Public Export Plus (calamity-inc/warframe-public-export-plus); newer riven stats from DE's Public Export; dispositions from the Warframe wiki (Module:Weapons/data, CC BY-SA 3.0); formulas from calamity-inc/warframe-riven-info";
const DE_URL: &str = "https://content.warframe.com/PublicExport/";
const DE_UPGRADES: &str = "ExportUpgrades_en.json";
/// The wiki's weapon modules with riven weapons.
const WIKI_URL: &str = "https://wiki.warframe.com/w/Module:Weapons/data/";
const WIKI_PAGES: [&str; 6] = ["primary", "secondary", "melee", "archwing", "companion", "modular"];
const WIKI_CACHE: &str = "wiki-dispositions.json";

/// What the wiki says about a weapon: its disposition, plus its name and family for weapons the
/// export doesn't have yet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiWeapon {
    pub disposition: f64,
    pub name: String,
    /// "Corufell" for Corufell and Corufell Prime.
    pub family: String,
}
/// Check at startup when the last check is older than this.
const MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
/// A card that doesn't fit triggers a check, at most this often.
const MISFIT_GAP: Duration = Duration::from_secs(60 * 60);
const PREFIX: &str = "/Lotus/Upgrades/Mods/Randomized/";

/// What the Tools and Rivens pages show.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DataStatus {
    /// "idle", "checking", "ready" (just updated or confirmed current) or "failed".
    pub state: &'static str,
    /// The date of the data in use.
    pub built: String,
    /// "bundled" or "downloaded".
    pub source: &'static str,
    /// Last finished check (unix ms).
    pub checked_at: Option<i64>,
    pub error: Option<String>,
}

static STATUS: Mutex<Option<DataStatus>> = Mutex::new(None);
static DIR: OnceLock<PathBuf> = OnceLock::new();
static APP: OnceLock<AppHandle> = OnceLock::new();
static RUNNING: AtomicBool = AtomicBool::new(false);
static LAST_AUTO: Mutex<Option<Instant>> = Mutex::new(None);

fn status() -> DataStatus {
    STATUS.lock().unwrap().clone().unwrap_or_else(|| DataStatus {
        state: "idle",
        built: rivens::data().built.clone(),
        source: "bundled",
        ..Default::default()
    })
}

fn set_status(f: impl FnOnce(&mut DataStatus)) {
    let mut s = status();
    f(&mut s);
    *STATUS.lock().unwrap() = Some(s.clone());
    if let Some(app) = APP.get() {
        let _ = app.emit("riven-data", s);
    }
}

/// Loads a cached download if it's newer than the bundled copy, then checks for updates in the
/// background if the last check is old. Call once from setup.
pub fn start(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let Ok(dir) = app.path().app_data_dir().map(|d| d.join("riven-data")) else { return };
    let _ = std::fs::create_dir_all(dir.join("export"));
    let _ = DIR.set(dir.clone());
    let mut source = "bundled";
    if let Some(cached) = std::fs::read_to_string(dir.join("rivens.json")).ok().and_then(|t| rivens::parse_data(&t).ok()) {
        if cached.built >= rivens::data().built {
            rivens::set_data(cached);
            source = "downloaded";
        }
    }
    let checked_at = checked_at(&dir);
    *STATUS.lock().unwrap() =
        Some(DataStatus { state: "idle", built: rivens::data().built.clone(), source, checked_at, error: None });
    // Data built before DE's export was read misses its stats: check at once.
    let stale = checked_at.is_none_or(|t| now_ms() - t > MAX_AGE.as_millis() as i64)
        || !dir.join("export").join(DE_UPGRADES).exists();
    if stale {
        std::thread::spawn(refresh);
    }
}

/// A riven card didn't fit the data: check for new data (at most hourly).
pub fn refresh_after_misfit() {
    let mut last = LAST_AUTO.lock().unwrap();
    if last.is_some_and(|t| t.elapsed() < MISFIT_GAP) {
        return;
    }
    *last = Some(Instant::now());
    std::thread::spawn(refresh);
}

#[tauri::command]
pub fn riven_data_status() -> DataStatus {
    let mut s = status();
    if RUNNING.load(Ordering::Relaxed) {
        s.state = "checking";
    }
    s
}

/// The Tools page's "Check for updates".
#[tauri::command]
pub fn check_riven_data() -> DataStatus {
    if !RUNNING.load(Ordering::Relaxed) {
        std::thread::spawn(refresh);
    }
    let mut s = status();
    s.state = "checking";
    s
}

/// Checks the source, rebuilds the data if any file changed, and swaps it in.
fn refresh() {
    let Some(dir) = DIR.get() else { return };
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    set_status(|s| {
        s.state = "checking";
        s.error = None;
    });
    let result = update(dir);
    let _ = std::fs::write(dir.join("checked"), now_ms().to_string());
    match result {
        Ok(changes) => {
            set_status(|s| {
                s.state = "ready";
                s.built = rivens::data().built.clone();
                if changes.is_some() {
                    s.source = "downloaded";
                }
                s.checked_at = Some(now_ms());
            });
            if let Some(changes) = changes {
                crate::overlay::riven_data_changed(&changes);
            }
        }
        Err(e) => {
            eprintln!("[rivens] data update failed: {e}");
            set_status(|s| {
                s.state = "failed";
                s.error = Some(e);
                s.checked_at = Some(now_ms());
            });
        }
    }
    RUNNING.store(false, Ordering::SeqCst);
}

/// Downloads the changed files and rebuilds. `Ok(None)` when nothing changed; otherwise the
/// differences from the data that was in use.
fn update(dir: &Path) -> Result<Option<Vec<String>>, String> {
    let etags_path = dir.join("etags.json");
    let mut etags: HashMap<String, String> =
        std::fs::read_to_string(&etags_path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut changed = false;
    for file in FILES {
        let path = dir.join("export").join(file);
        let mut request = crate::net::agent().get(format!("{BASE_URL}{file}"));
        if let (true, Some(tag)) = (path.exists(), etags.get(file)) {
            request = request.header("If-None-Match", tag);
        }
        let mut response = request.call().map_err(|e| format!("{file}: {e}"))?;
        if response.status() == 304 {
            continue;
        }
        let tag = response.headers().get("etag").and_then(|v| v.to_str().ok()).map(String::from);
        let bytes = response.body_mut().with_config().limit(64 * 1024 * 1024).read_to_vec().map_err(|e| format!("{file}: {e}"))?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, &path)).map_err(|e| format!("{file}: {e}"))?;
        match tag {
            Some(t) => etags.insert(file.to_string(), t),
            None => etags.remove(file),
        };
        changed = true;
    }
    // DE's riven stats; on failure, the last good copy (Public Export Plus's stats still work).
    match fetch_de_upgrades(dir, &mut etags) {
        Ok(fresh) => changed |= fresh,
        Err(e) => eprintln!("[rivens] DE's export: {e}"),
    }
    let _ = std::fs::write(&etags_path, serde_json::to_string(&etags).unwrap_or_default());

    // The wiki's dispositions; on failure, the last good copy (the export's values still work).
    let cache = dir.join(WIKI_CACHE);
    let cached: BTreeMap<String, WikiWeapon> =
        std::fs::read_to_string(&cache).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let dispositions = match fetch_wiki_dispositions() {
        Ok(fresh) => {
            if fresh != cached {
                let _ = std::fs::write(&cache, serde_json::to_string(&fresh).unwrap_or_default());
                changed = true;
            }
            fresh
        }
        Err(e) => {
            eprintln!("[rivens] wiki dispositions: {e}");
            cached
        }
    };
    if !changed && dir.join("rivens.json").exists() {
        return Ok(None);
    }

    let read = |f: &str| -> Result<Value, String> {
        let text = std::fs::read_to_string(dir.join("export").join(f)).map_err(|e| format!("{f}: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("{f}: {e}"))
    };
    let de = std::fs::read(dir.join("export").join(DE_UPGRADES)).ok().and_then(|b| de_rivens(&b));
    let built = build_data(&read(FILES[0])?, &read(FILES[1])?, &read(FILES[2])?, de.as_ref(), &dispositions, &today());
    let text = serde_json::to_string(&built).map_err(|e| e.to_string())?;
    let new = rivens::parse_data(&text)?;
    if new.types.len() < 7 || new.weapons.len() < 300 {
        return Err(format!("the data looks incomplete ({} riven types, {} weapons)", new.types.len(), new.weapons.len()));
    }
    let changes = differences(&rivens::data(), &new);
    let tmp = dir.join("rivens.json.tmp");
    std::fs::write(&tmp, &text).and_then(|_| std::fs::rename(&tmp, dir.join("rivens.json"))).map_err(|e| e.to_string())?;
    rivens::set_data(new);
    Ok(Some(changes))
}

/// Dispositions and stat values that changed, for the overlay log.
pub fn differences(old: &RivenData, new: &RivenData) -> Vec<String> {
    let mut out = Vec::new();
    for (path, w) in &new.weapons {
        match old.weapons.get(path) {
            Some(o) if (o.disposition - w.disposition).abs() > 1e-9 => {
                out.push(format!("{} disposition {} → {}", w.name, o.disposition, w.disposition))
            }
            None if w.kind.is_some() => out.push(format!("new weapon {} ({})", w.name, w.disposition)),
            _ => {}
        }
    }
    for (kind, t) in &new.types {
        for (tag, s) in &t.stats {
            match old.types.get(kind).and_then(|o| o.stats.get(tag)) {
                Some(o) if (o.value - s.value).abs() > 1e-12 => out.push(format!("{kind} {tag} {} → {}", o.value, s.value)),
                None => out.push(format!("{kind}: new stat {tag}")),
                _ => {}
            }
        }
    }
    out.sort();
    out
}

/// Downloads DE's `ExportUpgrades_en.json` if its name in DE's index changed. `Ok(true)` when a
/// new copy was saved.
fn fetch_de_upgrades(dir: &Path, etags: &mut HashMap<String, String>) -> Result<bool, String> {
    let get = |url: String, limit: u64| -> Result<Vec<u8>, String> {
        let mut response = crate::net::agent().get(&url).call().map_err(|e| e.to_string())?;
        response.body_mut().with_config().limit(limit).read_to_vec().map_err(|e| e.to_string())
    };
    let packed = get(format!("{DE_URL}index_en.txt.lzma"), 1024 * 1024)?;
    let name = de_index_entry(&packed, DE_UPGRADES).ok_or_else(|| format!("no {DE_UPGRADES} in DE's index"))?;
    let path = dir.join("export").join(DE_UPGRADES);
    if path.exists() && etags.get(DE_UPGRADES) == Some(&name) {
        return Ok(false);
    }
    let bytes = get(format!("{DE_URL}Manifest/{name}"), 64 * 1024 * 1024)?;
    if de_rivens(&bytes).is_none_or(|r| r.as_object().is_none_or(Map::is_empty)) {
        return Err(format!("{DE_UPGRADES} has no rivens"));
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, &path)).map_err(|e| e.to_string())?;
    etags.insert(DE_UPGRADES.to_string(), name);
    Ok(true)
}

/// A file's name in DE's index (`ExportUpgrades_en.json!00_iIR9…`): LZMA with the size in its
/// header and no end marker.
fn de_index_entry(packed: &[u8], file: &str) -> Option<String> {
    let mut index = Vec::new();
    lzma_rs::lzma_decompress(&mut std::io::Cursor::new(packed), &mut index).ok()?;
    let index = String::from_utf8_lossy(&index);
    index.lines().map(str::trim).find(|l| l.strip_prefix(file).is_some_and(|rest| rest.starts_with('!'))).map(String::from)
}

/// Riven path → DE's entry for it. The file has raw line breaks and tabs inside strings, which
/// JSON doesn't allow; they're spaces in the text anyway.
fn de_rivens(bytes: &[u8]) -> Option<Value> {
    let clean: Vec<u8> = bytes.iter().map(|b| if matches!(b, b'\r' | b'\n' | b'\t') { b' ' } else { *b }).collect();
    let file: Value = serde_json::from_slice(&clean).ok()?;
    let rivens = file
        .get("ExportUpgrades")?
        .as_array()?
        .iter()
        .filter_map(|e| Some((e.get("uniqueName")?.as_str()?.to_string(), e.clone())))
        .filter(|(path, _)| path.starts_with(PREFIX))
        .collect();
    Some(Value::Object(rivens))
}

/// DE's float32 values (0.0099999998) in Public Export Plus's spelling (0.01): 7 significant digits.
fn single(v: f64) -> f64 {
    format!("{v:.6e}").parse().unwrap_or(v)
}

/// Downloads the wiki's weapon modules: weapon path → what the wiki says about it.
fn fetch_wiki_dispositions() -> Result<BTreeMap<String, WikiWeapon>, String> {
    let mut found = HashMap::new();
    for page in WIKI_PAGES {
        let lua = crate::net::agent()
            .get(format!("{WIKI_URL}{page}?action=raw"))
            .call()
            .map_err(|e| format!("{page}: {e}"))?
            .body_mut()
            .with_config()
            .limit(16 * 1024 * 1024)
            .read_to_string()
            .map_err(|e| format!("{page}: {e}"))?;
        wiki_dispositions(&lua, &mut found);
    }
    let out = settled(found);
    if out.len() < 300 {
        return Err(format!("only {} weapons found", out.len()));
    }
    Ok(out)
}

/// Reads one wiki weapon module (`Haalvu = {` / `["Tenet Quanta"] = {`, then fields indented
/// two tabs: `InternalName = "/Lotus/Weapons/…",`, `Disposition = 0.6,`, `Name = "…",`,
/// `Family = "…",`) into path → entries.
fn wiki_dispositions(lua: &str, found: &mut HashMap<String, Vec<WikiWeapon>>) {
    let (mut path, mut value, mut name, mut family) = (None, None, String::new(), String::new());
    let quoted = |rest: &str| rest.split('"').next().unwrap_or_default().to_string();
    for line in lua.lines() {
        if line.starts_with('\t') && !line.starts_with("\t\t") {
            // A weapon's opening or closing line.
            if let (Some(p), Some(disposition)) = (path.take(), value.take()) {
                let entry = WikiWeapon { disposition, name: std::mem::take(&mut name), family: std::mem::take(&mut family) };
                found.entry(p).or_default().push(entry);
            }
            (path, value, name, family) = (None, None, String::new(), String::new());
        } else if let Some(rest) = line.strip_prefix("\t\tInternalName = \"") {
            path = Some(quoted(rest));
        } else if let Some(rest) = line.strip_prefix("\t\tDisposition = ") {
            value = rest.trim().trim_end_matches(',').parse().ok();
        } else if let Some(rest) = line.strip_prefix("\t\tName = \"") {
            name = quoted(rest);
        } else if let Some(rest) = line.strip_prefix("\t\tFamily = \"") {
            family = quoted(rest);
        }
    }
}

/// Keeps the values the wiki agrees on and that are real dispositions. Kitgun chambers are listed
/// twice (as primary and secondary, with different values) under one path; those, and the odd
/// copy-paste slip, keep the export's value.
fn settled(found: HashMap<String, Vec<WikiWeapon>>) -> BTreeMap<String, WikiWeapon> {
    found
        .into_iter()
        .filter(|(_, v)| {
            let d = v[0].disposition;
            v.iter().all(|w| (w.disposition - d).abs() < 1e-9) && (0.5..=1.6).contains(&d)
        })
        .map(|(p, mut v)| (p, v.swap_remove(0)))
        .collect()
}

/// Builds the riven data from the three export files, as `scripts/gen_rivens.py` does, with
/// riven stats only DE's export (`de`, from [`de_rivens`]) has added, the wiki's dispositions
/// replacing the export's, and weapons only the wiki has added as their family's riven weapon.
pub fn build_data(
    upgrades: &Value,
    weapons: &Value,
    words: &Value,
    de: Option<&Value>,
    dispositions: &BTreeMap<String, WikiWeapon>,
    built: &str,
) -> Value {
    let text = |tag: Option<&Value>| tag.and_then(Value::as_str).and_then(|t| words.get(t)).and_then(Value::as_str).map(String::from);
    let empty = Map::new();
    let upgrades = upgrades.as_object().unwrap_or(&empty);
    let weapons = weapons.as_object().unwrap_or(&empty);

    let (mut types, mut veiled, mut riven_for) = (Map::new(), Map::new(), HashMap::<String, String>::new());
    for (path, entry) in upgrades {
        let Some(kind) = path.strip_prefix(PREFIX) else { continue };
        let label = text(entry.get("name")).unwrap_or_else(|| kind.to_string());
        if kind.starts_with("Raw") {
            veiled.insert(path.clone(), json!(label));
            continue;
        }
        let mut stats = Map::new();
        for e in entry.get("upgradeEntries").and_then(Value::as_array).into_iter().flatten() {
            let Some(tag) = e.get("tag").and_then(Value::as_str) else { continue };
            let first = e.get("upgradeValues").and_then(Value::as_array).and_then(|v| v.first());
            let loc = text(first.and_then(|v| v.get("locTag")));
            stats.insert(
                tag.to_string(),
                json!({
                    "value": first.and_then(|v| v.get("value")).cloned().unwrap_or(json!(0)),
                    "prefix": text(e.get("prefixTag")).unwrap_or_default(),
                    "suffix": text(e.get("suffixTag")).unwrap_or_default(),
                    "label": loc.map_or_else(|| tag.to_string(), |l| clean_label(&l)),
                }),
            );
        }
        // Stats only DE's export has yet; its text is already in English.
        let de_entries = de.and_then(|d| d.get(path)).and_then(|e| e.get("upgradeEntries")).and_then(Value::as_array);
        for e in de_entries.into_iter().flatten() {
            let Some(tag) = e.get("tag").and_then(Value::as_str).filter(|t| !stats.contains_key(*t)) else { continue };
            let first = e.get("upgradeValues").and_then(Value::as_array).and_then(|v| v.first());
            let word = |key: &str| e.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
            stats.insert(
                tag.to_string(),
                json!({
                    "value": single(first.and_then(|v| v.get("value")).and_then(Value::as_f64).unwrap_or(0.0)),
                    "prefix": word("prefixTag"),
                    "suffix": word("suffixTag"),
                    "label": clean_label(first.and_then(|v| v.get("locTag")).and_then(Value::as_str).unwrap_or(tag)),
                }),
            );
        }
        types.insert(kind.to_string(), json!({ "label": label, "stats": stats }));
        for item in entry.get("compatibleItems").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            riven_for.insert(item.to_string(), kind.to_string());
        }
    }

    let mut riven_weapons = Map::new();
    for (path, w) in weapons {
        let Some(disposition) = w.get("omegaAttenuation") else { continue };
        let disposition = dispositions.get(path).map_or_else(|| disposition.clone(), |w| json!(w.disposition));
        let name = text(w.get("name")).unwrap_or_else(|| path.rsplit('/').next().unwrap_or_default().to_string());
        let mut entry = json!({ "name": name, "disposition": disposition });
        if let Some(base) = base_of(&riven_for, weapons, path) {
            entry["kind"] = json!(riven_for[&base]);
            entry["base"] = json!(base);
        }
        riven_weapons.insert(path.clone(), entry);
    }

    // Weapons newer than the export: the riven type and base of their family's riven weapon
    // (the base weapon's own entry first).
    let mut family: HashMap<&str, (Value, Value)> = HashMap::new();
    let mut known: Vec<(&String, &Value)> = riven_weapons.iter().collect();
    known.sort_by_key(|(p, e)| (e.get("base").and_then(Value::as_str) != Some(p.as_str()), (*p).clone()));
    for (path, entry) in known {
        let (Some(kind), Some(base), Some(w)) = (entry.get("kind"), entry.get("base"), dispositions.get(path)) else { continue };
        if !w.family.is_empty() {
            family.entry(w.family.as_str()).or_insert_with(|| (kind.clone(), base.clone()));
        }
    }
    let mut added = Map::new();
    for (path, w) in dispositions {
        if weapons.contains_key(path) || w.name.is_empty() {
            continue;
        }
        if let Some((kind, base)) = family.get(w.family.as_str()) {
            added.insert(path.clone(), json!({ "name": w.name, "disposition": w.disposition, "kind": kind, "base": base }));
        }
    }
    riven_weapons.extend(added);
    json!({ "source": SOURCE, "built": built, "types": types, "veiled": veiled, "weapons": riven_weapons })
}

/// The weapon in a riven type's `compatibleItems` that `path` descends from (via `parentName`).
fn base_of<'a>(riven_for: &HashMap<String, String>, weapons: &'a Map<String, Value>, start: &'a str) -> Option<String> {
    let mut path = Some(start);
    for _ in 0..16 {
        let p = path?;
        if riven_for.contains_key(p) {
            return Some(p.to_string());
        }
        path = weapons.get(p).and_then(|w| w.get("parentName")).and_then(Value::as_str);
    }
    None
}

/// `<DT_FIRE_COLOR>Heat` → `Heat`; one riven stat uses `|STAT1|` instead of `|val|`.
fn clean_label(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut in_tag = false;
    for c in label.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("|STAT1|", "|val|").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn checked_at(dir: &Path) -> Option<i64> {
    std::fs::read_to_string(dir.join("checked")).ok()?.trim().parse().ok()
}

/// Today's date (UTC) as `2026-10-09`.
fn today() -> String {
    let days = now_ms().div_euclid(86_400_000);
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Value, Value, Value) {
        let upgrades = json!({
            "/Lotus/Upgrades/Mods/Randomized/LotusRifleRandomModRare": {
                "name": "/L/RifleRiven",
                "compatibleItems": ["/W/Latron"],
                "upgradeEntries": [{
                    "tag": "WeaponFireDamageMod", "prefixTag": "/L/HeatPre", "suffixTag": "/L/HeatSuf",
                    "upgradeValues": [{ "value": 0.01, "locTag": "/L/Heat" }]
                }]
            },
            "/Lotus/Upgrades/Mods/Randomized/RawRifleRandomMod": { "name": "/L/Veiled" },
            "/Lotus/Upgrades/Mods/Other": { "name": "/L/Other" }
        });
        let weapons = json!({
            "/W/Latron": { "name": "/L/Latron", "omegaAttenuation": 1.4 },
            "/W/LatronPrime": { "name": "/L/LatronPrime", "parentName": "/W/Latron", "omegaAttenuation": 1.3 },
            "/W/Pet": { "name": "/L/Pet", "parentName": "/W/PetBase", "omegaAttenuation": 1.0 },
            "/W/NoRivens": { "name": "/L/Latron" }
        });
        let words = json!({
            "/L/RifleRiven": "Rifle Riven Mod", "/L/HeatPre": "igni", "/L/HeatSuf": "pha",
            "/L/Heat": "|val|% <DT_FIRE_COLOR>Heat", "/L/Veiled": "Veiled Rifle Riven",
            "/L/Latron": "Latron", "/L/LatronPrime": "Latron Prime", "/L/Pet": "Pet"
        });
        (upgrades, weapons, words)
    }

    #[test]
    fn builds_the_bundled_format() {
        let (u, w, d) = fixture();
        let built = build_data(&u, &w, &d, None, &BTreeMap::new(), "2026-10-09");
        let data = rivens::parse_data(&built.to_string()).unwrap();
        assert_eq!(data.built, "2026-10-09");
        let heat = &data.types["LotusRifleRandomModRare"].stats["WeaponFireDamageMod"];
        assert_eq!((heat.label.as_str(), heat.prefix.as_str(), heat.value), ("|val|% Heat", "igni", 0.01));
        assert_eq!(data.veiled["/Lotus/Upgrades/Mods/Randomized/RawRifleRandomMod"], "Veiled Rifle Riven");
        let prime = &data.weapons["/W/LatronPrime"];
        assert_eq!((prime.kind.as_deref(), prime.base.as_deref()), (Some("LotusRifleRandomModRare"), Some("/W/Latron")));
        assert!(data.weapons["/W/Pet"].kind.is_none());
        assert!(!data.weapons.contains_key("/W/NoRivens"));
        assert!(data.variants_differ("/W/Latron"));
    }

    #[test]
    fn des_export_adds_the_stats_public_export_plus_lacks() {
        // As DE ships it: one list, English text, float32 values, a line break inside a string.
        let de = b"{\"ExportUpgrades\":[{\"uniqueName\":\"/Lotus/Upgrades/Mods/Randomized/LotusRifleRandomModRare\",
            \"description\":[\"two\nlines\"],\"upgradeEntries\":[
            {\"tag\":\"WeaponFireDamageMod\",\"prefixTag\":\"other\",\"suffixTag\":\"other\",\"upgradeValues\":[{\"value\":0.0099999998,\"locTag\":\"|val|% <DT_FIRE_COLOR>Hot\"}]},
            {\"tag\":\"WeaponGasDamageMod\",\"prefixTag\":\"cali\",\"suffixTag\":\"fel\",\"upgradeValues\":[{\"value\":0.0099999998,\"locTag\":\"|val|% <DT_GAS_COLOR>Gas\"}]},
            {\"tag\":\"WeaponZoomFovMod\",\"prefixTag\":\"hera\",\"suffixTag\":\"lis\",\"upgradeValues\":[{\"value\":0.0046660001,\"locTag\":\"|val|% Zoom\"}]}]},
            {\"uniqueName\":\"/Lotus/Upgrades/Mods/Other\",\"upgradeEntries\":[]}]}";
        let de = de_rivens(de).expect("parses despite the line break");
        assert_eq!(de.as_object().unwrap().len(), 1, "rivens only");
        let (u, w, d) = fixture();
        let data = rivens::parse_data(&build_data(&u, &w, &d, Some(&de), &BTreeMap::new(), "a").to_string()).unwrap();
        let stats = &data.types["LotusRifleRandomModRare"].stats;
        let heat = &stats["WeaponFireDamageMod"];
        assert_eq!((heat.label.as_str(), heat.prefix.as_str()), ("|val|% Heat", "igni"), "Public Export Plus's own stats stay");
        let gas = &stats["WeaponGasDamageMod"];
        assert_eq!((gas.label.as_str(), gas.prefix.as_str(), gas.suffix.as_str(), gas.value), ("|val|% Gas", "cali", "fel", 0.01));
        assert_eq!(stats["WeaponZoomFovMod"].value, 0.004666);
        assert_eq!(single(-0.0049999999), -0.005);
    }

    /// DE's index decodes and names the upgrades file. Needs the network.
    #[test]
    #[ignore]
    fn live_des_export() {
        let dir = std::env::temp_dir().join("wf-companion-de-export-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("export")).unwrap();
        let mut etags = HashMap::new();
        assert!(fetch_de_upgrades(&dir, &mut etags).unwrap(), "first fetch saves it");
        assert!(etags[DE_UPGRADES].starts_with("ExportUpgrades_en.json!"));
        assert!(!fetch_de_upgrades(&dir, &mut etags).unwrap(), "unchanged the second time");
        let de = de_rivens(&std::fs::read(dir.join("export").join(DE_UPGRADES)).unwrap()).unwrap();
        let shotgun = &de["/Lotus/Upgrades/Mods/Randomized/LotusShotgunRandomModRare"]["upgradeEntries"];
        assert!(shotgun.as_array().unwrap().iter().any(|e| e["tag"] == "WeaponZoomFovMod"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lists_changed_dispositions() {
        let (u, w, d) = fixture();
        let old = rivens::parse_data(&build_data(&u, &w, &d, None, &BTreeMap::new(), "a").to_string()).unwrap();
        let mut w2 = w.clone();
        w2["/W/LatronPrime"]["omegaAttenuation"] = json!(1.35);
        let new = rivens::parse_data(&build_data(&u, &w2, &d, None, &BTreeMap::new(), "b").to_string()).unwrap();
        assert_eq!(differences(&old, &new), ["Latron Prime disposition 1.3 → 1.35"]);
    }

    #[test]
    fn wiki_dispositions_replace_the_exports() {
        let lua = "return {\n\t[\"Latron Prime\"] = {\n\t\tDisposition = 1.35,\n\t\tInternalName = \"/W/LatronPrime\",\n\t\tAttacks = {\n\t\t\t{\n\t\t\t\tDisposition = 9,\n\t\t\t},\n\t\t},\n\t},\n\tLatron = {\n\t\tInternalName = \"/W/Latron\",\n\t},\n\t['Kit (Primary)'] = {\n\t\tDisposition = 1.1,\n\t\tInternalName = \"/W/Kit\",\n\t},\n\t['Kit (Secondary)'] = {\n\t\tDisposition = 0.75,\n\t\tInternalName = \"/W/Kit\",\n\t},\n\tClaws = {\n\t\tDisposition = 0,\n\t\tInternalName = \"/W/Claws\",\n\t},\n}";
        let mut found = HashMap::new();
        wiki_dispositions(lua, &mut found);
        let wiki = settled(found);
        // No value, two different values (a kitgun) and 0 are all skipped; nested fields too.
        let only: Vec<(&str, f64)> = wiki.iter().map(|(p, w)| (p.as_str(), w.disposition)).collect();
        assert_eq!(only, [("/W/LatronPrime", 1.35)]);
        let (u, w, d) = fixture();
        let data = rivens::parse_data(&build_data(&u, &w, &d, None, &wiki, "a").to_string()).unwrap();
        assert_eq!(data.weapons["/W/LatronPrime"].disposition, 1.35);
        assert_eq!(data.weapons["/W/Latron"].disposition, 1.4, "the export's value without a wiki one");
    }

    #[test]
    fn weapons_only_the_wiki_has_join_their_family() {
        // Corufell Prime (update 44.0) isn't in the export yet; the wiki lists it in Corufell's
        // family, so its rivens use Corufell's riven type at its own disposition.
        let (u, w, d) = fixture();
        let entry = |disposition: f64, name: &str, family: &str| WikiWeapon { disposition, name: name.into(), family: family.into() };
        let wiki = BTreeMap::from([
            ("/W/Latron".to_string(), entry(1.4, "Latron", "Latron")),
            ("/W/LatronNew".to_string(), entry(0.5, "Latron Newer", "Latron")),
            ("/W/Orphan".to_string(), entry(0.9, "Orphan", "Nobody")),
            ("/W/NoRivens".to_string(), entry(1.0, "Latron", "Latron")),
        ]);
        let data = rivens::parse_data(&build_data(&u, &w, &d, None, &wiki, "a").to_string()).unwrap();
        let new = &data.weapons["/W/LatronNew"];
        assert_eq!((new.name.as_str(), new.disposition), ("Latron Newer", 0.5));
        assert_eq!((new.kind.as_deref(), new.base.as_deref()), (Some("LotusRifleRandomModRare"), Some("/W/Latron")));
        assert!(!data.weapons.contains_key("/W/Orphan"), "no family with rivens");
        assert!(!data.weapons.contains_key("/W/NoRivens"), "in the export without a disposition: not a riven weapon");
    }

    /// The live wiki has Haalvu at 0.6 (September 2026; the export still says 0.5). Needs the
    /// network.
    #[test]
    #[ignore]
    fn live_wiki_dispositions() {
        let wiki = fetch_wiki_dispositions().unwrap();
        assert!(wiki.len() > 550, "{}", wiki.len());
        assert_eq!(wiki["/Lotus/Weapons/Sentients/SentOctaMiniGun/SentOctaMiniGun"].disposition, 0.6);
        let prime = &wiki["/Lotus/Weapons/Tenno/Archwing/Melee/PrimeCorufell/PrimeCorufellScytheWeapon"];
        assert_eq!((prime.name.as_str(), prime.family.as_str(), prime.disposition), ("Corufell Prime", "Corufell", 0.5));
    }

    #[test]
    fn today_is_a_date() {
        let t = today();
        assert_eq!(t.len(), 10);
        assert!(t.starts_with("20"));
    }

    /// Downloads the export into a temp folder, then checks again: the second check should find
    /// nothing changed (ETags). Needs the network.
    #[test]
    #[ignore]
    fn live_update_then_unchanged() {
        let dir = std::env::temp_dir().join("wf-companion-riven-data-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("export")).unwrap();
        let first = update(&dir).unwrap();
        assert!(first.is_some(), "first run builds the data");
        assert!(dir.join("rivens.json").exists());
        let etags = std::fs::read_to_string(dir.join("etags.json")).unwrap();
        assert!(etags.contains("dict.en.json") && etags.contains(DE_UPGRADES), "{etags}");
        let built = rivens::parse_data(&std::fs::read_to_string(dir.join("rivens.json")).unwrap()).unwrap();
        let shotgun = &built.types["LotusShotgunRandomModRare"].stats;
        assert!(shotgun.contains_key("WeaponZoomFovMod") && shotgun.contains_key("WeaponGasDamageMod"), "DE's stats are in");
        assert_eq!(update(&dir).unwrap(), None, "nothing changed the second time");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The Rust port gives the same data as `gen_rivens.py`. Run with `WF_EXPORT_DIR` set to a
    /// folder with the three export files and the `wiki-dispositions.json` the bundled copy was
    /// made from.
    #[test]
    #[ignore]
    fn port_matches_the_script() {
        let dir = PathBuf::from(std::env::var("WF_EXPORT_DIR").expect("WF_EXPORT_DIR"));
        let read = |f: &str| serde_json::from_str::<Value>(&std::fs::read_to_string(dir.join(f)).unwrap()).unwrap();
        let bundled: Value = serde_json::from_str(include_str!("../data/rivens.json")).unwrap();
        let wiki: BTreeMap<String, WikiWeapon> = serde_json::from_str(&std::fs::read_to_string(dir.join(WIKI_CACHE)).unwrap()).unwrap();
        let de = std::fs::read(dir.join(DE_UPGRADES)).ok().and_then(|b| de_rivens(&b));
        let built = build_data(&read(FILES[0]), &read(FILES[1]), &read(FILES[2]), de.as_ref(), &wiki, bundled["built"].as_str().unwrap());
        assert_eq!(built, bundled);
    }
}
