//! The Foundry page: every item that gives mastery, with the account's progress on each.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::pipeline::Shared;
use crate::translate::wfcd::BLUEPRINT_IMAGE;
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
pub const PART_SECTIONS: [&str; 16] = [
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
    /// The part built plus its unbuilt blueprints (a Chassis or a Chassis Blueprint both count).
    pub owned: i64,
    /// How many of `owned` are unbuilt blueprints.
    pub blueprints: i64,
}

/// The blueprint of a built part: a warframe's parts are listed as the built component
/// (`…ChassisComponent`), which is crafted from `…ChassisBlueprint`. `None` for anything else.
pub fn blueprint_of(path: &str) -> Option<String> {
    path.strip_suffix("Component").map(|p| format!("{p}Blueprint"))
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
    /// Incarnon unlocked on the account's copy.
    pub incarnon: bool,
    /// The weapon has an Incarnon form: a Genesis exists for it or one of its variants, or it's
    /// an Incarnon weapon of its own.
    pub incarnon_capable: bool,
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
pub fn totals(current: &Flat, sections: &[&str]) -> HashMap<String, i64> {
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
            let blueprint = blueprint_of(&p.path);
            let built = parts_owned.get(&p.path).copied().unwrap_or(0);
            let blueprints = blueprint.as_ref().and_then(|bp| parts_owned.get(bp).copied()).unwrap_or(0);
            let own_image = part.and_then(|i| i.image.clone());
            // A part that also comes as a blueprint (its count includes them) is drawn like one:
            // the blueprint picture behind, the part's own picture over it.
            let (image, overlay) = if blueprint.is_some() {
                (Some(BLUEPRINT_IMAGE.to_string()), own_image)
            } else {
                (own_image, index.blueprint_overlay(&p.path))
            };
            PartView {
                path: p.path.clone(),
                name: index.display_name(&p.path),
                image,
                overlay,
                needed: p.count,
                owned: built + blueprints,
                blueprints,
            }
        })
        .collect();
    let name = index.display_name(&m.path);
    FoundryItem {
        incarnon_capable: matches!(m.tab, Tab::Primary | Tab::Secondary | Tab::Melee) && index.incarnon_capable(&name),
        path: m.path.clone(),
        name,
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
        // The parts themselves: a part's blueprint still has to be crafted first.
        ready_to_build: !parts.is_empty() && parts.iter().all(|p| p.owned - p.blueprints >= i64::from(p.needed)),
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

    /// A warframe part counts as owned built (`…Component`) or as its unbuilt blueprint
    /// (`…Blueprint`); the set's parts and counts stay the same, and a blueprint still to craft
    /// doesn't make the item ready to build.
    #[test]
    fn part_blueprints_count_as_owned() {
        let wr = "/Lotus/Types/Recipes/WarframeRecipes/Nova";
        let index = ItemIndex::build(&[(
            "Warframes".into(),
            format!(
                r#"[{{"uniqueName":"/Nova","name":"Nova","masterable":true,
                "components":[{{"uniqueName":"{wr}Blueprint","itemCount":1}},{{"uniqueName":"{wr}ChassisComponent","itemCount":1}},
                {{"uniqueName":"{wr}HelmetComponent","itemCount":1}}]}}]"#
            )
            .into_bytes(),
        )]);
        let raw = format!(
            r#"{{"Recipes": [{{"ItemType": "{wr}Blueprint", "ItemCount": 1}}, {{"ItemType": "{wr}ChassisBlueprint", "ItemCount": 2}}],
            "MiscItems": [{{"ItemType": "{wr}ChassisComponent", "ItemCount": 1}}, {{"ItemType": "{wr}HelmetComponent", "ItemCount": 1}}]}}"#
        );
        let current = parse(raw.as_bytes()).unwrap().values;
        let items = build(&index, &current, &HashSet::new());
        let nova = items.iter().find(|i| i.name == "Nova").unwrap();
        let counts: Vec<(i64, i64)> = nova.parts.iter().map(|p| (p.owned, p.blueprints)).collect();
        assert_eq!(nova.parts.len(), 3, "same parts per set");
        assert_eq!(counts, [(1, 0), (3, 2), (1, 0)], "Chassis: 1 built + 2 blueprints");
        let chassis = &nova.parts[1];
        assert_eq!(chassis.image.as_deref(), Some(BLUEPRINT_IMAGE), "drawn as a blueprint");
        assert!(nova.ready_to_build);

        // Only a Chassis Blueprint: owned, but not ready to build.
        let raw = format!(
            r#"{{"Recipes": [{{"ItemType": "{wr}Blueprint", "ItemCount": 1}}, {{"ItemType": "{wr}ChassisBlueprint", "ItemCount": 1}}],
            "MiscItems": [{{"ItemType": "{wr}HelmetComponent", "ItemCount": 1}}]}}"#
        );
        let current = parse(raw.as_bytes()).unwrap().values;
        let items = build(&index, &current, &HashSet::new());
        let nova = items.iter().find(|i| i.name == "Nova").unwrap();
        assert_eq!((nova.parts[1].owned, nova.parts[1].blueprints), (1, 1));
        assert!(!nova.ready_to_build);
    }
}

