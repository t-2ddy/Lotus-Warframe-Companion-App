//! The Rivens page: every riven on the account with its weapon, generated name and stats.
//!
//! A riven's `UpgradeFingerprint` holds the weapon (`compat`), rank (`lvl`, missing = 0), and each
//! rolled stat as a tag plus a 30-bit roll. Turning that into numbers needs each stat's base value
//! and name parts per riven type, and the weapon's disposition (`omegaAttenuation`). Those come
//! from Warframe Public Export Plus, bundled as `data/rivens.json` (`scripts/gen_rivens.py`).
//! The formulas and name rules follow calamity-inc's warframe-riven-info (`RivenParser.js`).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::pipeline::Shared;
use crate::riven_grades::{best_stats, grade, rule_for, BestStats, GradeSheet};
use crate::translate::{parse, Flat, OwnedRiven};

/// Stats are shown at this rank (max), the way rivens are compared and traded.
pub const MAX_RANK: u32 = 8;

/// Every riven's disposition: DE set all weapons to 1.2 (October 2026), so the per-weapon values
/// in the data no longer apply.
pub const DISPOSITION: f64 = 1.2;

#[derive(Debug, Deserialize)]
pub struct RivenData {
    /// When the file was made (`2026-10-09`); the newer of the bundled copy and a download wins.
    #[serde(default)]
    pub built: String,
    pub types: HashMap<String, RivenType>,
    /// Veiled riven path → label, e.g. "Rifle Riven Mod".
    pub veiled: HashMap<String, String>,
    pub weapons: HashMap<String, Weapon>,
}

#[derive(Debug, Deserialize)]
pub struct RivenType {
    pub label: String,
    pub stats: HashMap<String, StatInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StatInfo {
    /// Base value per rank at disposition 1; negative for stats where less is better (recoil).
    pub value: f64,
    pub prefix: String,
    pub suffix: String,
    /// Stat text with `|val|` where the number goes, e.g. `|val|% Critical Chance`.
    pub label: String,
}

#[derive(Debug, Deserialize)]
pub struct Weapon {
    pub name: String,
    pub disposition: f64,
    /// The riven type this weapon takes (`LotusPistolRandomModRare`); none for items that take
    /// no rivens (Exalted weapons, Amps, pets).
    #[serde(default)]
    pub kind: Option<String>,
    /// The weapon a riven is for: Latron for Latron Prime and Latron Wraith.
    #[serde(default)]
    pub base: Option<String>,
}

impl RivenData {
    /// The riven type for a weapon path.
    pub fn kind_of(&self, weapon: &str) -> Option<&str> {
        self.weapons.get(weapon)?.kind.as_deref()
    }

    /// Every weapon sharing `weapon`'s base (itself included).
    pub fn family(&self, weapon: &str) -> Vec<(&str, &Weapon)> {
        let Some(base) = self.weapons.get(weapon).and_then(|w| w.base.as_deref()) else {
            return self.weapons.get_key_value(weapon).map(|(p, w)| vec![(p.as_str(), w)]).unwrap_or_default();
        };
        let mut out: Vec<(&str, &Weapon)> =
            self.weapons.iter().filter(|(_, w)| w.base.as_deref() == Some(base)).map(|(p, w)| (p.as_str(), w)).collect();
        out.sort_by(|a, b| a.0.cmp(b.0));
        out
    }

    /// The weapon's variants don't all share one disposition (Latron 1.4, Latron Prime 1.3), so a
    /// card's numbers depend on which one it's shown with.
    pub fn variants_differ(&self, weapon: &str) -> bool {
        let family = self.family(weapon);
        family.iter().any(|(_, w)| (w.disposition - family[0].1.disposition).abs() > 1e-6)
    }

    fn parse(text: &str) -> Result<RivenData, String> {
        let mut data: RivenData = serde_json::from_str(text).map_err(|e| e.to_string())?;
        // Tests keep each weapon's old value: their cards were captured before the change.
        if !cfg!(test) {
            data.flatten_dispositions();
        }
        data.fill_factions();
        Ok(data)
    }

    /// Every weapon at [`DISPOSITION`], whatever the data lists.
    fn flatten_dispositions(&mut self) {
        for w in self.weapons.values_mut() {
            w.disposition = DISPOSITION;
        }
    }

    /// Archgun rivens no longer roll Damage to Infested, so the export leaves it out, but older
    /// archgun rivens still have it. Every type with some faction damage gets all the factions
    /// any type has, worth the same as its own (they're all 0.005).
    fn fill_factions(&mut self) {
        const MARK: &str = "FactionDamage";
        // Faction → (label, prefix, suffix), the same in every type.
        let mut templates: HashMap<String, StatInfo> = HashMap::new();
        for t in self.types.values() {
            for (tag, s) in &t.stats {
                if let Some((_, faction)) = tag.split_once(MARK) {
                    templates.entry(faction.to_string()).or_insert_with(|| s.clone());
                }
            }
        }
        for t in self.types.values_mut() {
            // "WeaponFactionDamage" or "WeaponMeleeFactionDamage", and what a faction is worth.
            let Some((stem, value)) = t.stats.iter().find_map(|(tag, s)| {
                tag.split_once(MARK).map(|(before, _)| (format!("{before}{MARK}"), s.value))
            }) else {
                continue;
            };
            for (faction, s) in &templates {
                t.stats.entry(format!("{stem}{faction}")).or_insert_with(|| StatInfo { value, ..s.clone() });
            }
        }
    }
}

static DATA: RwLock<Option<Arc<RivenData>>> = RwLock::new(None);

/// The bundled copy (`data/rivens.json`).
pub fn bundled() -> RivenData {
    RivenData::parse(include_str!("../data/rivens.json")).expect("bundled rivens.json is valid")
}

/// The riven data in use: a download if one is newer than the bundled copy (see
/// `riven_data`), else the bundled copy.
pub fn data() -> Arc<RivenData> {
    if let Some(d) = DATA.read().unwrap().as_ref() {
        return d.clone();
    }
    let mut slot = DATA.write().unwrap();
    slot.get_or_insert_with(|| Arc::new(bundled())).clone()
}

/// Swaps in newer riven data (from a download).
pub fn set_data(d: RivenData) {
    *DATA.write().unwrap() = Some(Arc::new(d));
}

/// Parses a riven data file in the bundled format.
pub fn parse_data(text: &str) -> Result<RivenData, String> {
    RivenData::parse(text)
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Fingerprint {
    compat: Option<String>,
    lvl: Option<u32>,
    lvl_req: Option<u32>,
    rerolls: Option<u32>,
    pol: Option<String>,
    #[serde(default)]
    buffs: Vec<Roll>,
    #[serde(default)]
    curses: Vec<Roll>,
    challenge: Option<Challenge>,
}

#[derive(Debug, Clone, Deserialize)]
struct Roll {
    #[serde(rename = "Tag")]
    tag: String,
    #[serde(rename = "Value")]
    value: u32,
}

#[derive(Debug, Deserialize)]
struct Challenge {
    #[serde(rename = "Type", default)]
    kind: String,
    #[serde(rename = "Progress", default)]
    progress: u32,
    #[serde(rename = "Required", default)]
    required: u32,
}

/// Number-of-stats multipliers, indexed by count (RivenParser.js).
const BUFF_COUNT_ATTEN: [f64; 6] = [0.0, 1.0, 0.66000003, 0.5, 0.40000001, 0.34999999];
const CURSE_BUFF_ATTEN: [f64; 6] = [0.0, 1.0, 0.33000001, 0.5, 1.25, 1.5];

fn at(table: &[f64; 6], n: usize) -> f64 {
    table[n.min(table.len() - 1)]
}

/// A roll's position in its 90–110% range, 0.0–1.0.
fn roll_fraction(v: u32) -> f64 {
    let f = f64::from(v) / f64::from(0x3FFF_FFFFu32);
    if (0.0..=1.0).contains(&f) { f } else { 0.0 }
}

/// Spliced traits (Update 44.1) merge two traits into one. The combined elements roll in the
/// same range as a single element, so they're worked out like Heat (tag `Spliced<Name>`).
pub const SPLICED_ELEMENTS: [&str; 6] = ["Corrosive", "Viral", "Blast", "Gas", "Radiation", "Magnetic"];

/// The other spliced traits, as the card words them (wiki: Riven Splicer, checked 2026-10-09;
/// it uses both "Slam Damage" and "Slam Attack Damage", and both "Magazine Reload While
/// Holstered" and "Reload While Holstered"). Their ranges aren't known yet, so they're shown
/// but not graded (tag `Pending:<Name>`). "Windup" and "Wind Up" read the same once spaces go.
pub const SPLICED_PENDING: &[&str] = &[
    // Ranged
    "Weakpoint Damage",
    "Weakpoint Critical Chance",
    "Ammo Efficiency",
    "Magazine Reload While Holstered",
    "Reload While Holstered",
    "Status Damage",
    // Melee
    "Heavy Attack Damage",
    "Heavy Attack Windup Speed",
    "Parry Angle",
    "Slam Attack Damage",
    "Slam Damage",
    // All
    "Damage to Orokin",
    "Damage to Techrot",
    "Damage to Scaldra",
];

pub const PENDING_PREFIX: &str = "Pending:";
/// A stat read off a card that this riven type can't roll any more (or never could), kept as
/// read: `Legacy:WeaponZoomFovMod` when another type still rolls it, `Legacy:<label>` if not.
pub const LEGACY_PREFIX: &str = "Legacy:";
/// A stat on a riven read off a card whose number couldn't be read (`Unread:WeaponToxinDamageMod`):
/// a damaged number, or a positive the riven's name lists but the card read missed. Put back by
/// the overlay as "not read".
pub const UNREAD_PREFIX: &str = "Unread:";

/// A stat this riven type can no longer roll: DE removes stats from types over time (negative
/// Impact on older weapons' rivens, Zoom and Projectile Speed on shotgun rivens), and rivens
/// rolled before keep them. Its data from a type that still rolls it, for the name and grading
/// sheet; `None` if the type rolls it, it's spliced, or no type has it.
pub fn legacy_stat(data: &RivenData, kind: &RivenType, tag: &str) -> Option<StatInfo> {
    let tag = tag.strip_prefix(LEGACY_PREFIX).unwrap_or(tag);
    if kind.stats.contains_key(tag) || spliced_element(tag).is_some() || tag.starts_with(PENDING_PREFIX) {
        return None;
    }
    let mut found: Vec<(&String, &StatInfo)> = data.types.iter().filter_map(|(k, t)| t.stats.get(tag).map(|s| (k, s))).collect();
    found.sort_by_key(|(k, _)| *k);
    found.first().map(|(_, s)| (*s).clone())
}

/// A stat that only rolls as a negative: stored with a negative value and no name parts, so it
/// can't be a positive (melee's "Chance to Gain Combo Count"). Spliced elements have no name
/// parts either, but positive values.
pub fn curse_only(s: &StatInfo) -> bool {
    s.prefix.is_empty() && s.suffix.is_empty() && s.value < 0.0
}

/// The value a curse's numbers are worked out from: as stored, except a negative-only stat's
/// is turned so its curse comes out negative, as the game shows it ("-12.4% Chance to Gain
/// Combo Count").
pub fn curse_base(s: &StatInfo) -> f64 {
    if curse_only(s) { s.value.abs() } else { s.value }
}

/// A stat label without its number: `|val|% Zoom` → "Zoom".
pub fn label_name(label: &str) -> String {
    let rest = label.replace("|val|", "");
    let rest = rest.trim_start();
    rest.strip_prefix('%').or_else(|| rest.strip_prefix("s ")).unwrap_or(rest).trim().to_string()
}

/// A combined element's name from its tag: the game's on owned rivens (`WeaponViralDamageMod`,
/// seen in a capture 2026-10-09) or the overlay's (`SplicedViral`).
pub fn spliced_element(tag: &str) -> Option<&'static str> {
    SPLICED_ELEMENTS.iter().copied().find(|e| {
        tag.strip_prefix("Spliced") == Some(e)
            || tag.strip_prefix("Weapon").and_then(|t| t.strip_suffix("DamageMod")) == Some(e)
    })
}

/// A stat's data, including spliced elements (Heat's numbers under the element's name; an owned
/// Latron riven's Viral roll matches them).
pub fn stat_info(kind: &RivenType, tag: &str) -> Option<StatInfo> {
    if let Some(s) = kind.stats.get(tag) {
        return Some(s.clone());
    }
    let name = spliced_element(tag)?;
    let heat = kind.stats.get("WeaponFireDamageMod")?;
    Some(StatInfo { value: heat.value, prefix: String::new(), suffix: String::new(), label: format!("|val|% {name}") })
}

/// A spliced trait, or one this riven type's data doesn't have: a combined element, a pending
/// trait read off a card, or an unknown tag on an owned riven (`WeaponStatusDamage`).
pub fn is_spliced(data: &RivenData, kind: &RivenType, tag: &str) -> bool {
    if tag.starts_with(LEGACY_PREFIX) || tag.starts_with(UNREAD_PREFIX) || legacy_stat(data, kind, tag).is_some() {
        return false;
    }
    tag.starts_with(PENDING_PREFIX) || spliced_element(tag).is_some() || !kind.stats.contains_key(tag)
}

/// A readable name for a tag the data doesn't have: `WeaponStatusDamage` → "Status Damage",
/// `WeaponFactionDamageInfested` → "Damage to Infested".
pub fn trait_name(tag: &str) -> String {
    let core = tag.strip_prefix("Weapon").unwrap_or(tag);
    let core = core.strip_suffix("Mod").unwrap_or(core);
    if let Some(faction) = core.strip_prefix("FactionDamage").or_else(|| core.strip_prefix("MeleeFactionDamage")) {
        return format!("Damage to {faction}");
    }
    let mut out = String::new();
    for (i, c) in core.chars().enumerate() {
        if i > 0 && c.is_uppercase() {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

fn is_faction(tag: &str) -> bool {
    tag.contains("FactionDamage")
}

/// The stat line as the game words it: `+148.5% Critical Chance`, `x1.45 Damage to Corpus`.
pub(crate) fn stat_text(tag: &str, label: &str, value: f64) -> String {
    let number = if is_faction(tag) {
        format!("x{:.2}", 1.0 + value)
    } else {
        let shown = if label.contains("|val|%") { value * 100.0 } else { value };
        let rounded = (shown * 10.0).round() / 10.0;
        let sign = if rounded > 0.0 { "+" } else if rounded < 0.0 { "-" } else { "" };
        let abs = rounded.abs();
        if abs.fract() == 0.0 { format!("{sign}{abs:.0}") } else { format!("{sign}{abs:.1}") }
    };
    label.replace("|val|", &number)
}

/// One stat on a card. `good` is false for the negative (curse).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StatView {
    pub tag: String,
    pub text: String,
    pub good: bool,
    /// How good the roll is, 0.0 (worst) to 1.0 (best). For the negative, smaller is better.
    pub quality: f64,
    /// Letter for the roll: S, A+, A, A-, B+, B, B-, C+, C, C-, F.
    pub letter: String,
    /// The stat at its worst and best roll on this riven (max rank).
    pub worst: String,
    pub best: String,
    /// Positives: wanted by the grading sheet. Negative: on the weapon's harmless list.
    /// `None` when the weapon isn't in the sheet.
    pub wanted: Option<bool>,
    /// A spliced (or unrecognised) trait whose range isn't known yet: shown, not graded
    /// ("collecting data").
    pub pending: bool,
    /// A stat this riven type can't roll any more (see [`legacy_stat`]): shown, not graded
    /// ("no longer rolls"). Always `pending` too.
    pub legacy: bool,
    /// Its number couldn't be read off the card ("not read"; `worst`/`best` give the range it's
    /// in). Always `pending` too.
    pub unread: bool,
}

/// Roll letter, as in warframe-riven-info's `floatToGrade`: the roll mapped onto −10…+10.
pub fn letter(quality: f64) -> &'static str {
    let v = -10.0 + 20.0 * quality;
    match v {
        v if v >= 9.5 => "S",
        v if v >= 7.5 => "A+",
        v if v >= 5.5 => "A",
        v if v >= 3.5 => "A-",
        v if v >= 1.5 => "B+",
        v if v >= -1.5 => "B",
        v if v >= -3.5 => "B-",
        v if v >= -5.5 => "C+",
        v if v >= -7.5 => "C",
        v if v >= -9.5 => "C-",
        _ => "F",
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RivenView {
    pub id: String,
    #[serde(skip)]
    pub item_type: String,
    /// Riven art (image-cache name, from WFCD), filled in by the command.
    pub image: Option<String>,
    /// Rifle, Shotgun, Pistol, Melee, Zaw, Kitgun, Archgun, Companion Weapon.
    pub kind: String,
    /// Weapon game path (`compat`), used to look the riven up on warframe.market.
    pub compat: String,
    pub weapon: String,
    /// Generated name, e.g. "Crita-satican".
    pub name: String,
    pub rank: u32,
    pub mastery_req: u32,
    pub rerolls: u32,
    /// Madurai, Vazarin or Naramon.
    pub polarity: String,
    pub disposition: f64,
    /// 1–5, as shown in game.
    pub dots: u8,
    pub stats: Vec<StatView>,
    /// Great, Good, OK or Bad from the grading sheet; `None` if the weapon isn't listed.
    pub grade: Option<String>,
    /// Average roll quality of all stats, 0–100.
    pub perfectness: f64,
    /// The weapon's best stats from the grading sheet, marked with what this riven has.
    pub best: Option<BestStats>,
    /// Has a spliced trait: the sheet's grades don't cover those yet, so there's no grade and it
    /// shows "collecting data".
    pub grade_pending: bool,
}

/// A riven whose unveiling challenge is in progress.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Unveiling {
    pub kind: String,
    pub challenge: String,
    pub progress: u32,
    pub required: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VeiledCount {
    pub kind: String,
    pub count: i64,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RivensPage {
    pub rivens: Vec<RivenView>,
    pub unveiling: Vec<Unveiling>,
    pub veiled: Vec<VeiledCount>,
}

/// "Rifle Riven Mod" → "Rifle".
fn kind_label(label: &str) -> String {
    label.trim_end_matches(" Riven Mod").to_string()
}

fn polarity(pol: &str) -> &'static str {
    match pol {
        "AP_ATTACK" => "Madurai",
        "AP_DEFENSE" => "Vazarin",
        "AP_TACTIC" => "Naramon",
        _ => "",
    }
}

/// Disposition dots: 1 below 0.7, then 0.9, 1.1, 1.3, else 5 (the wiki's ranges).
pub fn dots(disposition: f64) -> u8 {
    match disposition {
        d if d < 0.7 => 1,
        d if d < 0.9 => 2,
        d if d <= 1.1 => 3,
        d if d <= 1.3 => 4,
        _ => 5,
    }
}

/// "RandomizedStyleKill" → "Style Kill".
fn challenge_name(path: &str) -> String {
    let last = path.rsplit('/').next().unwrap_or_default().trim_start_matches("Randomized");
    let mut out = String::new();
    for (i, c) in last.chars().enumerate() {
        if i > 0 && c.is_uppercase() {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// The generated riven name: buffs sorted by roll (ties by base value), first as a capitalised
/// prefix, middle ones as `-prefix`, the last as a suffix.
fn riven_name(kind: &RivenType, buffs: &[Roll]) -> String {
    let mut best: HashMap<&str, u32> = HashMap::new();
    for b in buffs {
        let e = best.entry(&b.tag).or_insert(0);
        *e = (*e).max(b.value);
    }
    let base = |t: &str| kind.stats.get(t).map_or(0.0, |s| s.value);
    let mut sorted: Vec<&Roll> = buffs.iter().collect();
    sorted.sort_by(|a, b| {
        best[b.tag.as_str()]
            .cmp(&best[a.tag.as_str()])
            .then(base(&a.tag).partial_cmp(&base(&b.tag)).unwrap_or(std::cmp::Ordering::Equal))
    });
    let (Some(first), Some(last)) = (sorted.first(), sorted.last()) else { return String::new() };
    let mut name = String::new();
    for b in &sorted {
        let Some(s) = kind.stats.get(&b.tag) else { continue };
        if b.tag == last.tag {
            name.push_str(&s.suffix);
        } else if b.tag == first.tag {
            let mut c = s.prefix.chars();
            if let Some(f) = c.next() {
                name.extend(f.to_uppercase());
                name.push_str(c.as_str());
            }
        } else {
            name.push('-');
            name.push_str(&s.prefix);
        }
    }
    name
}

/// Stat values at `rank` (RivenParser.js `parseRiven`), with each roll's quality and range.
fn stats(data: &RivenData, kind: &RivenType, fp: &Fingerprint, disposition: f64, rank: u32) -> Vec<StatView> {
    let atten = 1.5 * disposition * 10.0; // specific-fit attenuation × disposition × riven base drain
    let level = f64::from(rank + 1);
    let curse_atten = 1.25f64.powi(fp.curses.len() as i32);
    let lerp = |f: f64| 0.9 + 0.2 * f;
    let mut out = Vec::new();
    let make = |tag: &str, s: &StatInfo, value_at: &dyn Fn(f64) -> f64, roll: u32, good: bool| {
        let f = roll_fraction(roll);
        // A bigger negative is worse, so its quality runs the other way.
        let quality = if good { f } else { 1.0 - f };
        let (worst, best) = if good { (0.0, 1.0) } else { (1.0, 0.0) };
        StatView {
            tag: tag.to_string(),
            text: stat_text(tag, &s.label, value_at(f)),
            good,
            quality,
            letter: letter(quality).to_string(),
            worst: stat_text(tag, &s.label, value_at(worst)),
            best: stat_text(tag, &s.label, value_at(best)),
            wanted: None,
            pending: false,
            legacy: false,
            unread: false,
        }
    };
    // A trait without numbers (a spliced one the data doesn't have): shown by name, ungraded
    // ("collecting data"). It still counts towards the buff and curse totals above. Pending tags
    // from a card are put back by the overlay with the card's own numbers.
    // A stat the type can't roll any more: shown by name, ungraded ("no longer rolls"); its
    // numbers for this type aren't known.
    let unknown = |out: &mut Vec<StatView>, tag: &str, good: bool| {
        if !tag.starts_with(PENDING_PREFIX) && !tag.starts_with(LEGACY_PREFIX) && !tag.starts_with(UNREAD_PREFIX) {
            let legacy = legacy_stat(data, kind, tag);
            out.push(StatView {
                tag: tag.to_string(),
                text: legacy.as_ref().map_or_else(|| trait_name(tag), |s| label_name(&s.label)),
                good,
                quality: 0.0,
                letter: "?".into(),
                worst: String::new(),
                best: String::new(),
                wanted: None,
                pending: true,
                legacy: legacy.is_some(),
                unread: false,
            });
        }
    };
    for b in &fp.buffs {
        let Some(s) = stat_info(kind, &b.tag) else {
            unknown(&mut out, &b.tag, true);
            continue;
        };
        let s = &s;
        let n = at(&BUFF_COUNT_ATTEN, fp.buffs.len());
        out.push(make(&b.tag, s, &|f| s.value * atten * curse_atten * lerp(f) * n * level, b.value, true));
    }
    for c in &fp.curses {
        let Some(s) = stat_info(kind, &c.tag) else {
            unknown(&mut out, &c.tag, false);
            continue;
        };
        let s = &s;
        let n = at(&CURSE_BUFF_ATTEN, fp.buffs.len()) * at(&BUFF_COUNT_ATTEN, fp.curses.len());
        let base = curse_base(s);
        out.push(make(&c.tag, s, &|f| -base * atten * lerp(f) * n * level, c.value, false));
    }
    out
}

/// Grades a riven with the sheet and marks each stat as wanted / harmless.
pub(crate) fn apply_grade(r: &mut RivenView, sheet: &GradeSheet) {
    let Some(rule) = rule_for(sheet, &r.kind, &r.weapon) else { return };
    let buffs: Vec<&str> = r.stats.iter().filter(|s| s.good).map(|s| s.tag.as_str()).collect();
    let curses: Vec<&str> = r.stats.iter().filter(|s| !s.good).map(|s| s.tag.as_str()).collect();
    let g = grade(rule, &buffs, &curses);
    r.best = Some(best_stats(rule, &buffs, &curses));
    let mut wanted = g.wanted.into_iter();
    for s in &mut r.stats {
        s.wanted = Some(if s.good { wanted.next().unwrap_or(false) } else { g.harmless_negative });
    }
    r.grade = Some(g.grade.to_string());
}

/// One unveiled riven with its stats shown at `show_rank` (the Rivens page uses max rank; the
/// overlay uses the riven's own rank so the numbers match the card). `None` for unveiling rivens
/// and unknown types.
pub fn view(data: &RivenData, sheet: &GradeSheet, r: &OwnedRiven, show_rank: u32) -> Option<RivenView> {
    view_with(data, sheet, r, show_rank, None)
}

/// The riven's rank (`lvl`, missing = 0).
pub fn own_rank(r: &OwnedRiven) -> u32 {
    serde_json::from_str::<Fingerprint>(&r.fingerprint).ok().and_then(|f| f.lvl).unwrap_or(0)
}

/// Like [`view`], with the stats worked out at another disposition. The game shows a riven's
/// numbers for the weapon it's being viewed with (e.g. Burston Prime 1.35 vs Burston 1.45).
pub fn view_with(
    data: &RivenData,
    sheet: &GradeSheet,
    r: &OwnedRiven,
    show_rank: u32,
    disposition: Option<f64>,
) -> Option<RivenView> {
    let short = r.item_type.rsplit('/').next().unwrap_or_default();
    let fp: Fingerprint = serde_json::from_str(&r.fingerprint).ok()?;
    if fp.challenge.is_some() {
        return None;
    }
    let kind = data.types.get(short)?;
    let compat = fp.compat.clone().unwrap_or_default();
    let weapon = data.weapons.get(&compat);
    let disposition = disposition.unwrap_or_else(|| weapon.map_or(DISPOSITION, |w| w.disposition));
    let stats = stats(data, kind, &fp, disposition, show_rank);
    let graded: Vec<f64> = stats.iter().filter(|s| !s.pending).map(|s| s.quality).collect();
    let perfectness = if graded.is_empty() { 0.0 } else { 100.0 * graded.iter().sum::<f64>() / graded.len() as f64 };
    let spliced = fp.buffs.iter().chain(&fp.curses).any(|r| is_spliced(data, kind, &r.tag));
    let mut view = RivenView {
        id: r.id.clone(),
        item_type: r.item_type.clone(),
        image: None,
        kind: kind_label(&kind.label),
        compat: compat.clone(),
        weapon: weapon.map_or_else(|| compat.rsplit('/').next().unwrap_or("Unknown").to_string(), |w| w.name.clone()),
        name: riven_name(kind, &fp.buffs),
        rank: fp.lvl.unwrap_or(0),
        mastery_req: fp.lvl_req.unwrap_or(0),
        rerolls: fp.rerolls.unwrap_or(0),
        polarity: polarity(fp.pol.as_deref().unwrap_or_default()).to_string(),
        disposition,
        dots: dots(disposition),
        stats,
        grade: None,
        perfectness,
        best: None,
        grade_pending: false,
    };
    apply_grade(&mut view, sheet);
    if spliced {
        view.grade = None;
        view.grade_pending = true;
    }
    Some(view)
}

/// Owned rivens from the store (falling back to the newest raw snapshot for captures from
/// before rivens were stored), for the latest account.
pub fn owned_rivens(shared: &Shared) -> Result<(Vec<OwnedRiven>, Flat), String> {
    let guard = shared.store.lock().unwrap();
    let Some(store) = guard.as_ref() else { return Ok((Vec::new(), Flat::new())) };
    let Some(account) = store.latest_account().map_err(|e| e.to_string())? else {
        return Ok((Vec::new(), Flat::new()));
    };
    let current = store.current(&account).map_err(|e| e.to_string())?;
    let mut owned = store.rivens(&account).map_err(|e| e.to_string())?;
    if owned.is_empty() {
        if let Some(raw) = store.latest_raw(&account)? {
            owned = parse(&raw).map(|inv| inv.rivens).unwrap_or_default();
        }
    }
    Ok((owned, current))
}

pub fn build(data: &RivenData, sheet: &GradeSheet, owned: &[OwnedRiven], current: &Flat) -> RivensPage {
    let mut page = RivensPage::default();
    for r in owned {
        let short = r.item_type.rsplit('/').next().unwrap_or_default();
        let fp: Fingerprint = serde_json::from_str(&r.fingerprint).unwrap_or_default();
        let kind_name = data
            .types
            .get(short)
            .map(|t| kind_label(&t.label))
            .or_else(|| data.veiled.get(&r.item_type).map(|l| kind_label(l)))
            .unwrap_or_else(|| short.to_string());

        if let Some(ch) = &fp.challenge {
            page.unveiling.push(Unveiling {
                kind: kind_name,
                challenge: challenge_name(&ch.kind),
                progress: ch.progress,
                required: ch.required,
            });
            continue;
        }
        if let Some(view) = view(data, sheet, r, MAX_RANK) {
            page.rivens.push(view);
        }
    }
    page.rivens.sort_by(|a, b| a.weapon.cmp(&b.weapon).then(a.name.cmp(&b.name)));

    let mut veiled: Vec<VeiledCount> = current
        .iter()
        .filter(|((section, item), n)| section == "RawUpgrades" && item.contains("/Randomized/") && **n > 0)
        .map(|((_, item), &count)| VeiledCount {
            kind: data.veiled.get(item).map_or_else(|| item.rsplit('/').next().unwrap_or_default().to_string(), |l| kind_label(l)),
            count,
        })
        .collect();
    veiled.sort_by(|a, b| a.kind.cmp(&b.kind));
    page.veiled = veiled;
    page
}

#[tauri::command]
pub fn get_rivens(shared: State<'_, Arc<Shared>>) -> Result<RivensPage, String> {
    let index = shared.items.read().unwrap().clone();
    let sheet = shared.riven_sheet.read().unwrap().clone();
    let (owned, current) = owned_rivens(&shared)?;
    let mut page = build(&data(), &sheet, &owned, &current);
    for r in &mut page.rivens {
        r.image = index.get(&r.item_type).and_then(|i| i.image.clone());
    }
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(fp: &str) -> OwnedRiven {
        OwnedRiven {
            id: "1".into(),
            item_type: "/Lotus/Upgrades/Mods/Randomized/LotusRifleRandomModRare".into(),
            fingerprint: fp.into(),
        }
    }

    #[test]
    fn bundled_data_loads() {
        let d = data();
        assert!(d.types.contains_key("LotusRifleRandomModRare"));
        assert!(d.weapons.len() > 300);
        assert!(!d.built.is_empty());
    }

    #[test]
    fn weapons_know_their_riven_type_and_variants() {
        let d = data();
        assert_eq!(d.kind_of("/Lotus/Weapons/Infested/Pistols/InfVomitGun/InfVomitGunWep"), Some("LotusPistolRandomModRare"));
        assert_eq!(d.kind_of("/Lotus/Weapons/Tenno/Shotgun/ShotgunVandal"), Some("LotusShotgunRandomModRare"));
        let latron = "/Lotus/Weapons/Tenno/Rifle/SemiAutoRifle";
        let names: Vec<&str> = d.family(latron).iter().map(|(_, w)| w.name.as_str()).collect();
        assert!(names.contains(&"Latron Prime") && names.contains(&"Latron Wraith"), "{names:?}");
        assert!(d.variants_differ(latron));
        assert!(!d.variants_differ("/Lotus/Weapons/Infested/Pistols/InfVomitGun/InfVomitGunWep"));
    }

    #[test]
    fn every_weapon_has_the_same_disposition() {
        let mut d = bundled();
        d.flatten_dispositions();
        assert!(d.weapons.values().all(|w| w.disposition == DISPOSITION));
        assert!(!d.variants_differ("/Lotus/Weapons/Tenno/Rifle/SemiAutoRifle"), "Latron and Latron Prime alike");
        assert_eq!(dots(DISPOSITION), 4);
    }

    #[test]
    fn rifle_riven_stats_and_name() {
        // Disposition-1 test weapon: two buffs at mid roll, one curse at mid roll, max rank.
        let mut d = RivenData { built: String::new(), types: HashMap::new(), veiled: HashMap::new(), weapons: HashMap::new() };
        d.types.insert("LotusRifleRandomModRare".into(), RivenType {
            label: "Rifle Riven Mod".into(),
            stats: [
                ("WeaponCritChanceMod", 0.016666, "crita", "cron", "|val|% Critical Chance"),
                ("WeaponFireIterationsMod", 0.01, "sati", "can", "|val|% Multishot"),
                ("WeaponRecoilReductionMod", -0.01, "zeti", "mag", "|val|% Weapon Recoil"),
                ("WeaponFactionDamageCorpus", 0.005, "manti", "tron", "|val| Damage to Corpus"),
            ]
            .into_iter()
            .map(|(t, v, p, s, l)| (t.to_string(), StatInfo { value: v, prefix: p.into(), suffix: s.into(), label: l.into() }))
            .collect(),
        });
        d.weapons.insert("/W".into(), Weapon { name: "Test Rifle".into(), disposition: 1.0, kind: None, base: None });
        let mid = 0x3FFF_FFFFu32 / 2;
        let fp = format!(
            r#"{{"compat":"/W","lvl":8,"lvlReq":10,"rerolls":3,"pol":"AP_ATTACK",
                "buffs":[{{"Tag":"WeaponFireIterationsMod","Value":{}}},{{"Tag":"WeaponCritChanceMod","Value":{mid}}}],
                "curses":[{{"Tag":"WeaponRecoilReductionMod","Value":{mid}}}]}}"#,
            mid + 1000
        );
        let page = build(&d, &GradeSheet::default(), &[owned(&fp)], &Flat::new());
        let r = &page.rivens[0];
        assert_eq!((r.weapon.as_str(), r.kind.as_str(), r.polarity.as_str(), r.dots), ("Test Rifle", "Rifle", "Madurai", 3));
        assert_eq!(r.name, "Saticron", "higher roll first as the prefix, the other as the suffix");
        // 0.016666 × 15 × 1.25 (one curse) × 1.0 (mid roll) × 0.66 (two buffs) × 9 = 185.6%
        assert_eq!(r.stats[1].text, "+185.6% Critical Chance");
        assert!(r.stats[1].good);
        // Curse: −(−0.01) × 15 × 1.0 × 0.33000001 × 1.0 × 9 = +44.55…% recoil (bad)
        assert_eq!(r.stats[2].text, "+44.6% Weapon Recoil");
        assert!(!r.stats[2].good);
    }

    #[test]
    fn archgun_rivens_keep_damage_to_infested() {
        // An older archgun riven showed "collecting data": archgun rivens no longer roll Damage
        // to Infested, so the export doesn't list it. (Made-up rolls.)
        let d = data();
        let r = OwnedRiven {
            id: "a".into(),
            item_type: "/Lotus/Upgrades/Mods/Randomized/LotusArchgunRandomModRare".into(),
            fingerprint: r#"{"compat":"/Lotus/Weapons/Tenno/Archwing/Primary/ArchBurstGun/ArchBurstGun","buffs":[{"Tag":"WeaponFireRateMod","Value":500000000},{"Tag":"WeaponFactionDamageInfested","Value":600000000}],"curses":[]}"#.into(),
        };
        let v = view(&d, &GradeSheet::default(), &r, MAX_RANK).unwrap();
        assert!(!v.grade_pending);
        let infested = v.stats.iter().find(|s| s.tag == "WeaponFactionDamageInfested").unwrap();
        assert!(!infested.pending && infested.text.starts_with('x') && infested.text.ends_with("Damage to Infested"), "{}", infested.text);
        assert!(v.name.ends_with("ada") || v.name.to_lowercase().starts_with("pura"), "{}", v.name);
        // Types that already list it are unchanged; melee keeps its own tags.
        let melee = &d.types["PlayerMeleeWeaponRandomModRare"];
        assert!(!melee.stats.contains_key("WeaponFactionDamageInfested"));
    }

    #[test]
    fn owned_rivens_keep_removed_stats() {
        // An older shotgun riven with negative Zoom, which shotguns can't roll any more.
        // (Made-up rolls.)
        let d = data();
        let r = OwnedRiven {
            id: "s".into(),
            item_type: "/Lotus/Upgrades/Mods/Randomized/LotusShotgunRandomModRare".into(),
            fingerprint: r#"{"compat":"/Lotus/Weapons/Tenno/Shotgun/Shotgun","lvl":8,"buffs":[{"Tag":"WeaponCritChanceMod","Value":500000000},{"Tag":"WeaponCritDamageMod","Value":500000000}],"curses":[{"Tag":"WeaponZoomFovMod","Value":500000000}]}"#.into(),
        };
        let v = view(&d, &GradeSheet::default(), &r, MAX_RANK).unwrap();
        let zoom = v.stats.iter().find(|s| s.tag == "WeaponZoomFovMod").unwrap();
        assert_eq!((zoom.text.as_str(), zoom.legacy, zoom.pending), ("Zoom", true, true));
        assert!(!v.grade_pending, "graded from its other stats");
        assert!(v.stats.iter().filter(|s| !s.legacy).all(|s| s.letter != "?"));
    }

    #[test]
    fn spliced_traits_on_owned_rivens() {
        // Example owned rivens: Torid with Status Damage, Latron with Viral.
        let d = data();
        let torid = OwnedRiven {
            id: "t".into(),
            item_type: "/Lotus/Upgrades/Mods/Randomized/LotusRifleRandomModRare".into(),
            fingerprint: r#"{"compat":"/Lotus/Weapons/ClanTech/Bio/BioWeapon","lvlReq":13,"lvl":8,"rerolls":227,"pol":"AP_DEFENSE","buffs":[{"Tag":"WeaponCritDamageMod","Value":209117350},{"Tag":"WeaponStatusDamage","Value":1048830309},{"Tag":"WeaponFireRateMod","Value":41943244}],"curses":[{"Tag":"WeaponZoomFovMod","Value":478554922}]}"#.into(),
        };
        let v = view(&d, &GradeSheet::default(), &torid, MAX_RANK).unwrap();
        assert_eq!(v.stats.len(), 4, "the spliced trait is shown, not dropped");
        let status = v.stats.iter().find(|s| s.tag == "WeaponStatusDamage").unwrap();
        assert_eq!((status.text.as_str(), status.pending, status.letter.as_str()), ("Status Damage", true, "?"));
        assert!(v.grade_pending && v.grade.is_none());
        let latron = OwnedRiven {
            id: "l".into(),
            item_type: "/Lotus/Upgrades/Mods/Randomized/LotusRifleRandomModRare".into(),
            fingerprint: r#"{"compat":"/Lotus/Weapons/Tenno/Rifle/SemiAutoRifle","lvlReq":9,"rerolls":2,"pol":"AP_TACTIC","buffs":[{"Tag":"WeaponCritChanceMod","Value":933856123},{"Tag":"WeaponViralDamageMod","Value":1024886253},{"Tag":"WeaponFireRateMod","Value":345049011}],"curses":[{"Tag":"WeaponArmorPiercingDamageMod","Value":1024719885}],"lvl":8}"#.into(),
        };
        // Shown with Latron Prime (1.3), as on the card: "+119.7% Viral".
        let v = view_with(&d, &GradeSheet::default(), &latron, 8, Some(1.3)).unwrap();
        let viral = v.stats.iter().find(|s| s.tag == "WeaponViralDamageMod").unwrap();
        assert_eq!((viral.text.as_str(), viral.pending), ("+119.7% Viral", false));
        assert!(v.grade_pending);
        assert_eq!(trait_name("WeaponFactionDamageInfested"), "Damage to Infested");
    }

    #[test]
    fn stat_text_formats() {
        assert_eq!(stat_text("WeaponFactionDamageCorpus", "|val| Damage to Corpus", 0.4512), "x1.45 Damage to Corpus");
        assert_eq!(stat_text("WeaponFactionDamageCorpus", "|val| Damage to Corpus", -0.3), "x0.70 Damage to Corpus");
        assert_eq!(stat_text("WeaponPunctureDepthMod", "|val| Punch Through", 2.71), "+2.7 Punch Through");
        assert_eq!(stat_text("ComboDurationMod", "|val|s Combo Duration", 8.0), "+8s Combo Duration");
    }

    #[test]
    fn unveiling_and_veiled_counts() {
        let d = data();
        let fp = r#"{"challenge":{"Type":"/Lotus/Types/Challenges/RandomizedStyleKill","Progress":3,"Required":48}}"#;
        let mut flat = Flat::new();
        flat.insert(("RawUpgrades".into(), "/Lotus/Upgrades/Mods/Randomized/RawRifleRandomMod".into()), 8);
        let page = build(&d, &GradeSheet::default(), &[owned(fp)], &flat);
        assert!(page.rivens.is_empty());
        assert_eq!(page.unveiling[0].challenge, "Style Kill");
        assert_eq!((page.unveiling[0].progress, page.unveiling[0].required), (3, 48));
        assert_eq!(page.veiled, [VeiledCount { kind: "Rifle".into(), count: 8 }]);
    }
}

#[cfg(test)]
mod live_check {
    /// Run with `INV_FILE=<inventory json> SHEET_DIR=<folder> cargo test live_rivens -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_rivens() {
        let raw = std::fs::read(std::env::var("INV_FILE").unwrap()).unwrap();
        let inv = crate::translate::parse(&raw).unwrap();
        // SHEET_DIR: a folder with the sheet's tabs as <tab>.csv (downloaded if missing or old).
        let sheet = crate::riven_grades::load_or_refresh(std::path::Path::new(&std::env::var("SHEET_DIR").unwrap()));
        let page = super::build(&super::data(), &sheet, &inv.rivens, &inv.values);
        for r in &page.rivens {
            let stats: Vec<String> = r
                .stats
                .iter()
                .map(|s| {
                    let mark = match s.wanted { Some(true) => "*", Some(false) => "", None => "?" };
                    format!("{}{}{} [{}]", if s.good { "" } else { "[-]" }, mark, s.text, s.letter)
                })
                .collect();
            println!(
                "{:6} {:3.0}% {} {} | {}",
                r.grade.as_deref().unwrap_or("-"),
                r.perfectness,
                r.weapon,
                r.name,
                stats.join(" ; ")
            );
        }
        println!("unveiling: {:?}\nveiled: {:?}", page.unveiling, page.veiled);
    }
}
