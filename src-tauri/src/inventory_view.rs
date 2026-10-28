//! The Inventory page: parts, relics, mods, arcanes and everything else the account holds.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::foundry::blueprint_of;
use crate::pipeline::Shared;
use crate::translate::wfcd::{split_relic_name, BLUEPRINT_IMAGE, REFINEMENTS};
use crate::translate::{split_rank_key, Flat, ItemIndex, RANKS};

/// A part, blueprint or misc item.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tile {
    pub path: String,
    pub name: String,
    pub image: Option<String>,
    /// For blueprints: the image of what it builds, drawn over the blueprint picture.
    pub overlay: Option<String>,
    /// Drawn under the image (Tauforged Archon Shards: the shard under WFCD's glow-only image).
    pub underlay: Option<String>,
    pub count: i64,
    /// "Blueprint" / "Part" on the Parts tab; the WFCD type ("Resource", "Gem", …) on Misc.
    pub kind: String,
    pub prime: bool,
    /// What a part builds: "warframe", "weapon", "companion" or "other".
    pub builds: &'static str,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RankCount {
    pub rank: u32,
    pub count: i64,
}

/// A mod or arcane, all copies combined.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModTile {
    pub path: String,
    pub name: String,
    pub image: Option<String>,
    pub count: i64,
    /// Copies per rank, lowest rank first.
    pub ranks: Vec<RankCount>,
    pub kind: String,
    pub rarity: Option<String>,
    pub polarity: Option<String>,
    pub drain: Option<i32>,
    pub max_rank: Option<u32>,
    pub compat: Option<String>,
    /// Full card image on the Warframe wiki (shown at max rank).
    pub wiki_image: Option<String>,
    /// The highest rank among the owned copies, and that rank's stat lines.
    pub top_rank: u32,
    pub stats: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RewardView {
    pub path: String,
    pub name: String,
    pub image: Option<String>,
    pub overlay: Option<String>,
    /// "Common", "Uncommon" or "Rare".
    pub rarity: &'static str,
    pub owned: i64,
}

/// One relic with its amount per refinement.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelicRow {
    pub name: String,
    pub era: String,
    pub image: Option<String>,
    pub vaulted: bool,
    /// Amounts for Intact, Exceptional, Flawless, Radiant.
    pub counts: [i64; 4],
    pub total: i64,
    pub rewards: Vec<RewardView>,
}

/// One part of a prime set as it's traded: the main blueprint, a weapon part, or a warframe
/// part's blueprint (the recipe lists the built `…Component`, which can't be traded).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetPart {
    /// Its warframe.market item (`gameRef`).
    pub path: String,
    pub name: String,
    pub image: Option<String>,
    pub overlay: Option<String>,
    pub needed: u32,
    /// Tradable copies held (built warframe parts and built weapons don't count).
    pub owned: i64,
}

/// A prime set the account holds parts of.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrimeSet {
    /// The built item, which is also the set's warframe.market item (`frost_prime_set`).
    pub path: String,
    pub name: String,
    /// The main blueprint's picture, with the item drawn over it.
    pub image: Option<String>,
    pub overlay: Option<String>,
    pub vaulted: bool,
    /// Complete sets held: each part's copies over how many the set needs, the lowest of them.
    pub count: i64,
    pub parts: Vec<SetPart>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryData {
    pub parts: Vec<Tile>,
    pub sets: Vec<PrimeSet>,
    pub relics: Vec<RelicRow>,
    pub mods: Vec<ModTile>,
    pub arcanes: Vec<ModTile>,
    pub misc: Vec<Tile>,
}

/// Where a part's build target falls, from the WFCD file of the item that uses it.
fn builds_from_file(file: &str) -> &'static str {
    match file {
        "Warframes" => "warframe",
        "Primary" | "Secondary" | "Melee" | "Arch-Gun" | "Arch-Melee" | "SentinelWeapons" => "weapon",
        "Pets" | "Sentinels" => "companion",
        _ => "other",
    }
}

/// Fallback for parts WFCD doesn't list, from their path.
fn builds_from_path(path: &str) -> &'static str {
    if path.contains("/WarframeRecipes/") {
        "warframe"
    } else if path.contains("/Weapons/") {
        "weapon"
    } else if ["/Sentinels/", "Kubrow", "Catbrow", "/Pets/"].iter().any(|k| path.contains(k)) {
        "companion"
    } else {
        "other"
    }
}

/// Ranks a relic's rewards by Intact drop chance: the 3 most likely are Common, the next 2
/// Uncommon, the last Rare. (WFCD's own `rarity` labels are unreliable for relic rewards.)
fn reward_rarities(chances: &[f64]) -> Vec<&'static str> {
    let mut order: Vec<usize> = (0..chances.len()).collect();
    order.sort_by(|&a, &b| chances[b].partial_cmp(&chances[a]).unwrap_or(Ordering::Equal));
    let mut out = vec!["Common"; chances.len()];
    for (position, &i) in order.iter().enumerate() {
        out[i] = match position {
            0..=2 => "Common",
            3..=4 => "Uncommon",
            _ => "Rare",
        };
    }
    out
}

/// Compares names so "Lith S2" sorts before "Lith S10".
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let chunks = |s: &str| -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = Vec::new();
        for c in s.chars() {
            let digit = c.is_ascii_digit();
            match out.last_mut() {
                Some((d, text)) if *d == digit => text.push(c),
                _ => out.push((digit, c.to_string())),
            }
        }
        out
    };
    for (x, y) in chunks(a).iter().zip(chunks(b).iter()) {
        let ord = match (x.0, y.0) {
            (true, true) => x.1.parse::<u64>().unwrap_or(0).cmp(&y.1.parse::<u64>().unwrap_or(0)),
            _ => x.1.to_lowercase().cmp(&y.1.to_lowercase()),
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    a.len().cmp(&b.len())
}

const ERAS: [&str; 5] = ["Lith", "Meso", "Neo", "Axi", "Requiem"];

fn era_rank(era: &str) -> usize {
    ERAS.iter().position(|e| *e == era).unwrap_or(ERAS.len())
}

#[derive(Default)]
struct ModAcc {
    ranks: BTreeMap<u32, i64>,
}

/// Builds the Inventory tabs from the stored state. Pure, so it can be tested without Tauri.
pub fn build(index: &ItemIndex, current: &Flat) -> InventoryData {
    let mut data = InventoryData::default();
    let mut relics: HashMap<String, RelicRow> = HashMap::new();
    let mut mods: HashMap<String, ModAcc> = HashMap::new();
    let mut arcanes: HashMap<String, ModAcc> = HashMap::new();
    let mut owned: HashMap<&str, i64> = HashMap::new();

    for ((section, path), &value) in current {
        if value <= 0 {
            continue;
        }
        if matches!(section.as_str(), "MiscItems" | "Recipes") {
            *owned.entry(path.as_str()).or_insert(0) += value;
        }
        let info = index.get(path);
        let file = info.map(|i| i.file.as_str());
        match section.as_str() {
            "Recipes" => data.parts.push(part_tile(index, path, value, true)),
            "MiscItems" if file == Some("Relics") => {
                let info = info.expect("checked above");
                let (base, refinement) = split_relic_name(&info.name);
                let row = relics.entry(base.to_string()).or_insert_with(|| RelicRow {
                    name: base.to_string(),
                    era: base.split(' ').next().unwrap_or(base).to_string(),
                    image: info.image.clone(),
                    vaulted: info.vaulted,
                    counts: [0; 4],
                    total: 0,
                    rewards: Vec::new(),
                });
                let slot = REFINEMENTS.iter().position(|r| *r == refinement).unwrap_or(0);
                row.counts[slot] += value;
                row.total += value;
                if refinement == "Intact" {
                    row.image = info.image.clone();
                }
            }
            "MiscItems" if file == Some("Components") || (file.is_none() && path.contains("/Recipes/")) => {
                data.parts.push(part_tile(index, path, value, path.ends_with("Blueprint")));
            }
            "MiscItems" | "Consumables" => data.misc.push(misc_tile(index, path, value)),
            "RawUpgrades" => add_upgrade(index, &mut mods, &mut arcanes, path, 0, value),
            s if s == RANKS => {
                if let Some((p, rank)) = split_rank_key(path) {
                    add_upgrade(index, &mut mods, &mut arcanes, p, rank, value);
                }
            }
            _ => {}
        }
    }

    for row in relics.values_mut() {
        let rewards = index.relic_rewards(&row.name);
        let chances: Vec<f64> = rewards.iter().map(|r| r.chance).collect();
        let rarities = reward_rarities(&chances);
        let mut views: Vec<RewardView> = rewards
            .iter()
            .zip(rarities)
            .map(|(r, rarity)| RewardView {
                path: r.path.clone(),
                name: r.name.clone(),
                image: index.get(&r.path).and_then(|i| i.image.clone()),
                overlay: index.blueprint_overlay(&r.path),
                rarity,
                owned: owned.get(r.path.as_str()).copied().unwrap_or(0),
            })
            .collect();
        let order = |r: &str| ["Common", "Uncommon", "Rare"].iter().position(|x| *x == r).unwrap_or(3);
        views.sort_by(|a, b| order(a.rarity).cmp(&order(b.rarity)).then_with(|| a.name.cmp(&b.name)));
        row.rewards = views;
    }

    data.sets = prime_sets(index, &owned);
    data.relics = relics.into_values().collect();
    data.relics.sort_by(|a, b| era_rank(&a.era).cmp(&era_rank(&b.era)).then_with(|| natural_cmp(&a.name, &b.name)));
    data.mods = mod_tiles(index, mods);
    data.arcanes = mod_tiles(index, arcanes);
    data.parts.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    data.misc.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    data
}

/// Prime sets with at least one complete set of tradable parts held, by name. `owned` counts
/// MiscItems and Recipes, where tradable parts and blueprints are kept. Resources (Orokin Cells)
/// aren't set parts.
fn prime_sets(index: &ItemIndex, owned: &HashMap<&str, i64>) -> Vec<PrimeSet> {
    let mut sets: Vec<PrimeSet> = index
        .masterables()
        .iter()
        .filter(|m| m.is_prime)
        .filter_map(|m| {
            let parts: Vec<SetPart> = m
                .parts
                .iter()
                .filter(|p| !crate::mastery::is_resource(index, &p.path))
                .map(|p| {
                    let path = blueprint_of(&p.path).unwrap_or_else(|| p.path.clone());
                    let blueprint = path.ends_with("Blueprint");
                    let image = index.get(&path).and_then(|i| i.image.clone());
                    SetPart {
                        name: index.display_name(&path),
                        image: if blueprint { image.or_else(|| Some(BLUEPRINT_IMAGE.to_string())) } else { image },
                        overlay: if blueprint { index.blueprint_overlay(&path) } else { None },
                        needed: p.count.max(1),
                        owned: owned.get(path.as_str()).copied().unwrap_or(0),
                        path,
                    }
                })
                .collect();
            let count = parts.iter().map(|p| p.owned / i64::from(p.needed)).min().unwrap_or(0);
            if count == 0 {
                return None;
            }
            let info = index.get(&m.path);
            Some(PrimeSet {
                path: m.path.clone(),
                name: index.display_name(&m.path),
                image: Some(BLUEPRINT_IMAGE.to_string()),
                overlay: info.and_then(|i| i.image.clone()),
                vaulted: m.vaulted,
                count,
                parts,
            })
        })
        .collect();
    sets.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    sets
}

fn part_tile(index: &ItemIndex, path: &str, count: i64, blueprint: bool) -> Tile {
    let name = index.display_name(path);
    let image = index.get(path).and_then(|i| i.image.clone());
    Tile {
        path: path.to_string(),
        // Blueprints WFCD doesn't list still get the standard blueprint picture.
        image: if blueprint { image.or_else(|| Some(BLUEPRINT_IMAGE.to_string())) } else { image },
        overlay: if blueprint { index.blueprint_overlay(path) } else { None },
        underlay: None,
        count,
        kind: if blueprint { "Blueprint" } else { "Part" }.to_string(),
        prime: name.contains("Prime"),
        builds: index.parent_file_of_part(path).map_or_else(|| builds_from_path(path), builds_from_file),
        name,
    }
}

fn misc_tile(index: &ItemIndex, path: &str, count: i64) -> Tile {
    let info = index.get(path);
    let name = index.display_name(path);
    Tile {
        path: path.to_string(),
        image: info.and_then(|i| i.image.clone()),
        overlay: None,
        underlay: info.and_then(|i| i.underlay.clone()),
        count,
        kind: info.and_then(|i| i.kind.clone()).unwrap_or_else(|| "Other".to_string()),
        prime: name.contains("Prime"),
        builds: "other",
        name,
    }
}

fn add_upgrade(
    index: &ItemIndex,
    mods: &mut HashMap<String, ModAcc>,
    arcanes: &mut HashMap<String, ModAcc>,
    path: &str,
    rank: u32,
    count: i64,
) {
    if path.contains("/Randomized/") {
        return; // Rivens get their own page.
    }
    let target = match index.get(path).map(|i| i.file.as_str()) {
        Some("Arcanes") => arcanes,
        _ => mods,
    };
    *target.entry(path.to_string()).or_default().ranks.entry(rank).or_insert(0) += count;
}

fn mod_tiles(index: &ItemIndex, acc: HashMap<String, ModAcc>) -> Vec<ModTile> {
    let mut tiles: Vec<ModTile> = acc
        .into_iter()
        .map(|(path, a)| {
            let info = index.get(&path);
            let top_rank = a.ranks.keys().next_back().copied().unwrap_or(0);
            let stats = info
                .and_then(|i| {
                    let levels = &i.level_stats;
                    levels.get(top_rank as usize).or_else(|| levels.last()).cloned()
                })
                .unwrap_or_default();
            ModTile {
                wiki_image: info.and_then(|i| i.wiki_image.clone()),
                top_rank,
                stats,
                name: index.display_name(&path),
                image: info.and_then(|i| i.image.clone()),
                count: a.ranks.values().sum(),
                ranks: a.ranks.into_iter().map(|(rank, count)| RankCount { rank, count }).collect(),
                kind: info.and_then(|i| i.kind.clone()).unwrap_or_else(|| "Other".to_string()),
                rarity: info.and_then(|i| i.rarity.clone()),
                polarity: info.and_then(|i| i.polarity.clone()),
                drain: info.and_then(|i| i.drain),
                max_rank: info.and_then(|i| i.max_rank),
                compat: info.and_then(|i| i.compat.clone()),
                path,
            }
        })
        .collect();
    tiles.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    tiles
}

#[tauri::command]
pub fn get_inventory(shared: State<'_, Arc<Shared>>) -> Result<InventoryData, String> {
    let index = shared.items.read().unwrap().clone();
    let guard = shared.store.lock().unwrap();
    let Some(store) = guard.as_ref() else { return Ok(InventoryData::default()) };
    let Some(account) = store.latest_account().map_err(|e| e.to_string())? else {
        return Ok(InventoryData::default());
    };
    let current = store.current(&account).map_err(|e| e.to_string())?;
    Ok(build(&index, &current))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::parse;

    fn index() -> ItemIndex {
        ItemIndex::build(&[
            (
                "Primary".into(),
                br#"[{"uniqueName":"/W/BratonPrime","name":"Braton Prime","masterable":true,
                    "components":[{"uniqueName":"/Lotus/Types/Recipes/Weapons/WeaponParts/BratonPrimeBarrel"}]}]"#
                    .to_vec(),
            ),
            (
                "Components".into(),
                br#"[{"uniqueName":"/Lotus/Types/Recipes/Weapons/WeaponParts/BratonPrimeBarrel","name":"Barrel","imageName":"barrel.png"}]"#
                    .to_vec(),
            ),
            (
                "Relics".into(),
                br#"[{"uniqueName":"/R/S8I","name":"Lith S8 Intact","vaulted":true,"imageName":"lith.png","rewards":[
                        {"chance":25.33,"item":{"uniqueName":"/P/Forma","name":"Forma Blueprint"}},
                        {"chance":25.33,"item":{"uniqueName":"/Lotus/Types/Recipes/Weapons/WeaponParts/BratonPrimeBarrel","name":"Braton Prime Barrel"}},
                        {"chance":25.33,"item":{"uniqueName":"/P/Gram","name":"Gram Prime Blade"}},
                        {"chance":11,"item":{"uniqueName":"/P/Rubico","name":"Rubico Prime Barrel"}},
                        {"chance":11,"item":{"uniqueName":"/P/Orthos","name":"Orthos Prime Blueprint"}},
                        {"chance":2,"item":{"uniqueName":"/P/Strad","name":"Stradavar Prime Blueprint"}}]},
                    {"uniqueName":"/R/S8R","name":"Lith S8 Radiant","vaulted":true},
                    {"uniqueName":"/R/S10I","name":"Lith S10 Intact"},
                    {"uniqueName":"/R/A1I","name":"Axi A1 Intact"}]"#
                    .to_vec(),
            ),
            (
                "Mods".into(),
                br#"[{"uniqueName":"/M/Serration","name":"Serration","type":"Rifle Mod","rarity":"Uncommon","fusionLimit":10,
                    "wikiaThumbnail":"https://wiki.warframe.com/images/SerrationMod.png",
                    "levelStats":[{"stats":["+15% Damage"]},{"stats":["+30% Damage"]},{"stats":["+45% Damage"]},
                        {"stats":["+60% Damage"]},{"stats":["+75% Damage"]},{"stats":["+90% Damage"]},{"stats":["+105% Damage"]},
                        {"stats":["+120% Damage"]},{"stats":["+135% Damage"]},{"stats":["+150% Damage"]},{"stats":["+165% Damage"]}]}]"#.to_vec(),
            ),
            (
                "Arcanes".into(),
                br#"[{"uniqueName":"/A/Energize","name":"Arcane Energize","type":"Warframe Arcane","levelStats":[{},{},{},{},{},{}]}]"#.to_vec(),
            ),
            ("Resources".into(), br#"[{"uniqueName":"/X/Ferrite","name":"Ferrite","type":"Resource"}]"#.to_vec()),
        ])
    }

    #[test]
    fn sorts_items_into_tabs() {
        let raw = r#"{
            "MiscItems": [
                {"ItemCount": 1, "ItemType": "/Lotus/Types/Recipes/Weapons/WeaponParts/BratonPrimeBarrel"},
                {"ItemCount": 56, "ItemType": "/R/S8I"}, {"ItemCount": 1, "ItemType": "/R/S8R"},
                {"ItemCount": 3, "ItemType": "/R/S10I"}, {"ItemCount": 2, "ItemType": "/R/A1I"},
                {"ItemCount": 3462103, "ItemType": "/X/Ferrite"}],
            "Recipes": [{"ItemCount": 2, "ItemType": "/Lotus/Types/Recipes/WarframeRecipes/PaladinChassisBlueprint"}],
            "RawUpgrades": [{"ItemCount": 3, "ItemType": "/M/Serration"}, {"ItemCount": 1, "ItemType": "/A/Energize"}],
            "Upgrades": [
                {"UpgradeFingerprint": "{\"lvl\":10}", "ItemType": "/M/Serration"},
                {"UpgradeFingerprint": "{\"lvl\":8}", "ItemType": "/Lotus/Upgrades/Mods/Randomized/LotusRifleRandomModRare"}]
        }"#;
        let data = build(&index(), &parse(raw.as_bytes()).unwrap().values);

        let names: Vec<&str> = data.parts.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Braton Prime Barrel", "Paladin Chassis Blueprint"]);
        let barrel = &data.parts[0];
        assert_eq!((barrel.kind.as_str(), barrel.prime, barrel.builds), ("Part", true, "weapon"));
        assert_eq!((data.parts[1].kind.as_str(), data.parts[1].builds), ("Blueprint", "warframe"));

        let relic_names: Vec<&str> = data.relics.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(relic_names, ["Lith S8", "Lith S10", "Axi A1"], "era order, then natural order");
        let s8 = &data.relics[0];
        assert_eq!((s8.counts, s8.total, s8.vaulted), ([56, 0, 0, 1], 57, true));
        assert_eq!(s8.rewards.iter().map(|r| r.rarity).collect::<Vec<_>>(), ["Common", "Common", "Common", "Uncommon", "Uncommon", "Rare"]);
        let barrel_reward = s8.rewards.iter().find(|r| r.name == "Braton Prime Barrel").unwrap();
        assert_eq!(barrel_reward.owned, 1);

        assert_eq!(data.mods.len(), 1, "Riven left out");
        let serration = &data.mods[0];
        assert_eq!(serration.count, 4);
        assert_eq!(serration.ranks, vec![RankCount { rank: 0, count: 3 }, RankCount { rank: 10, count: 1 }]);
        assert_eq!(serration.max_rank, Some(10));
        assert_eq!((serration.top_rank, serration.stats.clone()), (10, vec!["+165% Damage".to_string()]));
        assert_eq!(serration.wiki_image.as_deref(), Some("wiki__SerrationMod.png"));
        assert_eq!(data.arcanes[0].name, "Arcane Energize");
        assert_eq!(data.arcanes[0].max_rank, Some(5));

        assert_eq!(data.misc.len(), 1);
        assert_eq!((data.misc[0].name.as_str(), data.misc[0].kind.as_str()), ("Ferrite", "Resource"));
    }

    #[test]
    fn prime_sets_count_tradable_parts() {
        let wr = "/Lotus/Types/Recipes/WarframeRecipes/";
        let wp = "/Lotus/Types/Recipes/Weapons/WeaponParts/";
        let warframes = format!(
            r#"[{{"uniqueName":"/F/FrostPrime","name":"Frost Prime","masterable":true,"isPrime":true,"vaulted":true,"imageName":"frost.png",
                "components":[{{"uniqueName":"{wr}FrostPrimeBlueprint"}},{{"uniqueName":"{wr}FrostPrimeChassisComponent"}},
                    {{"uniqueName":"/Lotus/Types/Items/MiscItems/OrokinCell","itemCount":2}}]}}]"#
        );
        let primary = format!(
            r#"[{{"uniqueName":"/W/BratonPrime","name":"Braton Prime","masterable":true,"isPrime":true,
                "components":[{{"uniqueName":"{wp}BratonPrimeBarrel","itemCount":2}}]}},
               {{"uniqueName":"/W/LatronPrime","name":"Latron Prime","masterable":true,"isPrime":true,
                "components":[{{"uniqueName":"{wp}LatronPrimeBarrel"}}]}},
               {{"uniqueName":"/W/Braton","name":"Braton","masterable":true,"components":[{{"uniqueName":"{wp}BratonBarrel"}}]}}]"#
        );
        let components = format!(
            r#"[{{"uniqueName":"{wr}FrostPrimeBlueprint","name":"Blueprint"}},{{"uniqueName":"{wr}FrostPrimeChassisComponent","name":"Chassis"}},
                {{"uniqueName":"{wp}BratonPrimeBarrel","name":"Barrel","imageName":"barrel.png"}},{{"uniqueName":"{wp}LatronPrimeBarrel","name":"Barrel"}},
                {{"uniqueName":"{wp}BratonBarrel","name":"Barrel"}}]"#
        );
        let index = ItemIndex::build(&[
            ("Warframes".into(), warframes.into_bytes()),
            ("Primary".into(), primary.into_bytes()),
            ("Components".into(), components.into_bytes()),
            ("Resources".into(), br#"[{"uniqueName":"/Lotus/Types/Items/MiscItems/OrokinCell","name":"Orokin Cell"}]"#.to_vec()),
        ]);
        // Two Frost sets (the built chassis doesn't count, its blueprints do), one Braton barrel
        // of the two a set needs, no Latron parts, and a non-prime part.
        let raw = format!(
            r#"{{"Recipes":[{{"ItemCount":2,"ItemType":"{wr}FrostPrimeBlueprint"}},{{"ItemCount":3,"ItemType":"{wr}FrostPrimeChassisBlueprint"}}],
                "MiscItems":[{{"ItemCount":1,"ItemType":"{wr}FrostPrimeChassisComponent"}},{{"ItemCount":1,"ItemType":"{wp}BratonPrimeBarrel"}},
                    {{"ItemCount":1,"ItemType":"{wp}BratonBarrel"}},{{"ItemCount":9,"ItemType":"/Lotus/Types/Items/MiscItems/OrokinCell"}}]}}"#
        );
        let data = build(&index, &parse(raw.as_bytes()).unwrap().values);
        let sets: Vec<(&str, i64)> = data.sets.iter().map(|s| (s.name.as_str(), s.count)).collect();
        assert_eq!(sets, [("Frost Prime", 2)], "only complete sets; Braton has one barrel of two");
        let frost = &data.sets[0];
        let parts: Vec<(&str, &str, i64, u32)> =
            frost.parts.iter().map(|p| (p.path.rsplit('/').next().unwrap(), p.name.as_str(), p.owned, p.needed)).collect();
        assert_eq!(
            parts,
            [
                ("FrostPrimeBlueprint", "Frost Prime Blueprint", 2, 1),
                ("FrostPrimeChassisBlueprint", "Frost Prime Chassis Blueprint", 3, 1),
            ],
            "traded as blueprints; no Orokin Cells"
        );
        assert_eq!((frost.image.as_deref(), frost.overlay.as_deref(), frost.vaulted), (Some(BLUEPRINT_IMAGE), Some("frost.png"), true));
    }

    #[test]
    fn natural_order() {
        assert_eq!(natural_cmp("Lith S2", "Lith S10"), Ordering::Less);
        assert_eq!(natural_cmp("Neo A1", "Neo A1"), Ordering::Equal);
        assert_eq!(reward_rarities(&[2.0, 25.33, 11.0, 25.33, 11.0, 25.33]), ["Rare", "Common", "Uncommon", "Common", "Uncommon", "Common"]);
    }
}
