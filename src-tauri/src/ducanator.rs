//! Ducanator: which prime parts give the most ducats for the platinum they're worth.
//!
//! - Numbers come from warframe.market's Ducats tool (`/v1/tools/ducats`, the data behind
//!   <https://warframe.market/tools/ducats>): per item and per period, the median and
//!   volume-weighted average (WA) price of the trades, the volume, and `ducats / price`.
//! - Rows use the previous day (steadier than one hour) and rank by ducats per platinum at the WA
//!   price: medians are mostly whole numbers, so the median ratio ties a lot.
//! - Items are matched through the cached `/v2/items` list (`id` → `gameRef`, the game path the
//!   inventory uses). Sets are dropped: Baro doesn't take them.
//! - Each part knows its set's progress (from WFCD's build parts) and its relic tier (from its
//!   ducat value), for the page's "don't include" filters.
//!
//! Being polite: one ~530 KB download, only when the page opens and the cached copy is over an
//! hour old (their numbers change hourly), through `market::get` (shared throttle, User-Agent).
//! No background polling. A failed download keeps showing the cached copy.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};

use crate::market::{self, FetchError};
use crate::pipeline::Shared;
use crate::translate::wfcd::BLUEPRINT_IMAGE;
use crate::translate::{mastery_xp, Flat, ItemIndex, Tab};

const TOOL_URL: &str = "https://api.warframe.market/v1/tools/ducats";
const ITEMS_URL: &str = "https://api.warframe.market/v2/items";
/// How long a downloaded copy is used before fetching again.
const MAX_AGE: Duration = Duration::from_secs(60 * 60);
/// Which list of the response the page shows.
const PERIOD: &str = "previous_day";
/// Inventory sections holding tradable prime parts: weapon parts and blueprints. Built warframe
/// components (also MiscItems) can't be traded, and aren't on the market list, so they never match.
const PART_SECTIONS: [&str; 2] = ["MiscItems", "Recipes"];

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DucatRow {
    pub path: String,
    pub name: String,
    pub image: Option<String>,
    /// For blueprints: the built item, drawn over the blueprint picture (as in Inventory).
    pub overlay: Option<String>,
    /// "warframe", "weapon", "companion" or "other" (archwing). Companions are parts of a set on
    /// the Foundry's Companion tab (sentinels, their weapons, pets) plus the Kavasa collar: the
    /// market tags sentinel parts as weapon parts.
    pub kind: String,
    /// Relic reward tier from the ducat value: "gold" (100), "silver" (45, 65), "bronze" (15, 25).
    pub tier: String,
    pub owned: i64,
    /// Progress on the set this part belongs to: parts owned and parts needed, counting a part
    /// needed twice as two. 0/0 when it isn't part of a known set.
    pub set_owned: u32,
    pub set_needed: u32,
    /// Copies beyond what the set needs (a dupe): safe to sell without setting the set back.
    pub spare: i64,
    /// The part builds an item you haven't mastered yet (any of its sets, if it's in more than
    /// one). False when it isn't part of a known set.
    pub for_unmastered: bool,
    pub ducats: i64,
    pub median: f64,
    pub wa_price: f64,
    pub volume: i64,
    /// Ducats per platinum at the WA price: the default sort.
    pub per_plat: f64,
    /// Ducats per platinum at the median price.
    pub per_plat_median: f64,
    /// Places moved in warframe.market's ranking over the last day (up is positive).
    pub trend_day: i64,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DucanatorData {
    pub rows: Vec<DucatRow>,
    pub account: Option<String>,
    /// When the shown copy was downloaded (ms since 1970).
    pub updated_at: Option<i64>,
    /// A download is under way; `ducanator-updated` fires when it ends.
    pub fetching: bool,
    /// The last download failed (the cached copy, if any, is still shown).
    pub error: Option<String>,
}

struct MarketItem {
    /// The game path (`gameRef`).
    path: String,
    set: bool,
    kind: &'static str,
    /// English name.
    name: String,
}

/// Parses `/v2/items` into warframe.market item id → item.
fn parse_items(bytes: &[u8]) -> Result<HashMap<String, MarketItem>, String> {
    let doc: Value = serde_json::from_slice(bytes).map_err(|e| format!("item list: {e}"))?;
    let list = doc.get("data").and_then(Value::as_array).ok_or("item list: no data array")?;
    Ok(list
        .iter()
        .filter_map(|i| {
            let id = i.get("id")?.as_str()?;
            let path = i.get("gameRef")?.as_str()?;
            let tags: Vec<&str> =
                i.get("tags").and_then(Value::as_array).map_or_else(Vec::new, |t| t.iter().filter_map(Value::as_str).collect());
            let kind = if tags.contains(&"warframe") {
                "warframe"
            } else if tags.contains(&"weapon") {
                "weapon"
            } else {
                "other"
            };
            let name = i.pointer("/i18n/en/name").and_then(Value::as_str).unwrap_or(path);
            let item = MarketItem { path: path.to_string(), set: tags.contains(&"set"), kind, name: name.to_string() };
            Some((id.to_string(), item))
        })
        .collect())
}

/// Relic reward tier from a part's ducat value.
fn tier(ducats: i64) -> &'static str {
    match ducats {
        100.. => "gold",
        45.. => "silver",
        _ => "bronze",
    }
}

/// A set's progress: parts owned (each counted up to the number needed) and parts needed.
#[derive(Clone, Copy, Default)]
struct SetProgress {
    owned: u32,
    needed: u32,
    companion: bool,
    /// For one part: copies had (blueprint or built) beyond what the set needs.
    spare: i64,
    /// For one part: some set it's in isn't mastered.
    for_unmastered: bool,
}

impl SetProgress {
    fn share(self) -> f64 {
        if self.needed == 0 {
            0.0
        } else {
            f64::from(self.owned) / f64::from(self.needed)
        }
    }
}

/// Each part's set progress (by the part's market path; a part in two sets takes the one you have
/// more of). Sets come from WFCD's build parts. A warframe's parts are listed there as the built
/// component (`…HelmetComponent`) while the market trades the blueprint (`…HelmetBlueprint`): the
/// blueprint is the set part, and either one counts as having it.
fn set_progress(
    index: &ItemIndex,
    tradable: &HashSet<&str>,
    owned: &HashMap<&str, i64>,
    current: &Flat,
) -> HashMap<String, SetProgress> {
    let mut parts: HashMap<String, SetProgress> = HashMap::new();
    let mut unmastered_parts: HashSet<String> = HashSet::new();
    for m in index.masterables() {
        let members: Vec<(String, &str, u32)> = m
            .parts
            .iter()
            .filter_map(|p| {
                let market = if tradable.contains(p.path.as_str()) {
                    p.path.clone()
                } else {
                    let bp = format!("{}Blueprint", p.path.strip_suffix("Component")?);
                    tradable.contains(bp.as_str()).then_some(bp)?
                };
                Some((market, p.path.as_str(), p.count.max(1)))
            })
            .collect();
        if members.is_empty() {
            continue;
        }
        let mut progress = SetProgress { companion: m.tab == Tab::Companion, ..Default::default() };
        let mut spares = Vec::with_capacity(members.len());
        for (market, component, needed) in &members {
            let mut have = owned.get(market.as_str()).copied().unwrap_or(0);
            if component != market {
                have += owned.get(component).copied().unwrap_or(0);
            }
            progress.owned += have.clamp(0, i64::from(*needed)) as u32;
            progress.needed += needed;
            spares.push((have - i64::from(*needed)).max(0));
        }
        let mastered = mastery_xp(current, &m.path) >= m.curve.xp_for_rank(m.max_rank);
        for ((market, _, _), spare) in members.into_iter().zip(spares) {
            if !mastered {
                unmastered_parts.insert(market.clone());
            }
            let best = parts.entry(market).or_default();
            if best.needed == 0 || progress.share() > best.share() {
                *best = SetProgress { spare, ..progress };
            }
        }
    }
    for (path, p) in &mut parts {
        p.for_unmastered = unmastered_parts.contains(path);
    }
    parts
}

/// Builds the rows from the tool response and the item list. Pure, for tests.
pub fn build(tool: &[u8], items: &[u8], index: &ItemIndex, current: &Flat) -> Result<Vec<DucatRow>, String> {
    let doc: Value = serde_json::from_slice(tool).map_err(|e| format!("ducats: {e}"))?;
    let list = doc
        .pointer(&format!("/payload/{PERIOD}"))
        .and_then(Value::as_array)
        .ok_or("ducats: no previous_day list")?;
    let items = parse_items(items)?;
    let mut owned: HashMap<&str, i64> = HashMap::new();
    for ((section, item), n) in current {
        if PART_SECTIONS.contains(&section.as_str()) {
            *owned.entry(item.as_str()).or_insert(0) += n;
        }
    }
    let tradable: HashSet<&str> = items.values().filter(|i| !i.set).map(|i| i.path.as_str()).collect();
    let part_sets = set_progress(index, &tradable, &owned, current);
    let num = |r: &Value, k: &str| r.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let mut rows: Vec<DucatRow> = list
        .iter()
        .filter_map(|r| {
            let item = items.get(r.get("item")?.as_str()?)?;
            // Sets aren't listed: Baro doesn't take them.
            if item.set {
                return None;
            }
            let path = &item.path;
            let ducats = num(r, "ducats") as i64;
            let image = index.get(path).and_then(|i| i.image.clone());
            let blueprint = path.ends_with("Blueprint");
            let progress = part_sets.get(path).copied().unwrap_or_default();
            let companion = progress.companion || path.contains("/SentinelRecipes/") || path.contains("/Kubrow/");
            Some(DucatRow {
                path: path.clone(),
                // The market's names: WFCD has no entry for warframe part blueprints, and doubles
                // up some others ("Kavasa Prime Kubrow Collar Kavasa Prime Band").
                name: item.name.clone(),
                image: if blueprint { image.or_else(|| Some(BLUEPRINT_IMAGE.to_string())) } else { image },
                overlay: if blueprint { index.blueprint_overlay(path) } else { None },
                kind: if companion { "companion" } else { item.kind }.to_string(),
                tier: tier(ducats).to_string(),
                owned: owned.get(path.as_str()).copied().unwrap_or(0),
                set_owned: progress.owned,
                set_needed: progress.needed,
                // Only tradable copies (blueprints, weapon parts) can be sold.
                spare: progress.spare.min(owned.get(path.as_str()).copied().unwrap_or(0)),
                for_unmastered: progress.for_unmastered,
                ducats,
                median: num(r, "median"),
                wa_price: num(r, "wa_price"),
                volume: num(r, "volume") as i64,
                per_plat: num(r, "ducats_per_platinum_wa"),
                per_plat_median: num(r, "ducats_per_platinum"),
                trend_day: num(r, "position_change_day") as i64,
            })
        })
        .collect();
    rows.sort_by(|a, b| b.per_plat.total_cmp(&a.per_plat).then_with(|| a.name.cmp(&b.name)));
    Ok(rows)
}

fn modified_ms(path: &std::path::Path) -> Option<i64> {
    let t = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    Some(t.duration_since(UNIX_EPOCH).ok()?.as_millis() as i64)
}

fn is_fresh(path: &std::path::Path, max_age: Duration) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < max_age)
}

fn download(url: &str, to: &std::path::Path) -> Result<(), String> {
    let bytes = match market::get(url) {
        Ok(b) => b,
        Err(FetchError::NotFound) => return Err(format!("{url}: not found")),
        Err(FetchError::Retry(e)) => return Err(e),
    };
    let tmp = to.with_extension("json.tmp");
    std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, to).map_err(|e| e.to_string())
}

#[derive(Default)]
struct Status {
    fetching: bool,
    error: Option<String>,
}

/// The ducat data service: the cache folder and the state of the one download.
pub struct Ducanator {
    dir: PathBuf,
    status: Arc<Mutex<Status>>,
}

impl Ducanator {
    /// `dir` is the market cache folder (shared with `market.rs`'s item list).
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, status: Arc::default() }
    }

    fn tool_file(&self) -> PathBuf {
        self.dir.join("ducats.json")
    }

    fn items_file(&self) -> PathBuf {
        self.dir.join("items.json")
    }

    /// Starts a download if the cached copy is missing or over an hour old and none is running.
    fn refresh_if_old(&self, app: &AppHandle) {
        if is_fresh(&self.tool_file(), MAX_AGE) && self.items_file().exists() {
            return;
        }
        {
            let mut s = self.status.lock().unwrap();
            if s.fetching {
                return;
            }
            s.fetching = true;
        }
        let (dir, tool, items, status, app) =
            (self.dir.clone(), self.tool_file(), self.items_file(), self.status.clone(), app.clone());
        std::thread::spawn(move || {
            let _ = std::fs::create_dir_all(&dir);
            // The item list is normally there already (mod prices download it); fetch it if not.
            let result = (if items.exists() { Ok(()) } else { download(ITEMS_URL, &items) })
                .and_then(|()| download(TOOL_URL, &tool));
            if let Err(e) = &result {
                eprintln!("[ducanator] download failed: {e}");
            }
            {
                let mut s = status.lock().unwrap();
                s.fetching = false;
                s.error = result.err();
            }
            let _ = app.emit("ducanator-updated", ());
        });
    }
}

#[tauri::command]
pub fn get_ducanator(
    app: AppHandle,
    shared: State<'_, Arc<Shared>>,
    ducanator: State<'_, Ducanator>,
) -> Result<DucanatorData, String> {
    ducanator.refresh_if_old(&app);
    let index = shared.items.read().unwrap().clone();
    let (account, current) = {
        let guard = shared.store.lock().unwrap();
        let store = guard.as_ref();
        let account = store.and_then(|s| s.latest_account().ok().flatten());
        let current = match (store, &account) {
            (Some(s), Some(a)) => s.current(a).map_err(|e| e.to_string())?,
            _ => Flat::new(),
        };
        (account, current)
    };
    let tool = std::fs::read(ducanator.tool_file()).ok();
    let items = std::fs::read(ducanator.items_file()).ok();
    let mut error = ducanator.status.lock().unwrap().error.clone();
    let rows = match (&tool, &items) {
        (Some(t), Some(i)) => build(t, i, &index, &current).unwrap_or_else(|e| {
            error.get_or_insert(e);
            Vec::new()
        }),
        _ => Vec::new(),
    };
    let status = ducanator.status.lock().unwrap();
    Ok(DucanatorData {
        rows,
        account,
        updated_at: tool.as_ref().and_then(|_| modified_ms(&ducanator.tool_file())),
        fetching: status.fetching,
        error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOOL: &str = r#"{"payload":{
        "previous_hour":[],
        "previous_day":[
            {"item":"a","ducats":45,"median":3,"wa_price":3.86,"volume":304,"ducats_per_platinum":15.0,"ducats_per_platinum_wa":11.66,"position_change_day":5},
            {"item":"b","ducats":100,"median":4,"wa_price":4.5,"volume":90,"ducats_per_platinum":25.0,"ducats_per_platinum_wa":22.22,"position_change_day":-3},
            {"item":"s","ducats":60,"median":20,"wa_price":21,"volume":40,"ducats_per_platinum":3.0,"ducats_per_platinum_wa":2.86,"position_change_day":0},
            {"item":"unknown","ducats":15,"median":1,"wa_price":1,"volume":1,"ducats_per_platinum":15,"ducats_per_platinum_wa":15,"position_change_day":0}
        ]}}"#;
    const ITEMS: &str = r#"{"data":[
        {"id":"a","gameRef":"/Lotus/Types/Recipes/Weapons/WeaponParts/AkbroncoPrimeLink","tags":["component","prime"],"i18n":{"en":{"name":"Akbronco Prime Link"}}},
        {"id":"b","gameRef":"/Lotus/Types/Recipes/WarframeRecipes/CalibanPrimeHelmetBlueprint","tags":["blueprint","prime"],"i18n":{"en":{"name":"Caliban Prime Neuroptics Blueprint"}}},
        {"id":"s","gameRef":"/Lotus/Weapons/Tenno/Akimbo/PrimeAkimboShotGun","tags":["set","prime"],"i18n":{"en":{"name":"Akbronco Prime Set"}}}
    ]}"#;

    #[test]
    fn ranks_parts_by_ducats_per_plat_and_counts_owned() {
        let index = ItemIndex::build(&[]);
        let mut current = Flat::new();
        current.insert(("MiscItems".into(), "/Lotus/Types/Recipes/Weapons/WeaponParts/AkbroncoPrimeLink".into()), 2);
        current.insert(("Recipes".into(), "/Lotus/Types/Recipes/WarframeRecipes/CalibanPrimeHelmetBlueprint".into()), 1);
        // Counted only from tradable sections.
        current.insert(("Suits".into(), "/Lotus/Types/Recipes/WarframeRecipes/CalibanPrimeHelmetBlueprint".into()), 5);
        let rows = build(TOOL.as_bytes(), ITEMS.as_bytes(), &index, &current).unwrap();
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        // Set and unknown id dropped, best ratio first.
        assert_eq!(names, ["Caliban Prime Neuroptics Blueprint", "Akbronco Prime Link"]);
        assert_eq!((rows[0].tier.as_str(), rows[1].tier.as_str()), ("gold", "silver"));
        let link = &rows[1];
        assert_eq!((link.owned, link.ducats, link.volume, link.trend_day), (2, 45, 304, 5));
        assert_eq!((link.per_plat, link.per_plat_median, link.wa_price, link.median), (11.66, 15.0, 3.86, 3.0));
        assert_eq!(rows[0].owned, 1);
        // The ratio is ducats / price, as on warframe.market.
        assert!((link.ducats as f64 / link.wa_price - link.per_plat).abs() < 0.01);
    }

    #[test]
    fn set_progress_counts_blueprints_and_built_components() {
        // Caliban Prime: WFCD lists the warframe parts as built components, the market trades the
        // blueprints.
        let wr = "/Lotus/Types/Recipes/WarframeRecipes/CalibanPrime";
        let warframes = format!(
            r#"[{{"uniqueName":"/Lotus/Powersuits/Sentient/CalibanPrime","name":"Caliban Prime","masterable":true,"isPrime":true,
                "components":[{{"uniqueName":"{wr}Blueprint","itemCount":1}},{{"uniqueName":"{wr}HelmetComponent","itemCount":1}},
                {{"uniqueName":"{wr}ChassisComponent","itemCount":1}},{{"uniqueName":"{wr}SystemsComponent","itemCount":1}},
                {{"uniqueName":"/Lotus/Types/Items/MiscItems/OrokinCell","itemCount":5}}]}}]"#
        );
        let index = ItemIndex::build(&[("Warframes".into(), warframes.into_bytes())]);
        let items = format!(
            r#"{{"data":[
            {{"id":"bp","gameRef":"{wr}Blueprint","tags":["warframe","prime","blueprint"]}},
            {{"id":"he","gameRef":"{wr}HelmetBlueprint","tags":["component","warframe","prime","blueprint"]}},
            {{"id":"ch","gameRef":"{wr}ChassisBlueprint","tags":["component","warframe","prime","blueprint"]}},
            {{"id":"sy","gameRef":"{wr}SystemsBlueprint","tags":["component","warframe","prime","blueprint"]}},
            {{"id":"set","gameRef":"/Lotus/Powersuits/Sentient/CalibanPrime","tags":["warframe","prime","set"],"i18n":{{"en":{{"name":"Caliban Prime Set"}}}}}}]}}"#
        );
        let row = |id: &str, ducats: i64| {
            format!(r#"{{"item":"{id}","ducats":{ducats},"median":5,"wa_price":5,"volume":10,"ducats_per_platinum":1,"ducats_per_platinum_wa":1,"position_change_day":0}}"#)
        };
        let tool = format!(
            r#"{{"payload":{{"previous_day":[{},{},{},{},{}]}}}}"#,
            row("bp", 45),
            row("he", 100),
            row("ch", 15),
            row("sy", 100),
            row("set", 260)
        );
        let mut current = Flat::new();
        current.insert(("Recipes".into(), format!("{wr}Blueprint")), 1);
        // A built Neuroptics counts as having that part.
        current.insert(("MiscItems".into(), format!("{wr}HelmetComponent")), 1);
        let rows = build(tool.as_bytes(), items.as_bytes(), &index, &current).unwrap();
        let get = |p: &str| rows.iter().find(|r| r.path == p).unwrap();
        let chassis = get(&format!("{wr}ChassisBlueprint"));
        assert_eq!((chassis.set_owned, chassis.set_needed, chassis.kind.as_str()), (2, 4, "warframe"));
        assert_eq!(chassis.spare, 0);
        // Caliban Prime has no mastery XP here: its parts are for an unmastered item.
        assert!(chassis.for_unmastered);
        let mut mastered = current.clone();
        mastered.insert(("XPInfo".into(), "/Lotus/Powersuits/Sentient/CalibanPrime".into()), 1_000_000);
        let rows_m = build(tool.as_bytes(), items.as_bytes(), &index, &mastered).unwrap();
        assert!(rows_m.iter().all(|r| !r.for_unmastered));
        // Three Caliban Prime Blueprints: two are spare. A built Neuroptics plus its blueprint: the
        // blueprint is a spare copy.
        let mut more = current.clone();
        more.insert(("Recipes".into(), format!("{wr}Blueprint")), 3);
        more.insert(("Recipes".into(), format!("{wr}HelmetBlueprint")), 1);
        let rows = build(tool.as_bytes(), items.as_bytes(), &index, &more).unwrap();
        let get = |p: &str| rows.iter().find(|r| r.path == p).unwrap();
        assert_eq!(get(&format!("{wr}Blueprint")).spare, 2);
        assert_eq!(get(&format!("{wr}HelmetBlueprint")).spare, 1);
        assert_eq!(get(&format!("{wr}ChassisBlueprint")).spare, 0);
        assert_eq!(chassis.tier, "bronze");
        assert!(rows.iter().all(|r| !r.path.starts_with("/Lotus/Powersuits/")), "no set rows");
    }

    /// Against the real cached data: `LOTUS_DUCATS=<path to a /v1/tools/ducats download>`, plus the
    /// app's own `market/items.json`, `wfcd/` and `latest_inventory.json`.
    #[test]
    #[ignore]
    fn real_data() {
        let app = std::path::PathBuf::from(std::env::var("APPDATA").unwrap()).join("io.lotus.companion");
        let tool = std::fs::read(std::env::var("LOTUS_DUCATS").unwrap()).unwrap();
        let items = std::fs::read(app.join("market/items.json")).unwrap();
        let index = ItemIndex::build(&crate::translate::wfcd::load_cached(&app.join("wfcd")));
        let inv = crate::translate::parse(&std::fs::read(app.join("latest_inventory.json")).unwrap()).unwrap();
        let rows = build(&tool, &items, &index, &inv.values).unwrap();
        let owned: Vec<&DucatRow> = rows.iter().filter(|r| r.owned > 0).collect();
        println!("{} rows, {} owned, {} copies, {} ducats", rows.len(), owned.len(),
            owned.iter().map(|r| r.owned).sum::<i64>(), owned.iter().map(|r| r.owned * r.ducats).sum::<i64>());
        for r in owned.iter().take(5).chain(owned.iter().rev().take(2)) {
            println!("{:<40} owned {:>2} {:>3}d wa {:>6.2} {:>6.2}/p img {:?} over {:?}", r.name, r.owned, r.ducats, r.wa_price, r.per_plat, r.image, r.overlay);
        }
        println!("no image: {}", rows.iter().filter(|r| r.image.is_none()).count());
        for r in rows.iter().filter(|r| r.image.is_none()).take(8) {
            println!("  no image: {} {}", r.name, r.path);
        }
        for r in rows.iter().filter(|r| r.path.ends_with("Blueprint") && r.overlay.is_none()).take(8) {
            println!("  blueprint without the built item: {} {}", r.name, r.path);
        }
        let parts: Vec<&DucatRow> = rows.iter().collect();
        assert_eq!(parts.len(), 594);
        let in_set = parts.iter().filter(|r| r.set_needed > 0).count();
        let half = parts.iter().filter(|r| r.set_needed > 0 && r.set_owned * 2 >= r.set_needed).count();
        println!("parts in a known set: {in_set}/594; in a set you own half or more of: {half}");
        println!("parts for unmastered items: {} (owned: {})", parts.iter().filter(|r| r.for_unmastered).count(), parts.iter().filter(|r| r.for_unmastered && r.owned > 0).count());
        let spare: Vec<&&DucatRow> = parts.iter().filter(|r| r.spare > 0).collect();
        println!("parts with spare copies: {} ({} spare copies); kept by \"Except dupes\" at 50%: {}",
            spare.len(), spare.iter().map(|r| r.spare).sum::<i64>(),
            spare.iter().filter(|r| r.set_owned * 2 >= r.set_needed).count());
        for r in parts.iter().filter(|r| r.set_needed == 0).take(6) {
            println!("  no set: {} {}", r.name, r.path);
        }
        let mut by = std::collections::BTreeMap::new();
        for r in &parts {
            *by.entry((r.kind.clone(), r.tier.clone())).or_insert(0) += 1;
        }
        println!("{by:?}");
    }

    #[test]
    fn cache_age() {
        let dir = std::env::temp_dir().join("lotus-ducanator-test");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("ducats.json");
        std::fs::write(&f, TOOL).unwrap();
        assert!(is_fresh(&f, MAX_AGE));
        assert!(!is_fresh(&f, Duration::ZERO));
        assert!(!is_fresh(&dir.join("missing.json"), MAX_AGE));
    }
}
