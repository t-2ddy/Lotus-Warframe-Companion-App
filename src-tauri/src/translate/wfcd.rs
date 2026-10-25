//! Item names from WFCD warframe-items: maps internal paths (`/Lotus/...`) to readable items.
//!
//! Data: https://github.com/WFCD/warframe-items (data/json/<Category>.json). There is no single
//! combined file, so each category is downloaded and cached in the app data folder.
//!
//! WFCD sometimes ships relics without their rewards (every relic, on 2026-10-23). Those are
//! filled from WFCD's drop data (drops.warframestat.us, made from DE's drop tables), or from the
//! snapshot `data/relic_rewards.json` (made by `scripts/gen_relic_rewards.py`) that ships with the app.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

const BASE_URL: &str = "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/";

/// Category files to download. Enemy and Node aren't items a player owns, so they're skipped.
pub const CATEGORIES: [&str; 24] = [
    "Arcanes", "Arch-Gun", "Arch-Melee", "Archwing", "Components", "Fish", "Gear", "Glyphs",
    "Honoria", "Melee", "Misc", "Mods", "Pets", "Primary", "Quests", "Railjack", "Relics",
    "Resources", "Secondary", "SentinelWeapons", "Sentinels", "Sigils", "Skins", "Warframes",
];

/// Re-download the data when the cached copy is older than this.
pub const MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

const DROPS_URL: &str = "https://drops.warframestat.us/data/relics.json";
/// The `files` entry (and cache file name) holding the drop data's relics.
pub const DROPS: &str = "DropRelics";
const BUNDLED_RELICS: &str = include_str!("../../data/relic_rewards.json");

/// What the app needs to know about one item.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ItemInfo {
    pub name: String,
    pub category: String,
    pub image: Option<String>,
    pub ducats: Option<u32>,
    pub tradable: bool,
    /// The WFCD file the item came from ("Mods", "Relics", "Resources", …).
    pub file: String,
    /// WFCD `type`, e.g. "Warframe Mod", "Secondary Arcane", "Gem".
    pub kind: Option<String>,
    pub rarity: Option<String>,
    pub polarity: Option<String>,
    pub drain: Option<i32>,
    /// Highest rank for mods and arcanes.
    pub max_rank: Option<u32>,
    /// What a mod fits, e.g. "Trinity" for an augment.
    pub compat: Option<String>,
    pub vaulted: bool,
    /// Full card image on the Warframe wiki (mods), shown at max rank.
    pub wiki_image: Option<String>,
    /// Drawn under `image`: WFCD's Tauforged Archon Shard images are only the glow, so the
    /// normal shard goes underneath.
    pub underlay: Option<String>,
    /// Stat lines per rank (mods and arcanes), index = rank.
    #[serde(skip)]
    pub level_stats: Vec<Vec<String>>,
}

/// One possible reward from a relic.
#[derive(Debug, Clone, PartialEq)]
pub struct RelicReward {
    pub path: String,
    pub name: String,
    /// Drop chance (%) for the Intact refinement, used to rank rarity.
    pub chance: f64,
}

/// The Foundry tab an item is listed under.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Tab {
    Warframe,
    Primary,
    Secondary,
    Melee,
    Modular,
    Arch,
    Companion,
}

/// How affinity (XP) turns into rank. Warframe-like items need twice the XP per rank of weapons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XpCurve {
    /// Warframes, Archwings, Necramechs, companions, K-Drives: rank r needs 1000·r² XP.
    Frame,
    /// Weapons, including arch-guns, sentinel weapons, kitguns, Zaws and amps: 500·r² XP.
    Weapon,
}

impl XpCurve {
    pub fn xp_for_rank(self, rank: u32) -> i64 {
        let per = match self {
            XpCurve::Frame => 1000,
            XpCurve::Weapon => 500,
        };
        per * i64::from(rank) * i64::from(rank)
    }

    pub fn rank_for_xp(self, xp: i64, max_rank: u32) -> u32 {
        (0..=max_rank).rev().find(|&r| xp >= self.xp_for_rank(r)).unwrap_or(0)
    }

    /// Account mastery points per item rank: 200 for Warframe-like items, 100 for weapons.
    pub fn mastery_per_rank(self) -> i64 {
        match self {
            XpCurve::Frame => 200,
            XpCurve::Weapon => 100,
        }
    }
}

/// One build requirement: an item path and how many are needed.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub path: String,
    pub count: u32,
}

/// An item that gives mastery, with what the Foundry page needs to know about it.
#[derive(Debug, Clone)]
pub struct Masterable {
    pub path: String,
    pub tab: Tab,
    pub curve: XpCurve,
    pub max_rank: u32,
    pub mastery_req: u32,
    pub is_prime: bool,
    pub vaulted: bool,
    pub parts: Vec<Part>,
}

/// The parts of a WFCD entry we read. Every field is optional so one odd entry can't break a file.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawItem {
    #[serde(default)]
    unique_name: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    image_name: Option<String>,
    #[serde(default)]
    ducats: Option<f64>,
    #[serde(default)]
    tradable: Option<bool>,
    #[serde(default)]
    masterable: Option<bool>,
    #[serde(default)]
    mastery_req: Option<f64>,
    #[serde(default)]
    max_level_cap: Option<f64>,
    #[serde(default)]
    is_prime: Option<bool>,
    #[serde(default)]
    vaulted: Option<bool>,
    #[serde(default)]
    rarity: Option<String>,
    #[serde(default)]
    polarity: Option<String>,
    #[serde(default)]
    base_drain: Option<f64>,
    #[serde(default)]
    fusion_limit: Option<f64>,
    #[serde(default)]
    compat_name: Option<String>,
    /// One entry per rank (arcanes have no `fusionLimit`; their max rank is this length − 1).
    #[serde(default)]
    level_stats: Vec<RawLevel>,
    #[serde(default)]
    wikia_thumbnail: Option<String>,
    #[serde(default)]
    rewards: Vec<RawReward>,
    #[serde(default)]
    components: Vec<RawComponent>,
    #[serde(default)]
    drops: Vec<RawDrop>,
    #[serde(default)]
    build_price: Option<f64>,
    #[serde(default)]
    build_time: Option<f64>,
    #[serde(default)]
    build_quantity: Option<f64>,
    #[serde(default)]
    market_cost: Option<f64>,
    #[serde(default)]
    bp_cost: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct RawDrop {
    #[serde(default)]
    location: String,
    #[serde(default)]
    chance: Option<f64>,
}

/// One place an item drops, as WFCD lists it: "Europa/Naamah (Assassination)",
/// "Lith S8 Relic (Intact)", "Ostron (Hok), Neutral", "Captain Vor".
#[derive(Debug, Clone, PartialEq)]
pub struct Drop {
    pub location: String,
    /// Chance in percent.
    pub chance: f64,
}

/// Foundry cost of building an item, and what the Market asks.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Build {
    pub credits: i64,
    /// Seconds.
    pub time: i64,
    pub quantity: i64,
    /// Platinum for the built item in the Market.
    pub market_cost: Option<i64>,
    /// Credits for its blueprint in the Market.
    pub bp_cost: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct RawLevel {
    #[serde(default)]
    stats: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawReward {
    #[serde(default)]
    chance: Option<f64>,
    #[serde(default)]
    item: Option<RawRewardItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRewardItem {
    #[serde(default)]
    unique_name: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawComponent {
    #[serde(default)]
    unique_name: String,
    #[serde(default)]
    item_count: Option<f64>,
}

/// Drop data's relics.json: `{"relics":[{"tier":"Lith","relicName":"T14","state":"Intact","rewards":[…]}]}`.
#[derive(Debug, Deserialize)]
struct DropFile {
    #[serde(default)]
    relics: Vec<DropRelic>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DropRelic {
    #[serde(default)]
    tier: String,
    #[serde(default)]
    relic_name: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    rewards: Vec<DropReward>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DropReward {
    #[serde(default)]
    item_name: String,
    #[serde(default)]
    chance: Option<f64>,
}

/// Relic name ("Lith T14") → its Intact rewards as (item name, chance).
type RelicTables = HashMap<String, Vec<(String, f64)>>;

/// The bundled snapshot: relic tables, and item paths for the reward names.
#[derive(Debug, Deserialize)]
struct BundledRelics {
    paths: HashMap<String, String>,
    relics: RelicTables,
}

/// The Intact tables of a drop data relics.json (empty if it doesn't parse).
fn drop_tables(bytes: &[u8]) -> RelicTables {
    let Ok(file) = serde_json::from_slice::<DropFile>(bytes) else { return RelicTables::new() };
    file.relics
        .into_iter()
        .filter(|r| r.state == "Intact" && !r.relic_name.is_empty() && !r.rewards.is_empty())
        .map(|r| {
            let rewards = r.rewards.into_iter().map(|x| (x.item_name, x.chance.unwrap_or(0.0))).collect();
            (format!("{} {}", r.tier, r.relic_name), rewards)
        })
        .collect()
}

/// Lookup from internal path to item.
#[derive(Debug, Default)]
pub struct ItemIndex {
    items: HashMap<String, ItemInfo>,
    masterables: Vec<Masterable>,
    /// Every path that appears as a build part of some item ("used for crafting").
    used_in_crafting: HashSet<String>,
    /// For a build part: the WFCD file of the first item that uses it ("Warframes", "Primary", …).
    parent_file: HashMap<String, String>,
    /// Relic name without refinement ("Lith S8") → its rewards, from the Intact entry when present.
    relic_rewards: HashMap<String, Vec<RelicReward>>,
    /// Internal Warframe codenames from WFCD paths ("Paladin") → names ("Oberon").
    frame_codenames: HashMap<String, String>,
    /// Lowercase item name → path, preferring whole items over components with the same name.
    by_name: HashMap<String, String>,
    /// Where each part or resource drops (WFCD `drops`).
    drops: HashMap<String, Vec<Drop>>,
    /// Foundry and Market costs of items that are built.
    builds: HashMap<String, Build>,
}

/// The image every blueprint shares in WFCD.
pub const BLUEPRINT_IMAGE: &str = "blueprint.png";
/// Image names starting with this are full mod cards from the Warframe wiki (see `images.rs`).
pub const WIKI_PREFIX: &str = "wiki__";

/// Turns a wiki image URL into an image-cache name:
/// `https://wiki.warframe.com/images/SerrationMod.png?0b8ff` → `wiki__SerrationMod.png`.
/// The `?…` part only busts browser caches, so it's dropped. Returns `None` for anything else.
fn wiki_cache_name(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://wiki.warframe.com/images/")?;
    let file = rest.split('?').next()?;
    let file = crate::net::decode_path_segment(file);
    let ok = !file.is_empty() && !file.contains(['/', '\\']) && !file.starts_with('.') && !file.contains("..");
    ok.then(|| format!("{WIKI_PREFIX}{file}"))
}
/// WFCD's generic Warframe Systems images, used for every Systems part and blueprint.
pub const SYSTEMS_IMAGE: &str = "GenericWarframeSystem.png";
pub const PRIME_SYSTEMS_IMAGE: &str = "GenericWarframePrimeSystem.png";

/// Codenames used in blueprint paths that don't match any WFCD Warframe path. Equinox is built from
/// two aspects: Anima (Night) and Animus (Day), each with its own Neuroptics, Chassis and Systems.
/// `(codename, name of its parts' owner, name of the item the bare blueprint builds)`.
const CODENAME_ALIASES: [(&str, &str, &str); 2] = [
    ("Anima", "Equinox Night", "Equinox Night Aspect"),
    ("Animus", "Equinox Day", "Equinox Day Aspect"),
];

/// When several items list the same build part, the one it's named after: equipment first,
/// cosmetics and everything else after (some ephemera recipes also list Warframe Systems).
fn parent_priority(file: &str) -> u8 {
    match file {
        "Warframes" | "Primary" | "Secondary" | "Melee" | "Archwing" | "Arch-Gun" | "Arch-Melee" | "Pets"
        | "Sentinels" | "SentinelWeapons" => 0,
        _ => 1,
    }
}

/// Relic refinements, in order of how refined they are.
pub const REFINEMENTS: [&str; 4] = ["Intact", "Exceptional", "Flawless", "Radiant"];

/// Splits "Lith S8 Radiant" into ("Lith S8", "Radiant"). Names without a known refinement
/// (a few special relics) count as Intact.
pub fn split_relic_name(name: &str) -> (&str, &str) {
    match name.rsplit_once(' ') {
        Some((base, last)) if REFINEMENTS.contains(&last) => (base, last),
        _ => (name, "Intact"),
    }
}

/// Items that give mastery but aren't in WFCD at all: `(path, name, tab, curve)`.
/// The Railjack Plexus (`DefaultHarness`) ranks to 30 like a Warframe.
const EXTRA_MASTERABLES: [(&str, &str, Tab, XpCurve); 1] =
    [("/Lotus/Types/Game/CrewShip/RailJack/DefaultHarness", "Plexus", Tab::Arch, XpCurve::Frame)];

/// Entries WFCD flags as masterable that aren't separate items a player can obtain.
const NOT_OBTAINABLE: &[&str] = &[
    // "Orion & Sirius": the second half of the Sirius & Orion Warframe, not its own frame.
    "/Lotus/Powersuits/SiriusOrion/OrionSuit",
];

/// Decides whether an item gives mastery, and if so which tab and XP curve it uses.
///
/// Sorting goes by WFCD file rather than WFCD's `type`, which mislabels some melee weapons
/// as "Rifle". Amp prisms aren't flagged `masterable` in WFCD but do give mastery.
fn classify(file: &str, item: &RawItem) -> Option<(Tab, XpCurve)> {
    if NOT_OBTAINABLE.contains(&item.unique_name.as_str()) {
        return None;
    }
    let kind = item.kind.as_deref().unwrap_or("");
    // Only an amp's Prism gives mastery; Scaffolds and Braces are also typed "Amp" in WFCD but
    // never appear in XPInfo.
    let is_amp = file == "Misc" && kind == "Amp" && item.name.as_deref().is_some_and(|n| n.ends_with(" Prism"));
    // Khora's Venari (and Prime) are typed "Warframe" in Pets.json and not flagged masterable, but
    // they do give mastery: without them the account total came out 12,000 XP short.
    let is_venari = file == "Pets" && kind == "Warframe";
    if !item.masterable.unwrap_or(false) && !is_amp && !is_venari {
        return None;
    }
    use {Tab::*, XpCurve::*};
    Some(match file {
        "Warframes" if kind == "Necramech" => (Arch, Frame),
        "Warframes" => (Warframe, Frame),
        "Primary" => (Primary, Weapon),
        "Secondary" => (Secondary, Weapon),
        "Melee" if kind == "Zaw Component" => (Modular, Weapon),
        "Melee" => (Melee, Weapon),
        "Arch-Gun" | "Arch-Melee" => (Arch, Weapon),
        "Archwing" => (Arch, Frame),
        "Pets" | "Sentinels" => (Companion, Frame),
        "SentinelWeapons" => (Companion, Weapon),
        "Misc" => match kind {
            "Kitgun Component" | "Amp" => (Modular, Weapon),
            "K-Drive Component" => (Modular, Frame),
            _ => return None,
        },
        _ => return None,
    })
}

impl ItemIndex {
    /// Builds the index from category files given as `(category, JSON bytes)`.
    ///
    /// Prime parts and blueprints are listed in Components.json with short names ("Barrel"),
    /// so their full name comes from the item that lists them as a component:
    /// "Braton Prime" + "Barrel" = "Braton Prime Barrel".
    pub fn build(files: &[(String, Vec<u8>)]) -> Self {
        let mut items = HashMap::new();
        let mut components: Vec<RawItem> = Vec::new();
        // Component path → (parent name, parent file); see `parent_priority`.
        let mut parent_of: HashMap<String, (String, String)> = HashMap::new();
        let mut masterables = Vec::new();
        let mut seen_masterable = HashSet::new();
        let mut used_in_crafting = HashSet::new();
        let mut relic_rewards: HashMap<String, Vec<RelicReward>> = HashMap::new();
        let mut frame_codenames: HashMap<String, String> = HashMap::new();
        // Every relic WFCD lists (name without refinement), with rewards or not.
        let mut relic_bases: HashSet<String> = HashSet::new();
        let mut drops: Option<RelicTables> = None;
        let mut item_drops: HashMap<String, Vec<Drop>> = HashMap::new();
        let mut builds: HashMap<String, Build> = HashMap::new();

        for (category, bytes) in files {
            if category == DROPS {
                drops = Some(drop_tables(bytes)).filter(|t| !t.is_empty());
                continue;
            }
            let Ok(raw) = serde_json::from_slice::<Vec<RawItem>>(bytes) else {
                eprintln!("[translate] couldn't parse WFCD {category}.json; skipping it");
                continue;
            };
            for item in raw {
                if item.unique_name.is_empty() {
                    continue;
                }
                keep_sources(&item, &mut item_drops, &mut builds);
                if category == "Components" {
                    components.push(item);
                    continue;
                }
                let Some(name) = item.name.as_deref().map(clean_name) else { continue };
                for c in &item.components {
                    let replace = parent_of
                        .get(&c.unique_name)
                        .is_none_or(|(_, file)| parent_priority(category) < parent_priority(file));
                    if replace {
                        parent_of.insert(c.unique_name.clone(), (name.clone(), category.clone()));
                    }
                    used_in_crafting.insert(c.unique_name.clone());
                }
                if category == "Warframes" {
                    if let Some(code) = item.unique_name.rsplit('/').next() {
                        frame_codenames.entry(code.to_string()).or_insert_with(|| name.clone());
                    }
                }
                if category == "Relics" {
                    relic_bases.insert(split_relic_name(&name).0.to_string());
                }
                if category == "Relics" && !item.rewards.is_empty() {
                    let (base, refinement) = split_relic_name(&name);
                    // Intact chances rank rarity reliably; other refinements are only a fallback.
                    if refinement == "Intact" || !relic_rewards.contains_key(base) {
                        let rewards = item
                            .rewards
                            .iter()
                            .filter_map(|r| {
                                let it = r.item.as_ref()?;
                                Some(RelicReward {
                                    path: it.unique_name.clone(),
                                    name: it.name.as_deref().map_or_else(|| fallback_name(&it.unique_name), clean_name),
                                    chance: r.chance.unwrap_or(0.0),
                                })
                            })
                            .collect();
                        relic_rewards.insert(base.to_string(), rewards);
                    }
                }
                if let Some((tab, curve)) = classify(category, &item) {
                    if seen_masterable.insert(item.unique_name.clone()) {
                        masterables.push(Masterable {
                            path: item.unique_name.clone(),
                            tab,
                            curve,
                            max_rank: item.max_level_cap.map_or(30, |c| c as u32),
                            mastery_req: item.mastery_req.map_or(0, |m| m as u32),
                            is_prime: item.is_prime.unwrap_or(false),
                            vaulted: item.vaulted.unwrap_or(false),
                            parts: merge_parts(&item.components),
                        });
                    }
                }
                items.entry(item.unique_name.clone()).or_insert_with(|| info(&item, name, category));
            }
        }

        // Parts last, so a resource that is also some item's component (Orokin Cell) keeps its own entry.
        for c in components {
            if items.contains_key(&c.unique_name) {
                continue;
            }
            let short = c.name.as_deref().map(clean_name).unwrap_or_default();
            // Parts and blueprints get their parent's name in front. A bare "Blueprint" always does,
            // even outside /Recipes/ (Cipher, Restorative and key blueprints live elsewhere).
            let name = match parent_of.get(&c.unique_name).map(|(n, _)| n) {
                Some(parent) if c.unique_name.contains("/Recipes/") || short == "Blueprint" => format!("{parent} {short}"),
                _ if !short.is_empty() => short,
                _ => fallback_name(&c.unique_name),
            };
            items.insert(c.unique_name.clone(), info(&c, name, "Components"));
        }

        // Only alongside real WFCD data, so a failed download doesn't leave a catalog of one item.
        let extras = if masterables.is_empty() { &[][..] } else { &EXTRA_MASTERABLES[..] };
        for &(path, name, tab, curve) in extras {
            if seen_masterable.insert(path.to_string()) {
                masterables.push(Masterable {
                    path: path.to_string(),
                    tab,
                    curve,
                    max_rank: 30,
                    mastery_req: 0,
                    is_prime: false,
                    vaulted: false,
                    parts: Vec::new(),
                });
            }
            items.entry(path.to_string()).or_insert_with(|| ItemInfo {
                name: name.to_string(),
                category: "Railjack".to_string(),
                image: None,
                ducats: None,
                tradable: false,
                file: "Extra".to_string(),
                kind: None,
                rarity: None,
                polarity: None,
                drain: None,
                max_rank: None,
                compat: None,
                vaulted: false,
                wiki_image: None,
                underlay: None,
                level_stats: Vec::new(),
            });
        }

        // Tauforged Archon Shards: WFCD's image is the glow alone (see `underlay`).
        let underlays: Vec<(String, String)> = items
            .iter()
            .filter(|(_, i)| i.image.as_deref().is_some_and(|img| img.contains("Glow")))
            .filter_map(|(path, _)| {
                let base = items.get(path.strip_suffix("Mythic")?)?;
                Some((path.clone(), base.image.clone()?))
            })
            .collect();
        for (path, image) in underlays {
            if let Some(i) = items.get_mut(&path) {
                i.underlay = Some(image);
            }
        }

        // When several items share a name (WFCD lists Forma twice, once without an image), prefer
        // whole items over components, then entries with an image; ties go to the smaller path
        // so the choice doesn't depend on hash order.
        let rank = |info: &ItemInfo| (info.file == "Components", info.image.is_none());
        let mut by_name: HashMap<String, String> = HashMap::new();
        for (path, info) in &items {
            let key = info.name.to_lowercase();
            let better = match by_name.get(&key).and_then(|p| items.get(p).map(|i| (p, i))) {
                None => true,
                Some((old_path, old)) => (rank(info), path) < (rank(old), old_path),
            };
            if better {
                by_name.insert(key, path.clone());
            }
        }

        // Relics WFCD lists without rewards: the drop data's table, else the bundled one. The
        // drop data names items only, so paths come from the snapshot, then from WFCD names.
        let bundled: BundledRelics = serde_json::from_str(BUNDLED_RELICS).expect("bundled relic_rewards.json is valid");
        let path_of = |name: &str| {
            // "2X Forma Blueprint" and "1200X Kuva" drop the plain item.
            let plain = name.split_once("X ").filter(|(n, _)| n.parse::<u32>().is_ok()).map_or(name, |(_, rest)| rest);
            bundled.paths.get(name).or_else(|| by_name.get(&plain.to_lowercase())).cloned().unwrap_or_default()
        };
        let mut filled = 0;
        for base in &relic_bases {
            if relic_rewards.contains_key(base) {
                continue;
            }
            // WFCD names Requiem relics "Requiem I Relic"; the drop data says "Requiem I".
            let key = base.strip_suffix(" Relic").unwrap_or(base);
            let Some(table) = drops.as_ref().and_then(|d| d.get(key)).or_else(|| bundled.relics.get(key)) else { continue };
            let rewards = table
                .iter()
                .map(|(name, chance)| RelicReward { path: path_of(name), name: clean_name(name), chance: *chance })
                .collect();
            relic_rewards.insert(base.clone(), rewards);
            filled += 1;
        }
        if filled > 0 {
            let source = if drops.is_some() { "drop data" } else { "bundled snapshot" };
            eprintln!("[translate] {filled} relics had no WFCD rewards; filled from the {source}");
        }

        let parent_file = parent_of.into_iter().map(|(path, (_, file))| (path, file)).collect();
        Self {
            items,
            masterables,
            used_in_crafting,
            parent_file,
            relic_rewards,
            frame_codenames,
            by_name,
            drops: item_drops,
            builds,
        }
    }

    /// For a blueprint, the image of what it builds, to draw over the generic blueprint picture.
    ///
    /// Found by name: "Braton Prime Blueprint" → Braton Prime. If the exact item has no image of
    /// its own, the last word is dropped (up to twice): "Oberon Chassis Blueprint" → Oberon Chassis
    /// → Oberon. Returns `None` for anything that isn't a blueprint.
    pub fn blueprint_overlay(&self, path: &str) -> Option<String> {
        let name = self.display_name(path);
        let mut base = name.strip_suffix(" Blueprint")?.to_string();
        // Warframe Systems always use WFCD's generic Systems images, prime or regular.
        if path.contains("/WarframeRecipes/") && base.ends_with(" Systems") {
            let image = if base.contains("Prime") { PRIME_SYSTEMS_IMAGE } else { SYSTEMS_IMAGE };
            return Some(image.to_string());
        }
        for _ in 0..3 {
            let image = self
                .by_name
                .get(&base.to_lowercase())
                .filter(|p| p.as_str() != path)
                .and_then(|p| self.items.get(p))
                .and_then(|i| i.image.clone())
                .filter(|img| !img.eq_ignore_ascii_case(BLUEPRINT_IMAGE));
            if image.is_some() {
                return image;
            }
            base = base.rsplit_once(' ')?.0.to_string();
        }
        None
    }

    /// The WFCD file of the item a build part belongs to ("Warframes", "Primary", …), if known.
    pub fn parent_file(&self, path: &str) -> Option<&str> {
        self.parent_file.get(path).map(String::as_str)
    }

    /// Rewards of a relic, by name without refinement ("Lith S8").
    pub fn relic_rewards(&self, base_name: &str) -> &[RelicReward] {
        self.relic_rewards.get(base_name).map_or(&[], Vec::as_slice)
    }

    /// Every relic (name without refinement) with its rewards.
    pub fn relics(&self) -> impl Iterator<Item = (&str, &[RelicReward])> {
        self.relic_rewards.iter().map(|(name, rewards)| (name.as_str(), rewards.as_slice()))
    }

    /// Names Warframe part blueprints WFCD doesn't list, which use the game's internal codenames:
    /// `/WarframeRecipes/PaladinChassisBlueprint` → "Oberon Chassis Blueprint" ("Helmet" = Neuroptics).
    fn frame_blueprint_name(&self, path: &str) -> Option<String> {
        if !path.contains("/WarframeRecipes/") {
            return None;
        }
        let stem = path.rsplit('/').next()?.strip_suffix("Blueprint")?;
        let alias = |code: &str| CODENAME_ALIASES.iter().find(|(c, _, _)| *c == code);
        for (suffix, part) in [("Helmet", "Neuroptics"), ("Chassis", "Chassis"), ("Systems", "Systems")] {
            if let Some(code) = stem.strip_suffix(suffix) {
                // Some paths use the frame's real name instead of its codename ("AshHelmetBlueprint").
                let frame = alias(code)
                    .map(|(_, owner, _)| owner.to_string())
                    .or_else(|| self.frame_codenames.get(code).cloned())
                    .unwrap_or_else(|| fallback_name(code));
                return Some(format!("{frame} {part} Blueprint"));
            }
        }
        if let Some((_, _, builds)) = alias(stem) {
            return Some(format!("{builds} Blueprint"));
        }
        self.frame_codenames.get(stem).map(|frame| format!("{frame} Blueprint"))
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Where a part or resource drops, as WFCD lists it.
    pub fn drops(&self, path: &str) -> &[Drop] {
        self.drops.get(path).map_or(&[], Vec::as_slice)
    }

    /// Foundry and Market costs of a built item.
    pub fn build_of(&self, path: &str) -> Option<&Build> {
        self.builds.get(path)
    }

    /// The path of an item by its name ("Orokin Cell"), any case.
    pub fn path_by_name(&self, name: &str) -> Option<&str> {
        self.by_name.get(&name.to_lowercase()).map(String::as_str)
    }

    /// Every item that gives mastery, in WFCD file order.
    pub fn masterables(&self) -> &[Masterable] {
        &self.masterables
    }

    /// True if the item is a build part of another item.
    pub fn used_in_crafting(&self, path: &str) -> bool {
        self.used_in_crafting.contains(path)
    }

    pub fn get(&self, path: &str) -> Option<&ItemInfo> {
        self.items.get(path)
    }

    /// A readable name for any path: the WFCD name if known, then a Warframe blueprint name from
    /// its codename, otherwise one made from the path.
    pub fn display_name(&self, path: &str) -> String {
        if let Some(info) = self.get(path) {
            return info.name.clone();
        }
        self.frame_blueprint_name(path).unwrap_or_else(|| fallback_name(path))
    }
}

/// An item's build parts, one entry per path. WFCD lists a dual weapon's single weapon twice
/// (Akbronco: Bronco, Bronco), which is one part needed twice.
fn merge_parts(components: &[RawComponent]) -> Vec<Part> {
    let mut parts: Vec<Part> = Vec::new();
    for c in components.iter().filter(|c| !c.unique_name.is_empty()) {
        let count = c.item_count.map_or(1, |n| n.max(1.0) as u32);
        match parts.iter_mut().find(|p| p.path == c.unique_name) {
            Some(p) => p.count += count,
            None => parts.push(Part { path: c.unique_name.clone(), count }),
        }
    }
    parts
}

/// Keeps an item's drop list and build costs (the first listing wins, as for `items`).
fn keep_sources(item: &RawItem, drops: &mut HashMap<String, Vec<Drop>>, builds: &mut HashMap<String, Build>) {
    if !item.drops.is_empty() && !drops.contains_key(&item.unique_name) {
        // WFCD's text has a broken "ö" in Höllvania.
        let list = item
            .drops
            .iter()
            .filter(|d| !d.location.is_empty())
            .map(|d| Drop { location: d.location.replace("H\u{FFFD}llvania", "Höllvania"), chance: d.chance.unwrap_or(0.0) })
            .collect();
        drops.insert(item.unique_name.clone(), list);
    }
    if item.build_price.is_some() || item.market_cost.is_some() {
        builds.entry(item.unique_name.clone()).or_insert_with(|| Build {
            credits: item.build_price.unwrap_or(0.0) as i64,
            time: item.build_time.unwrap_or(0.0) as i64,
            quantity: item.build_quantity.unwrap_or(1.0) as i64,
            market_cost: item.market_cost.filter(|c| *c > 0.0).map(|c| c as i64),
            bp_cost: item.bp_cost.filter(|c| *c > 0.0).map(|c| c as i64),
        });
    }
}

fn info(item: &RawItem, name: String, category: &str) -> ItemInfo {
    // Mods carry `fusionLimit`; arcanes only list one stat block per rank.
    let max_rank = item
        .fusion_limit
        .map(|f| f as u32)
        .or_else(|| (!item.level_stats.is_empty()).then(|| item.level_stats.len() as u32 - 1));
    ItemInfo {
        name,
        category: item.category.clone().unwrap_or_else(|| category.to_string()),
        image: item.image_name.clone(),
        ducats: item.ducats.filter(|d| *d > 0.0).map(|d| d as u32),
        tradable: item.tradable.unwrap_or(false),
        file: category.to_string(),
        kind: item.kind.clone(),
        rarity: item.rarity.clone(),
        polarity: item.polarity.clone(),
        drain: item.base_drain.map(|d| d as i32),
        max_rank,
        compat: item.compat_name.clone(),
        vaulted: item.vaulted.unwrap_or(false),
        wiki_image: item.wikia_thumbnail.as_deref().filter(|_| category == "Mods").and_then(wiki_cache_name),
        underlay: None,
        level_stats: if matches!(category, "Mods" | "Arcanes") {
            item.level_stats.iter().map(|l| l.stats.clone()).collect()
        } else {
            Vec::new()
        },
    }
}

/// An item name without the game's text markup: `<Shard_blue_simple> Azure Archon Shard` →
/// "Azure Archon Shard", `File-A-Style<Retro_tm> Binder` → "File-A-Style™ Binder", `Nightwave
/// <Reputation_small> Standing` → "Nightwave Standing". The tags are icons in game.
pub fn clean_name(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let Some(len) = rest[start..].find('>') else {
            rest = &rest[start..];
            break;
        };
        if &rest[start + 1..start + len] == "Retro_tm" {
            out.push('™');
        } else {
            out.push(' ');
        }
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Turns the last part of a path into words: `/.../BratonPrimeBarrel` → "Braton Prime Barrel".
pub fn fallback_name(path: &str) -> String {
    let last = path.rsplit('/').next().unwrap_or(path);
    let mut out = String::new();
    let mut prev: Option<char> = None;
    for c in last.chars() {
        if let Some(p) = prev {
            let boundary = (c.is_uppercase() && (p.is_lowercase() || p.is_ascii_digit()))
                || (c.is_ascii_digit() && p.is_alphabetic());
            if boundary {
                out.push(' ');
            }
        }
        out.push(c);
        prev = Some(c);
    }
    out
}

/// Loads cached category files (and the drop data) from `dir`, downloading any that are missing
/// or older than [`MAX_AGE`]. A failed or broken download falls back to the cached copy, so the
/// app works offline.
pub fn load_or_refresh(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let _ = std::fs::create_dir_all(dir);
    let sources = CATEGORIES.iter().map(|c| (*c, format!("{BASE_URL}{c}.json"))).chain([(DROPS, DROPS_URL.to_string())]);
    let mut files = Vec::new();
    for (name, url) in sources {
        let path = dir.join(format!("{name}.json"));
        let fresh = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age < MAX_AGE);
        if !fresh {
            match download(&url) {
                Ok(bytes) if usable(name, &bytes) || !path.exists() => {
                    let tmp = path.with_extension("json.tmp");
                    if std::fs::write(&tmp, &bytes).is_ok() {
                        let _ = std::fs::rename(&tmp, &path);
                    }
                }
                Ok(_) => {
                    eprintln!("[translate] downloaded {name}.json looks broken; keeping the cached copy");
                    // Try again after MAX_AGE rather than at every start.
                    let _ = std::fs::File::options().write(true).open(&path).and_then(|f| f.set_modified(SystemTime::now()));
                }
                Err(e) => eprintln!("[translate] download {name}.json failed: {e}"),
            }
        }
        if let Ok(bytes) = std::fs::read(&path) {
            files.push((name.to_string(), bytes));
        }
    }
    files
}

/// Loads only what's already cached, without touching the network. Used at startup.
pub fn load_cached(dir: &Path) -> Vec<(String, Vec<u8>)> {
    CATEGORIES
        .iter()
        .chain(&[DROPS])
        .filter_map(|c| std::fs::read(dir.join(format!("{c}.json"))).ok().map(|b| (c.to_string(), b)))
        .collect()
}

/// Whether a downloaded file is good enough to replace the cached copy: it parses and isn't
/// empty, and relics come with rewards.
fn usable(name: &str, bytes: &[u8]) -> bool {
    if name == DROPS {
        return !drop_tables(bytes).is_empty();
    }
    match serde_json::from_slice::<Vec<RawItem>>(bytes) {
        Ok(items) if name == "Relics" => items.iter().any(|i| !i.rewards.is_empty()),
        Ok(items) => !items.is_empty(),
        Err(_) => false,
    }
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let mut response = crate::net::agent().get(url).call().map_err(|e| e.to_string())?;
    response
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Vec<(String, Vec<u8>)> {
        let primary = r#"[{"uniqueName":"/Lotus/Weapons/Tenno/Rifle/BratonPrime","name":"Braton Prime",
            "category":"Primary","imageName":"braton-prime.png","tradable":false,
            "components":[{"uniqueName":"/Lotus/Types/Recipes/Weapons/WeaponParts/BratonPrimeBarrel"},
                          {"uniqueName":"/Lotus/Types/Recipes/Weapons/BratonPrimeBlueprint"},
                          {"uniqueName":"/Lotus/Types/Items/MiscItems/OrokinCell"}]}]"#;
        let components = r#"[{"uniqueName":"/Lotus/Types/Recipes/Weapons/WeaponParts/BratonPrimeBarrel","name":"Barrel","ducats":15,"tradable":true},
            {"uniqueName":"/Lotus/Types/Recipes/Weapons/BratonPrimeBlueprint","name":"Blueprint","ducats":45},
            {"uniqueName":"/Lotus/Types/Items/MiscItems/OrokinCell","name":"Orokin Cell"}]"#;
        let misc = r#"[{"uniqueName":"/Lotus/Types/Items/MiscItems/OrokinCell","name":"Orokin Cell","category":"Misc"}]"#;
        let relics = r#"[{"uniqueName":"/Lotus/Types/Game/Projections/T1VoidProjectionAtlasPrimeABronze","name":"Lith S8 Intact","tradable":true}]"#;
        vec![
            ("Components".into(), components.into()),
            ("Primary".into(), primary.into()),
            ("Misc".into(), misc.into()),
            ("Relics".into(), relics.into()),
        ]
    }

    #[test]
    fn names_lose_game_markup_and_tauforged_shards_get_an_underlay() {
        assert_eq!(clean_name("<Shard_blue_simple> Azure Archon Shard"), "Azure Archon Shard");
        assert_eq!(clean_name("File-A-Style<Retro_tm> Binder"), "File-A-Style™ Binder");
        assert_eq!(clean_name("Nightwave <Reputation_small> Standing"), "Nightwave Standing");
        assert_eq!(clean_name("The Scooticle<Retro_tm>"), "The Scooticle™");
        let misc = r#"[
            {"uniqueName":"/N/ArchonCrystalAmar","name":"<Shard_red_simple> Crimson Archon Shard","imageName":"ArchonShardAmar.png"},
            {"uniqueName":"/N/ArchonCrystalAmarMythic","name":"<Shard_red_simple> Tauforged Crimson Archon Shard","imageName":"ArchonShardAmarMythicGlow.png"}]"#;
        let index = ItemIndex::build(&[("Misc".to_string(), misc.as_bytes().to_vec())]);
        let tau = index.get("/N/ArchonCrystalAmarMythic").unwrap();
        assert_eq!(tau.name, "Tauforged Crimson Archon Shard");
        assert_eq!(tau.underlay.as_deref(), Some("ArchonShardAmar.png"));
        assert!(index.get("/N/ArchonCrystalAmar").unwrap().underlay.is_none());
    }

    #[test]
    fn names_prime_parts_from_their_parent() {
        let index = ItemIndex::build(&files());
        let barrel = index.get("/Lotus/Types/Recipes/Weapons/WeaponParts/BratonPrimeBarrel").unwrap();
        assert_eq!(barrel.name, "Braton Prime Barrel");
        assert_eq!(barrel.ducats, Some(15));
        assert!(barrel.tradable);
        assert_eq!(index.display_name("/Lotus/Types/Recipes/Weapons/BratonPrimeBlueprint"), "Braton Prime Blueprint");
    }

    #[test]
    fn dual_weapons_need_their_single_twice() {
        let secondary = br#"[{"uniqueName":"/W/Akbronco","name":"Akbronco","masterable":true,
            "components":[{"uniqueName":"/W/AkbroncoBlueprint"},{"uniqueName":"/W/Bronco","itemCount":1},{"uniqueName":"/W/Bronco","itemCount":1}]}]"#;
        let index = ItemIndex::build(&[("Secondary".into(), secondary.to_vec())]);
        let parts: Vec<(&str, u32)> = index.masterables()[0].parts.iter().map(|p| (p.path.as_str(), p.count)).collect();
        assert_eq!(parts, [("/W/AkbroncoBlueprint", 1), ("/W/Bronco", 2)]);
    }

    #[test]
    fn resources_keep_their_own_name() {
        let index = ItemIndex::build(&files());
        let cell = index.get("/Lotus/Types/Items/MiscItems/OrokinCell").unwrap();
        assert_eq!(cell.name, "Orokin Cell");
        assert_eq!(cell.category, "Misc");
    }

    #[test]
    fn relics_and_unknown_paths() {
        let index = ItemIndex::build(&files());
        assert_eq!(
            index.display_name("/Lotus/Types/Game/Projections/T1VoidProjectionAtlasPrimeABronze"),
            "Lith S8 Intact"
        );
        assert_eq!(index.display_name("/Lotus/Types/Items/MiscItems/CubicsRailjackItem"), "Cubics Railjack Item");
        assert_eq!(fallback_name("/Lotus/Types/Game/T1VoidProjection"), "T 1 Void Projection");
    }

    #[test]
    fn xp_curves() {
        assert_eq!(XpCurve::Weapon.xp_for_rank(30), 450_000);
        assert_eq!(XpCurve::Frame.xp_for_rank(30), 900_000);
        assert_eq!(XpCurve::Weapon.xp_for_rank(40), 800_000);
        assert_eq!(XpCurve::Weapon.rank_for_xp(449_999, 30), 29);
        assert_eq!(XpCurve::Weapon.rank_for_xp(10_000_000, 30), 30, "capped at max rank");
        assert_eq!(XpCurve::Frame.rank_for_xp(0, 30), 0);
    }

    #[test]
    fn classifies_masterables_into_tabs() {
        let files: Vec<(String, Vec<u8>)> = vec![
            ("Warframes".into(), br#"[{"uniqueName":"/F/Ash","name":"Ash","masterable":true,"type":"Warframe"},
                {"uniqueName":"/F/Voidrig","name":"Voidrig","masterable":true,"type":"Necramech","maxLevelCap":40},
                {"uniqueName":"/Lotus/Powersuits/SiriusOrion/OrionSuit","name":"Orion & Sirius","masterable":true,"type":"Warframe"}]"#.to_vec()),
            ("Melee".into(), br#"[{"uniqueName":"/M/Prova","name":"Prova","masterable":true,"type":"Rifle","masteryReq":2},
                {"uniqueName":"/M/Strike","name":"Balla","masterable":true,"type":"Zaw Component"}]"#.to_vec()),
            ("Misc".into(), br#"[{"uniqueName":"/A/Prism","name":"Mote Prism","masterable":false,"type":"Amp"},
                {"uniqueName":"/A/Brace","name":"Certus Brace","masterable":false,"type":"Amp"},
                {"uniqueName":"/A/Scaffold","name":"Phahd Scaffold","masterable":false,"type":"Amp"},
                {"uniqueName":"/X/Ferrite","name":"Ferrite","type":"Resource"}]"#.to_vec()),
            ("Pets".into(), br#"[{"uniqueName":"/P/Venari","name":"Venari","masterable":false,"type":"Warframe"},
                {"uniqueName":"/P/Moa","name":"Para Moa","masterable":true,"type":"Pets"}]"#.to_vec()),
            ("SentinelWeapons".into(), br#"[{"uniqueName":"/S/Gun","name":"Laser Rifle","masterable":true,
                "components":[{"uniqueName":"/R/GunBlueprint","itemCount":1},{"uniqueName":"/F/Ash","itemCount":2}]}]"#.to_vec()),
        ];
        let index = ItemIndex::build(&files);
        let tab = |p: &str| index.masterables().iter().find(|m| m.path == p).map(|m| (m.tab, m.curve));
        assert_eq!(tab("/F/Ash"), Some((Tab::Warframe, XpCurve::Frame)));
        assert_eq!(tab("/F/Voidrig"), Some((Tab::Arch, XpCurve::Frame)));
        assert_eq!(tab("/M/Prova"), Some((Tab::Melee, XpCurve::Weapon)), "WFCD type 'Rifle' is ignored");
        assert_eq!(tab("/M/Strike"), Some((Tab::Modular, XpCurve::Weapon)));
        assert_eq!(tab("/A/Prism"), Some((Tab::Modular, XpCurve::Weapon)), "amp prisms count despite the flag");
        assert_eq!(tab("/A/Brace"), None, "only prisms give mastery");
        assert_eq!(tab("/A/Scaffold"), None, "only prisms give mastery");
        assert_eq!(tab("/P/Moa"), Some((Tab::Companion, XpCurve::Frame)));
        assert_eq!(tab("/S/Gun"), Some((Tab::Companion, XpCurve::Weapon)));
        assert_eq!(tab("/P/Venari"), Some((Tab::Companion, XpCurve::Frame)), "gives mastery despite the flag");
        assert_eq!(
            tab("/Lotus/Types/Game/CrewShip/RailJack/DefaultHarness"),
            Some((Tab::Arch, XpCurve::Frame)),
            "Plexus is added even though WFCD doesn't list it"
        );
        assert_eq!(index.display_name("/Lotus/Types/Game/CrewShip/RailJack/DefaultHarness"), "Plexus");
        assert_eq!(tab("/Lotus/Powersuits/SiriusOrion/OrionSuit"), None, "half of Sirius & Orion, not obtainable");
        assert_eq!(tab("/X/Ferrite"), None);
        assert_eq!(index.masterables().len(), 9, "7 from WFCD + Venari + Plexus");

        let voidrig = index.masterables().iter().find(|m| m.path == "/F/Voidrig").unwrap();
        assert_eq!(voidrig.max_rank, 40);
        let gun = index.masterables().iter().find(|m| m.path == "/S/Gun").unwrap();
        assert_eq!(gun.parts, vec![
            Part { path: "/R/GunBlueprint".into(), count: 1 },
            Part { path: "/F/Ash".into(), count: 2 },
        ]);
        assert!(index.used_in_crafting("/F/Ash"));
        assert!(!index.used_in_crafting("/F/Voidrig"));
    }

    #[test]
    fn relics_without_rewards_are_filled_from_drop_data_or_the_snapshot() {
        // As WFCD shipped them on 2026-10-23: names, no rewards.
        let relics = br#"[{"uniqueName":"/R/T14","name":"Lith T14 Intact","rewards":[]},
            {"uniqueName":"/R/T14R","name":"Lith T14 Radiant","rewards":[]},
            {"uniqueName":"/R/Gone","name":"Lith Z99 Intact","rewards":[]}]"#;
        let misc = br#"[{"uniqueName":"/Forma","name":"Forma Blueprint","imageName":"forma.png"}]"#;
        let drops = br#"{"relics":[
            {"tier":"Lith","relicName":"T14","state":"Radiant","rewards":[{"itemName":"Fang Prime Handle","chance":20}]},
            {"tier":"Lith","relicName":"T14","state":"Intact","rewards":[
                {"itemName":"Fang Prime Handle","chance":25.33},
                {"itemName":"2X Forma Blueprint","chance":11},
                {"itemName":"Brand New Prime Barrel","chance":2}]},
            {"tier":"Requiem","state":"Intact","rewards":[{"itemName":"Lohk","chance":10}]}]}"#;
        let base = vec![("Relics".to_string(), relics.to_vec()), ("Misc".to_string(), misc.to_vec())];

        let mut files = base.clone();
        files.push((DROPS.to_string(), drops.to_vec()));
        let index = ItemIndex::build(&files);
        let rewards = index.relic_rewards("Lith T14");
        let got: Vec<(&str, &str, f64)> = rewards.iter().map(|r| (r.name.as_str(), r.path.as_str(), r.chance)).collect();
        assert_eq!(
            got,
            vec![
                ("Fang Prime Handle", "/Lotus/Types/Recipes/Weapons/WeaponParts/PrimeFangHandle", 25.33),
                ("2X Forma Blueprint", "/Lotus/Types/Recipes/Components/FormaBlueprint", 11.0),
                ("Brand New Prime Barrel", "", 2.0),
            ],
            "Intact table; paths from the snapshot, unknown items keep their name"
        );
        assert!(index.relic_rewards("Lith Z99").is_empty(), "in neither source");

        // No drop data: the bundled snapshot (made from the real tables) fills it.
        let index = ItemIndex::build(&base);
        let rewards = index.relic_rewards("Lith T14");
        assert_eq!(rewards.len(), 6);
        assert!(rewards.iter().all(|r| r.path.starts_with("/Lotus/")));
    }

    #[test]
    fn broken_downloads_dont_replace_the_cache() {
        assert!(!usable("Relics", br#"[{"uniqueName":"/R/A","name":"Lith A1 Intact","rewards":[]}]"#));
        assert!(usable("Relics", br#"[{"uniqueName":"/R/A","name":"Lith A1 Intact","rewards":[{"chance":2,"item":{"uniqueName":"/P/F","name":"F"}}]}]"#));
        assert!(usable("Misc", br#"[{"uniqueName":"/M/A","name":"A"}]"#));
        assert!(!usable("Misc", b"[]"));
        assert!(!usable("Misc", b"<html>rate limited</html>"));
        assert!(!usable(DROPS, br#"{"relics":[]}"#));
        assert!(usable(DROPS, br#"{"relics":[{"tier":"Lith","relicName":"A1","state":"Intact","rewards":[{"itemName":"F","chance":2}]}]}"#));
    }

    #[test]
    fn relics_codenames_and_mod_details() {
        let files: Vec<(String, Vec<u8>)> = vec![
            ("Warframes".into(), br#"[{"uniqueName":"/Lotus/Powersuits/Paladin/Paladin","name":"Oberon","masterable":true},
                {"uniqueName":"/Lotus/Powersuits/Mesa/MesaPrime","name":"Mesa Prime","masterable":true}]"#.to_vec()),
            ("Relics".into(), br#"[
                {"uniqueName":"/R/S8Radiant","name":"Lith S8 Radiant","vaulted":true,"rewards":[
                    {"chance":10,"item":{"uniqueName":"/P/Strad","name":"Stradavar Prime Blueprint"}}]},
                {"uniqueName":"/R/S8Intact","name":"Lith S8 Intact","vaulted":true,"rewards":[
                    {"chance":2,"item":{"uniqueName":"/P/Strad","name":"Stradavar Prime Blueprint"}},
                    {"chance":25.33,"item":{"uniqueName":"/P/Forma","name":"Forma Blueprint"}}]}]"#.to_vec()),
            ("Mods".into(), br#"[{"uniqueName":"/M/Link","name":"Abating Link","type":"Warframe Mod","rarity":"Rare",
                "polarity":"zenurik","baseDrain":6,"fusionLimit":3,"compatName":"Trinity"}]"#.to_vec()),
            ("Arcanes".into(), br#"[{"uniqueName":"/A/Slip","name":"Akimbo Slip Shot","type":"Secondary Arcane",
                "rarity":"Rare","levelStats":[{},{},{},{},{},{}]}]"#.to_vec()),
        ];
        let index = ItemIndex::build(&files);

        assert_eq!(split_relic_name("Lith S8 Radiant"), ("Lith S8", "Radiant"));
        assert_eq!(split_relic_name("Requiem I Relic"), ("Requiem I Relic", "Intact"));
        let rewards = index.relic_rewards("Lith S8");
        assert_eq!(rewards.len(), 2, "Intact entry wins over Radiant");
        assert_eq!(rewards[1].chance, 25.33);
        assert!(index.get("/R/S8Intact").unwrap().vaulted);

        assert_eq!(index.display_name("/Lotus/Types/Recipes/WarframeRecipes/PaladinChassisBlueprint"), "Oberon Chassis Blueprint");
        assert_eq!(index.display_name("/Lotus/Types/Recipes/WarframeRecipes/MesaPrimeHelmetBlueprint"), "Mesa Prime Neuroptics Blueprint");
        assert_eq!(index.display_name("/Lotus/Types/Recipes/WarframeRecipes/PaladinBlueprint"), "Oberon Blueprint");
        assert_eq!(index.display_name("/Lotus/Types/Recipes/WarframeRecipes/UnknownChassisBlueprint"), "Unknown Chassis Blueprint");
        assert_eq!(
            index.display_name("/Lotus/Types/Recipes/WarframeRecipes/AshHelmetBlueprint"),
            "Ash Neuroptics Blueprint",
            "real name instead of codename still gets Neuroptics"
        );
        // Equinox's aspects: Anima = Night, Animus = Day.
        let wr = "/Lotus/Types/Recipes/WarframeRecipes/";
        assert_eq!(index.display_name(&format!("{wr}AnimaBlueprint")), "Equinox Night Aspect Blueprint");
        assert_eq!(index.display_name(&format!("{wr}AnimusBlueprint")), "Equinox Day Aspect Blueprint");
        assert_eq!(index.display_name(&format!("{wr}AnimaChassisBlueprint")), "Equinox Night Chassis Blueprint");
        assert_eq!(index.display_name(&format!("{wr}AnimusHelmetBlueprint")), "Equinox Day Neuroptics Blueprint");
        assert_eq!(index.display_name(&format!("{wr}AnimusSystemsBlueprint")), "Equinox Day Systems Blueprint");

        assert_eq!(index.blueprint_overlay("/Lotus/Types/Recipes/WarframeRecipes/PaladinChassisBlueprint"), None, "Oberon has no image here");
        assert_eq!(index.blueprint_overlay("/M/Link"), None, "not a blueprint");

        let link = index.get("/M/Link").unwrap();
        assert_eq!((link.max_rank, link.drain, link.compat.as_deref()), (Some(3), Some(6), Some("Trinity")));
        assert_eq!(index.get("/A/Slip").unwrap().max_rank, Some(5), "6 stat blocks = ranks 0..5");
    }

    #[test]
    fn blueprint_overlays() {
        let files: Vec<(String, Vec<u8>)> = vec![
            ("Primary".into(), br#"[{"uniqueName":"/W/BratonPrime","name":"Braton Prime","imageName":"braton-prime.png",
                "components":[{"uniqueName":"/Lotus/Types/Recipes/Weapons/BratonPrimeBlueprint"}]}]"#.to_vec()),
            ("Warframes".into(), br#"[{"uniqueName":"/Lotus/Powersuits/Paladin/Paladin","name":"Oberon","imageName":"oberon.png"}]"#.to_vec()),
            ("Misc".into(), br#"[{"uniqueName":"/A/StoreForma","name":"Forma"},
                {"uniqueName":"/X/Forma","name":"Forma","imageName":"forma.png"}]"#.to_vec()),
            ("Gear".into(), br#"[{"uniqueName":"/Lotus/Types/Restoratives/Cipher","name":"Cipher","imageName":"cipher.png",
                "components":[{"uniqueName":"/Lotus/Types/StoreItems/Consumables/CipherBlueprint"}]}]"#.to_vec()),
            ("Components".into(), br#"[{"uniqueName":"/Lotus/Types/Recipes/Weapons/BratonPrimeBlueprint","name":"Blueprint","imageName":"blueprint.png"},
                {"uniqueName":"/Lotus/Types/Recipes/FormaBlueprint","name":"Forma Blueprint","imageName":"blueprint.png"},
                {"uniqueName":"/Lotus/Types/StoreItems/Consumables/CipherBlueprint","name":"Blueprint","imageName":"blueprint.png"}]"#.to_vec()),
        ];
        let index = ItemIndex::build(&files);
        assert_eq!(
            index.display_name("/Lotus/Types/StoreItems/Consumables/CipherBlueprint"),
            "Cipher Blueprint",
            "bare Blueprint outside /Recipes/ still gets its parent's name"
        );
        assert_eq!(index.blueprint_overlay("/Lotus/Types/StoreItems/Consumables/CipherBlueprint").as_deref(), Some("cipher.png"));
        assert_eq!(index.blueprint_overlay("/Lotus/Types/Recipes/Weapons/BratonPrimeBlueprint").as_deref(), Some("braton-prime.png"));
        assert_eq!(index.blueprint_overlay("/Lotus/Types/Recipes/FormaBlueprint").as_deref(), Some("forma.png"));
        assert_eq!(
            index.blueprint_overlay("/Lotus/Types/Recipes/WarframeRecipes/PaladinChassisBlueprint").as_deref(),
            Some("oberon.png"),
            "part blueprint falls back to the frame"
        );
        assert_eq!(index.blueprint_overlay("/W/BratonPrime"), None);
    }

    #[test]
    fn warframe_systems_use_generic_images_and_equipment_names() {
        let files: Vec<(String, Vec<u8>)> = vec![
            // An ephemera listed first that also uses Ash's Systems: the Warframe must still win.
            ("Skins".into(), br#"[{"uniqueName":"/S/SmokingBody","name":"Smoking Body Ephemera",
                "components":[{"uniqueName":"/Lotus/Types/Recipes/WarframeRecipes/AshSystemsComponent"}]}]"#.to_vec()),
            ("Warframes".into(), br#"[{"uniqueName":"/Lotus/Powersuits/Ninja/Ninja","name":"Ash","imageName":"Ash.png",
                "components":[{"uniqueName":"/Lotus/Types/Recipes/WarframeRecipes/AshSystemsComponent"}]}]"#.to_vec()),
            ("Components".into(), br#"[{"uniqueName":"/Lotus/Types/Recipes/WarframeRecipes/AshSystemsComponent","name":"Systems",
                "imageName":"GenericWarframeSystem.png"}]"#.to_vec()),
        ];
        let index = ItemIndex::build(&files);
        assert_eq!(index.display_name("/Lotus/Types/Recipes/WarframeRecipes/AshSystemsComponent"), "Ash Systems");
        assert_eq!(index.parent_file("/Lotus/Types/Recipes/WarframeRecipes/AshSystemsComponent"), Some("Warframes"));
        assert_eq!(
            index.blueprint_overlay("/Lotus/Types/Recipes/WarframeRecipes/AshSystemsBlueprint").as_deref(),
            Some(SYSTEMS_IMAGE),
            "not Ash.png"
        );
        assert_eq!(
            index.blueprint_overlay("/Lotus/Types/Recipes/WarframeRecipes/AtlasPrimeSystemsBlueprint").as_deref(),
            Some(PRIME_SYSTEMS_IMAGE)
        );
        assert_eq!(
            index.blueprint_overlay("/Lotus/Types/Recipes/WarframeRecipes/SiriusOrionSystemsBlueprint").as_deref(),
            Some(SYSTEMS_IMAGE),
            "not the Sirius colour palette"
        );
    }

    #[test]
    fn wiki_card_cache_names() {
        assert_eq!(
            wiki_cache_name("https://wiki.warframe.com/images/SerrationMod.png?0b8ff").as_deref(),
            Some("wiki__SerrationMod.png")
        );
        assert_eq!(
            wiki_cache_name("https://wiki.warframe.com/images/Hunter%27sMunitionsMod.png").as_deref(),
            Some("wiki__Hunter'sMunitionsMod.png")
        );
        assert_eq!(wiki_cache_name("https://example.com/images/x.png"), None);
        assert_eq!(wiki_cache_name("https://wiki.warframe.com/images/a/b.png"), None);
    }

    #[test]
    fn bad_file_is_skipped_not_fatal() {
        let mut f = files();
        f.push(("Mods".into(), b"not json".to_vec()));
        // Braton Prime, its Barrel and Blueprint, Orokin Cell, the relic.
        assert_eq!(ItemIndex::build(&f).len(), 5);
    }
}
