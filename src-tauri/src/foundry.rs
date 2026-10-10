//! The Foundry page: every item that gives mastery, with the account's progress on each.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::pipeline::Shared;
use crate::translate::{
    mastery_xp, Flat, ItemIndex, Masterable, Tab, ACCOUNT, ARCHON_SHARDS, INCARNON, MODULAR_PARTS, SUBSUMED,
};

/// Inventory sections that hold owned frames, weapons, companions and vehicles.
const OWNED_SECTIONS: [&str; 15] = [
    "Suits",
    "LongGuns",
    "Pistols",
    "Melee",
    "SpaceSuits",
    "SpaceGuns",
    "SpaceMelee",
    "Sentinels",
    "SentinelWeapons",
    "KubrowPets",
    "MoaPets",
    "MechSuits",
    "OperatorAmps",
    "Hoverboards",
    MODULAR_PARTS,
];

/// Sections that hold build parts: blueprints, prime parts, resources, plus owned items that are
/// themselves parts of something else (a Braton needed to build another weapon).
const PART_SECTIONS: [&str; 16] = [
    "MiscItems",
    "Recipes",
    "Suits",
    "LongGuns",
    "Pistols",
    "Melee",
    "SpaceSuits",
    "SpaceGuns",
    "SpaceMelee",
    "Sentinels",
    "SentinelWeapons",
    "KubrowPets",
    "MoaPets",
    "MechSuits",
    "OperatorAmps",
    "Hoverboards",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartView {
    pub path: String,
    pub name: String,
    pub image: Option<String>,
    /// For blueprints: the image of what it builds, drawn over the blueprint picture.
    pub overlay: Option<String>,
    pub needed: u32,
    pub owned: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundryItem {
    pub path: String,
    pub name: String,
    pub tab: Tab,
    pub image: Option<String>,
    pub is_prime: bool,
    pub vaulted: bool,
    pub mastery_req: u32,
    pub max_rank: u32,
    pub xp: i64,
    pub rank: u32,
    pub mastered: bool,
    pub owned: i64,
    pub subsumed: bool,
    pub archon_shards: i64,
    pub incarnon: bool,
    pub used_for_crafting: bool,
    pub favorite: bool,
    pub ready_to_build: bool,
    pub parts: Vec<PartView>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundryData {
    pub account: Option<String>,
    pub mastery_rank: Option<i64>,
    pub items: Vec<FoundryItem>,
}

/// Sums the values of `sections` per item path.
fn totals(current: &Flat, sections: &[&str]) -> HashMap<String, i64> {
    let wanted: HashSet<&str> = sections.iter().copied().collect();
    let mut out = HashMap::new();
    for ((section, item), value) in current {
        if wanted.contains(section.as_str()) {
            *out.entry(item.clone()).or_insert(0) += value;
        }
    }
    out
}

fn has(current: &Flat, section: &str, item: &str) -> i64 {
    current.get(&(section.to_string(), item.to_string())).copied().unwrap_or(0)
}

/// Builds the Foundry list. Pure, so it can be tested without Tauri.
pub fn build(index: &ItemIndex, current: &Flat, favorites: &HashSet<String>) -> Vec<FoundryItem> {
    let owned = totals(current, &OWNED_SECTIONS);
    let parts_owned = totals(current, &PART_SECTIONS);
    let mut items: Vec<FoundryItem> = index
        .masterables()
        .iter()
        .map(|m| item(index, current, &owned, &parts_owned, favorites, m))
        .collect();
    items.sort_by_key(|i| i.name.to_lowercase());
    items
}

fn item(
    index: &ItemIndex,
    current: &Flat,
    owned: &HashMap<String, i64>,
    parts_owned: &HashMap<String, i64>,
    favorites: &HashSet<String>,
    m: &Masterable,
) -> FoundryItem {
    let info = index.get(&m.path);
    let xp = mastery_xp(current, &m.path);
    let rank = m.curve.rank_for_xp(xp, m.max_rank);
    let parts: Vec<PartView> = m
        .parts
        .iter()
        .map(|p| {
            let part = index.get(&p.path);
            PartView {
                path: p.path.clone(),
                name: index.display_name(&p.path),
                image: part.and_then(|i| i.image.clone()),
                overlay: index.blueprint_overlay(&p.path),
                needed: p.count,
                owned: parts_owned.get(&p.path).copied().unwrap_or(0),
            }
        })
        .collect();
    FoundryItem {
        path: m.path.clone(),
        name: index.display_name(&m.path),
        tab: m.tab,
        image: info.and_then(|i| i.image.clone()),
        is_prime: m.is_prime,
        vaulted: m.vaulted,
        mastery_req: m.mastery_req,
        max_rank: m.max_rank,
        xp,
        rank,
        mastered: xp >= m.curve.xp_for_rank(m.max_rank),
        owned: owned.get(&m.path).copied().unwrap_or(0),
        subsumed: has(current, SUBSUMED, &m.path) > 0,
        archon_shards: has(current, ARCHON_SHARDS, &m.path),
        incarnon: has(current, INCARNON, &m.path) > 0,
        used_for_crafting: index.used_in_crafting(&m.path),
        favorite: favorites.contains(&m.path),
        ready_to_build: !parts.is_empty() && parts.iter().all(|p| p.owned >= i64::from(p.needed)),
        parts,
    }
}

#[tauri::command]
pub fn get_foundry(shared: State<'_, Arc<Shared>>) -> Result<FoundryData, String> {
    let index = shared.items.read().unwrap().clone();
    let guard = shared.store.lock().unwrap();
    let store = guard.as_ref();
    let account = store.and_then(|s| s.latest_account().ok().flatten());
    let (current, favorites) = match (store, &account) {
        (Some(s), Some(a)) => (
            s.current(a).map_err(|e| e.to_string())?,
            s.favorites(a).map_err(|e| e.to_string())?,
        ),
        _ => (Flat::new(), HashSet::new()),
    };
    let mastery_rank = current.get(&(ACCOUNT.to_string(), "PlayerLevel".to_string())).copied();
    Ok(FoundryData { items: build(&index, &current, &favorites), account, mastery_rank })
}

#[tauri::command]
pub fn set_favorite(shared: State<'_, Arc<Shared>>, item: String, on: bool) -> Result<(), String> {
    let guard = shared.store.lock().unwrap();
    let store = guard.as_ref().ok_or("database unavailable")?;
    let account = store.latest_account().map_err(|e| e.to_string())?.ok_or("no account captured yet")?;
    store.set_favorite(&account, &item, on).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::parse;

    fn index() -> ItemIndex {
        ItemIndex::build(&[
            (
                "Warframes".into(),
                br#"[{"uniqueName":"/Frost","name":"Frost Prime","masterable":true,"isPrime":true,"vaulted":true,
                    "imageName":"frost.png","components":[{"uniqueName":"/Lotus/Types/Recipes/FrostBP","itemCount":1},{"uniqueName":"/Cell","itemCount":2}]},
                    {"uniqueName":"/Bard","name":"Octavia","masterable":true}]"#
                    .to_vec(),
            ),
            (
                "Primary".into(),
                br#"[{"uniqueName":"/Braton","name":"Braton","masterable":true,"masteryReq":30}]"#.to_vec(),
            ),
            (
                "Components".into(),
                br#"[{"uniqueName":"/Lotus/Types/Recipes/FrostBP","name":"Blueprint"},{"uniqueName":"/Cell","name":"Orokin Cell"}]"#.to_vec(),
            ),
        ])
    }

    #[test]
    fn builds_progress_parts_and_flags() {
        let raw = r#"{"PlayerLevel": 20,
            "InfestedFoundry": {"ConsumedSuits": [{"s": "/Bard"}]},
            "Suits": [{"ItemType": "/Frost", "ArchonCrystalUpgrades": [{"UpgradeType": "/x"}]}],
            "Recipes": [{"ItemType": "/Lotus/Types/Recipes/FrostBP", "ItemCount": 1}],
            "MiscItems": [{"ItemCount": 1, "ItemType": "/Cell"}],
            "XPInfo": [{"ItemType": "/Frost", "XP": 900000}, {"ItemType": "/Braton", "XP": 125000}]}"#;
        let current = parse(raw.as_bytes()).unwrap().values;
        let favorites: HashSet<String> = ["/Braton".to_string()].into();
        let items = build(&index(), &current, &favorites);

        assert_eq!(
            items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
            ["Braton", "Frost Prime", "Octavia", "Plexus"],
            "Plexus is always added alongside real data"
        );
        let frost = &items[1];
        assert!(frost.mastered && frost.is_prime && frost.vaulted);
        assert_eq!((frost.rank, frost.owned, frost.archon_shards), (30, 1, 1));
        assert_eq!(frost.parts[0].name, "Frost Prime Blueprint");
        assert_eq!((frost.parts[1].needed, frost.parts[1].owned), (2, 1));
        assert!(!frost.ready_to_build, "only 1 of 2 Orokin Cells");

        let braton = &items[0];
        assert_eq!((braton.rank, braton.mastered, braton.owned), (15, false, 0));
        assert!(braton.favorite);
        assert_eq!(braton.mastery_req, 30);

        let octavia = &items[2];
        assert!(octavia.subsumed && !octavia.mastered);
    }
}
