//! Turns the raw inventory JSON into flat values that can be stored and compared.

use std::collections::BTreeMap;

use serde_json::Value;

/// Currencies kept as single values, stored under the section [`CURRENCY`].
pub const CURRENCIES: [(&str, &str); 5] = [
    ("RegularCredits", "Credits"),
    ("PremiumCredits", "Platinum"),
    ("PremiumCreditsFree", "Free platinum"),
    ("FusionPoints", "Endo"),
    ("PrimeTokens", "Ducats"),
];
pub const CURRENCY: &str = "Currency";
/// Mastery progress: one value per item, its affinity (XP).
pub const MASTERY: &str = "XPInfo";
/// Account-level values; `PlayerLevel` is the mastery rank.
pub const ACCOUNT: &str = "Account";
/// Warframes fed to the Helminth (`InfestedFoundry.ConsumedSuits`), value 1.
pub const SUBSUMED: &str = "Subsumed";
/// Archon shards installed per Warframe, value = number of shards.
pub const ARCHON_SHARDS: &str = "ArchonShards";
/// Parts of owned modular items (kitguns, Zaws, amps, MOAs, Hounds, K-Drives), value = copies.
pub const MODULAR_PARTS: &str = "ModularParts";
/// Weapons with Incarnon unlocked, from `EvolutionProgress`, value 1.
pub const INCARNON: &str = "EvolutionProgress";
/// Ranked mod and arcane copies (`Upgrades`), keyed `<path>#<rank>`, value = copies at that rank.
/// Rivens (`/Randomized/`) are left out until they get their own page.
pub const RANKS: &str = "UpgradeRanks";

/// Star chart nodes completed (`Missions`), keyed by node tag (e.g. `SolNode94`, `EarthToVenusJunction`),
/// value = completions.
pub const MISSIONS: &str = "Missions";
/// Nodes completed on The Steel Path (`Missions[].Tier == 1`), value 1.
pub const STEEL_PATH: &str = "SteelPath";
/// Railjack and Drifter intrinsic ranks (`PlayerSkills`, `LPS_*` keys), value = rank.
pub const INTRINSICS: &str = "Intrinsics";

/// Highest XP on any owned copy of an item, value = XP. Some items are only levelled here and never
/// appear in `XPInfo`: Predasites, Vulpaphylas, and modular companions (a hound's XP sits on the hound,
/// and its mastery belongs to its head part). For modular items the copy's XP is also recorded for
/// each of its `ModularParts`. Ungilded modular companions go to [`UNGILDED_XP`] instead.
pub const INSTANCE_XP: &str = "InstanceXP";

/// Modular companions (MOAs, Hounds, Predasites, Vulpaphylas) give mastery only once gilded: level
/// to 30, gild (back to rank 0), level to 30 again. An ungilded copy's XP lands here instead of
/// [`INSTANCE_XP`], value = best XP (≥ rank 30's means it's ready to gild). Verified on a real
/// account: an ungilded Sly Vulpaphyla at 1.46M XP has no `XPInfo` entry; a gilded Crescent has.
pub const UNGILDED_XP: &str = "UngildedXP";
/// Modular companions with a gilded copy owned, value 1.
pub const GILDED: &str = "Gilded";
/// The `Features` bit set on gilded items.
const FEATURE_GILDED: i64 = 8;

/// Bump when `flatten` starts tracking something new. The first capture after a bump is stored
/// as a fresh starting point, so newly tracked values don't show up as hundreds of "changes".
pub const FLATTEN_VERSION: i64 = 7;

/// Masterables that need gilding before they give mastery (see [`UNGILDED_XP`]): Predasites and
/// Vulpaphylas (the pet itself), MOAs and Hounds (their head).
pub fn needs_gilding(path: &str) -> bool {
    ["/Pets/CreaturePets/", "/Pets/MoaPets/MoaPetParts/", "/Pets/ZanukaPets/ZanukaPetParts/"]
        .iter()
        .any(|p| path.contains(p))
}

/// The XP that counts toward an item's mastery: its `XPInfo` entry or the best owned copy,
/// whichever is higher (see [`INSTANCE_XP`]).
pub fn mastery_xp(current: &Flat, path: &str) -> i64 {
    let get = |section: &str| current.get(&(section.to_string(), path.to_string())).copied().unwrap_or(0);
    get(MASTERY).max(get(INSTANCE_XP))
}

/// Builds the item key used in the [`RANKS`] section.
pub fn rank_key(path: &str, rank: u32) -> String {
    format!("{path}#{rank}")
}

/// Splits a [`RANKS`] key back into path and rank.
pub fn split_rank_key(key: &str) -> Option<(&str, u32)> {
    let (path, rank) = key.rsplit_once('#')?;
    Some((path, rank.parse().ok()?))
}

/// One value per (section, item path), for example `("MiscItems", "/Lotus/.../Ferrite") → 3462103`.
pub type Flat = BTreeMap<(String, String), i64>;

/// A parsed capture: which account it belongs to, its flat values, and its unveiled rivens.
#[derive(Debug, Clone)]
pub struct Inventory {
    pub account: String,
    pub values: Flat,
    /// Rivens don't fit the flat values (each has its own rolled stats), so they're kept whole.
    pub rivens: Vec<OwnedRiven>,
}

/// One unveiled riven (or one being unveiled) from `Upgrades`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedRiven {
    /// `ItemId.$oid`, unique per copy.
    pub id: String,
    /// e.g. `/Lotus/Upgrades/Mods/Randomized/LotusRifleRandomModRare`.
    pub item_type: String,
    /// The `UpgradeFingerprint` JSON string: weapon, rank, rolls (or the unveil challenge).
    pub fingerprint: String,
}

/// Rivens in `Upgrades` (veiled ones are plain counts in `RawUpgrades`).
fn rivens(doc: &Value) -> Vec<OwnedRiven> {
    doc.get("Upgrades")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|u| {
            let item_type = u.get("ItemType")?.as_str()?;
            if !item_type.contains("/Randomized/") {
                return None;
            }
            Some(OwnedRiven {
                id: u.pointer("/ItemId/$oid").and_then(Value::as_str).unwrap_or_default().to_string(),
                item_type: item_type.to_string(),
                fingerprint: u.get("UpgradeFingerprint").and_then(Value::as_str).unwrap_or("{}").to_string(),
            })
        })
        .collect()
}

/// Parses inventory JSON. Fails only if the bytes aren't a JSON object.
pub fn parse(bytes: &[u8]) -> Result<Inventory, String> {
    let doc: Value = serde_json::from_slice(bytes).map_err(|e| format!("inventory JSON: {e}"))?;
    if !doc.is_object() {
        return Err("inventory JSON is not an object".into());
    }
    Ok(Inventory { account: account_key(&doc), values: flatten(&doc), rivens: rivens(&doc) })
}

/// Identifies the Warframe account. The inventory has no plain account ID, but its creation
/// time (`Created`, milliseconds) is fixed per account, so it separates accounts on one PC.
pub fn account_key(doc: &Value) -> String {
    doc.pointer("/Created/$date/$numberLong")
        .and_then(Value::as_str)
        .map_or_else(|| "unknown".to_string(), |ms| format!("created-{ms}"))
}

/// Flattens every item list in the inventory.
///
/// Any top-level array of objects with an `ItemType` is an item list:
/// - entries with `ItemCount` (resources, blueprints, relics) → summed amounts
/// - `XPInfo` → affinity per item (mastery)
/// - everything else (frames, weapons, foundry jobs) → how many copies are owned
///
/// Lists that aren't items (missions, challenges) have no `ItemType` and are ignored.
///
/// Also recorded: mastery rank, subsumed Warframes, Archon shards per Warframe, and the parts of
/// owned modular items (an owned kitgun is stored as one generic item with a `ModularParts` list).
pub fn flatten(doc: &Value) -> Flat {
    let mut out = Flat::new();
    let Some(obj) = doc.as_object() else { return out };

    for (key, _) in CURRENCIES {
        if let Some(n) = obj.get(key).and_then(Value::as_i64) {
            add(&mut out, CURRENCY, key, n);
        }
    }
    if let Some(level) = obj.get("PlayerLevel").and_then(Value::as_i64) {
        add(&mut out, ACCOUNT, "PlayerLevel", level);
    }
    for suit in doc.pointer("/InfestedFoundry/ConsumedSuits").and_then(Value::as_array).into_iter().flatten() {
        if let Some(path) = suit.get("s").and_then(Value::as_str) {
            add(&mut out, SUBSUMED, path, 1);
        }
    }
    for mission in obj.get("Missions").and_then(Value::as_array).into_iter().flatten() {
        let Some(tag) = mission.get("Tag").and_then(Value::as_str) else { continue };
        let completes = mission.get("Completes").and_then(Value::as_i64).unwrap_or(0);
        if completes > 0 {
            add(&mut out, MISSIONS, tag, completes);
        }
        if mission.get("Tier").and_then(Value::as_i64) == Some(1) {
            add(&mut out, STEEL_PATH, tag, 1);
        }
    }
    for (skill, rank) in obj.get("PlayerSkills").and_then(Value::as_object).into_iter().flatten() {
        // LPS_* are intrinsic ranks; LPP_* are unspent points, which aren't progress.
        if let (true, Some(rank)) = (skill.starts_with("LPS_"), rank.as_i64()) {
            add(&mut out, INTRINSICS, skill, rank);
        }
    }

    for (section, value) in obj {
        let Some(list) = value.as_array() else { continue };
        for entry in list {
            let Some(path) = entry.get("ItemType").and_then(Value::as_str) else { continue };
            let shards = entry
                .get("ArchonCrystalUpgrades")
                .and_then(Value::as_array)
                .map_or(0, |s| s.iter().filter(|u| u.get("UpgradeType").is_some()).count());
            if shards > 0 {
                add(&mut out, ARCHON_SHARDS, path, shards as i64);
            }
            let copy_xp = (section != MASTERY).then(|| entry.get("XP").and_then(Value::as_i64)).flatten();
            let parts: Vec<&str> =
                entry.get("ModularParts").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
            let gilds = needs_gilding(path) || parts.iter().any(|p| needs_gilding(p));
            let gilded = entry.get("Features").and_then(Value::as_i64).unwrap_or(0) & FEATURE_GILDED != 0;
            let xp_section = if gilds && !gilded { UNGILDED_XP } else { INSTANCE_XP };
            for p in std::iter::once(path).chain(parts.iter().copied()) {
                if let Some(xp) = copy_xp {
                    keep_max(&mut out, xp_section, p, xp);
                }
                if gilds && gilded {
                    keep_max(&mut out, GILDED, p, 1);
                }
            }
            for p in &parts {
                add(&mut out, MODULAR_PARTS, p, 1);
            }
            if section == "Upgrades" && !path.contains("/Randomized/") {
                // The rank lives in a JSON string: {"lvl":5}. No `lvl` means rank 0.
                let rank = entry
                    .get("UpgradeFingerprint")
                    .and_then(Value::as_str)
                    .and_then(|fp| serde_json::from_str::<Value>(fp).ok())
                    .and_then(|fp| fp.get("lvl").and_then(Value::as_u64))
                    .unwrap_or(0) as u32;
                add(&mut out, RANKS, &rank_key(path, rank), 1);
            }
            if section == MASTERY {
                let xp = entry.get("XP").and_then(Value::as_i64).unwrap_or(0);
                let slot = out.entry((section.clone(), path.to_string())).or_insert(0);
                *slot = (*slot).max(xp);
            } else {
                let count = entry.get("ItemCount").and_then(Value::as_i64).unwrap_or(1);
                add(&mut out, section, path, count);
            }
        }
    }
    out
}

fn add(out: &mut Flat, section: &str, path: &str, n: i64) {
    *out.entry((section.to_string(), path.to_string())).or_insert(0) += n;
}

fn keep_max(out: &mut Flat, section: &str, path: &str, n: i64) {
    let slot = out.entry((section.to_string(), path.to_string())).or_insert(0);
    *slot = (*slot).max(n);
}

/// A difference between two captures for one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub section: String,
    pub item: String,
    pub before: i64,
    pub after: i64,
}

/// Compares two captures. Items missing on one side count as 0.
pub fn diff(before: &Flat, after: &Flat) -> Vec<Change> {
    let mut out = Vec::new();
    for (key, &new) in after {
        let old = before.get(key).copied().unwrap_or(0);
        if old != new {
            out.push(Change { section: key.0.clone(), item: key.1.clone(), before: old, after: new });
        }
    }
    for (key, &old) in before {
        if !after.contains_key(key) && old != 0 {
            out.push(Change { section: key.0.clone(), item: key.1.clone(), before: old, after: 0 });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "Created": {"$date": {"$numberLong": "1500000000000"}},
        "RegularCredits": 1234567, "PremiumCredits": 500, "FusionPoints": 10000,
        "MiscItems": [
            {"ItemCount": 250000, "ItemType": "/Lotus/Types/Items/MiscItems/Ferrite"},
            {"ItemCount": 3, "ItemType": "/Lotus/Types/Game/Projections/T1VoidProjectionAtlasPrimeABronze"}
        ],
        "Suits": [
            {"ItemType": "/Lotus/Powersuits/Volt/Volt", "XP": 100, "ItemId": {"$oid": "a"}},
            {"ItemType": "/Lotus/Powersuits/Volt/Volt", "XP": 5, "ItemId": {"$oid": "b"}}
        ],
        "XPInfo": [{"ItemType": "/Lotus/Powersuits/Volt/Volt", "XP": 100000}],
        "Missions": [{"Completes": 3, "Tag": "SolNode451"}]
    }"#;

    fn key(s: &str, p: &str) -> (String, String) {
        (s.to_string(), p.to_string())
    }

    #[test]
    fn flattens_sections() {
        let inv = parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(inv.account, "created-1500000000000");
        let v = &inv.values;
        assert_eq!(v[&key("Currency", "RegularCredits")], 1234567);
        assert_eq!(v[&key("MiscItems", "/Lotus/Types/Items/MiscItems/Ferrite")], 250000);
        assert_eq!(v[&key("Suits", "/Lotus/Powersuits/Volt/Volt")], 2, "two copies owned");
        assert_eq!(v[&key("XPInfo", "/Lotus/Powersuits/Volt/Volt")], 100000);
        // Missions have no ItemType, so they're only recorded through the star chart rule.
        assert_eq!(v[&key(MISSIONS, "SolNode451")], 3);
        assert_eq!(v.keys().filter(|(s, _)| s == MISSIONS).count(), 1);
    }

    #[test]
    fn records_rank_helminth_shards_modular_and_incarnon() {
        let doc = r#"{
            "PlayerLevel": 20,
            "InfestedFoundry": {"ConsumedSuits": [{"s": "/Lotus/Powersuits/Bard/Bard", "c": {}}]},
            "Suits": [{"ItemType": "/Lotus/Powersuits/Frost/FrostPrime", "ArchonCrystalUpgrades": [
                [], {"Color": "ACC_RED", "UpgradeType": "/A/Duration"}, {"Color": "ACC_YELLOW", "UpgradeType": "/A/Cast"}]}],
            "OperatorAmps": [{"ItemType": "/Lotus/Weapons/Amp/Generic", "ModularParts": ["/Amp/Prism", "/Amp/Scaffold"]}],
            "EvolutionProgress": [{"Progress": 0, "Rank": 4, "ItemType": "/Lotus/Weapons/Tenno/Rifle/SemiAutoRifle"}]
        }"#;
        let v = parse(doc.as_bytes()).unwrap().values;
        assert_eq!(v[&key(ACCOUNT, "PlayerLevel")], 20);
        assert_eq!(v[&key(SUBSUMED, "/Lotus/Powersuits/Bard/Bard")], 1);
        assert_eq!(v[&key(ARCHON_SHARDS, "/Lotus/Powersuits/Frost/FrostPrime")], 2, "empty slots don't count");
        assert_eq!(v[&key(MODULAR_PARTS, "/Amp/Prism")], 1);
        assert_eq!(v[&key(INCARNON, "/Lotus/Weapons/Tenno/Rifle/SemiAutoRifle")], 1);
    }

    #[test]
    fn copy_xp_counts_for_mastery() {
        // Real shapes: a gilded Predasite, an ungilded Vulpaphyla and two ungilded hounds with the
        // same head; none of them in XPInfo.
        let pets = "/Lotus/Types/Friendly/Pets";
        let doc = format!(
            r#"{{
            "KubrowPets": [
                {{"ItemType": "{pets}/CreaturePets/PharaohPredator", "XP": 2612965, "Features": 9,
                  "ModularParts": ["{pets}/CreaturePets/Parts/AntigenA", "{pets}/CreaturePets/Parts/MutagenA"]}},
                {{"ItemType": "{pets}/CreaturePets/VulpineInfested", "XP": 1463816,
                  "ModularParts": ["{pets}/CreaturePets/Parts/AntigenC"]}}],
            "MoaPets": [
                {{"ItemType": "{pets}/ZanukaPets/ZanukaPetB", "XP": 912851, "ModularParts": ["{pets}/ZanukaPets/ZanukaPetParts/HeadB", "/Parts/BodyA"]}},
                {{"ItemType": "{pets}/ZanukaPets/ZanukaPetB", "XP": 321698, "ModularParts": ["{pets}/ZanukaPets/ZanukaPetParts/HeadB", "/Parts/BodyC"]}}],
            "OperatorAmps": [{{"ItemType": "/Amp/Generic", "XP": 700000, "ModularParts": ["/Amp/Prism"]}}],
            "XPInfo": [{{"ItemType": "/Pets/Kavat", "XP": 500}}],
            "KubrowPets2": [{{"ItemType": "/Pets/Kavat", "XP": 900000}}]
        }}"#
        );
        let v = parse(doc.as_bytes()).unwrap().values;
        let pharaoh = format!("{pets}/CreaturePets/PharaohPredator");
        assert_eq!(mastery_xp(&v, &pharaoh), 2_612_965, "gilded, no XPInfo entry; the copy's XP counts");
        assert_eq!(v[&key(GILDED, &pharaoh)], 1);
        let vulpine = format!("{pets}/CreaturePets/VulpineInfested");
        assert_eq!(mastery_xp(&v, &vulpine), 0, "not gilded: no mastery yet");
        assert_eq!(v[&key(UNGILDED_XP, &vulpine)], 1_463_816);
        let head = format!("{pets}/ZanukaPets/ZanukaPetParts/HeadB");
        assert_eq!(mastery_xp(&v, &head), 0, "ungilded hounds give nothing");
        assert_eq!(v[&key(UNGILDED_XP, &head)], 912_851, "best ungilded copy");
        assert_eq!(mastery_xp(&v, "/Amp/Prism"), 700_000, "other modular items don't need gilding");
        assert_eq!(mastery_xp(&v, "/Pets/Kavat"), 900_000, "higher of XPInfo and copies");
        assert_eq!(mastery_xp(&v, "/Nothing"), 0);
    }

    #[test]
    fn records_missions_steel_path_and_intrinsics() {
        let doc = r#"{
            "Missions": [
                {"Completes": 43, "Tier": 1, "Tag": "SolNode27"},
                {"Completes": 2, "Tag": "EarthToVenusJunction"},
                {"Completes": 0, "Tag": "SolNode999"}
            ],
            "PlayerSkills": {"LPP_SPACE": 63830, "LPS_TACTICAL": 9, "LPS_DRIFT_COMBAT": 10}
        }"#;
        let v = parse(doc.as_bytes()).unwrap().values;
        assert_eq!(v[&key(MISSIONS, "SolNode27")], 43);
        assert_eq!(v[&key(STEEL_PATH, "SolNode27")], 1);
        assert_eq!(v[&key(MISSIONS, "EarthToVenusJunction")], 2);
        assert!(!v.contains_key(&key(STEEL_PATH, "EarthToVenusJunction")));
        assert!(!v.contains_key(&key(MISSIONS, "SolNode999")), "never completed");
        assert_eq!(v[&key(INTRINSICS, "LPS_TACTICAL")], 9);
        assert!(!v.contains_key(&key(INTRINSICS, "LPP_SPACE")), "unspent points aren't ranks");
    }

    #[test]
    fn records_mod_ranks_but_not_rivens() {
        let doc = r#"{"Upgrades": [
            {"UpgradeFingerprint": "{\"lvl\":5}", "ItemType": "/Lotus/Upgrades/Mods/Serration"},
            {"UpgradeFingerprint": "{\"lvl\":5}", "ItemType": "/Lotus/Upgrades/Mods/Serration"},
            {"UpgradeFingerprint": "{\"lvl\":2}", "ItemType": "/Lotus/Upgrades/Mods/Serration"},
            {"ItemType": "/Lotus/Upgrades/Mods/Vitality"},
            {"UpgradeFingerprint": "{\"lvl\":8,\"buffs\":[]}", "ItemType": "/Lotus/Upgrades/Mods/Randomized/LotusRifleRandomModRare"}
        ]}"#;
        let v = parse(doc.as_bytes()).unwrap().values;
        assert_eq!(v[&key(RANKS, "/Lotus/Upgrades/Mods/Serration#5")], 2);
        assert_eq!(v[&key(RANKS, "/Lotus/Upgrades/Mods/Serration#2")], 1);
        assert_eq!(v[&key(RANKS, "/Lotus/Upgrades/Mods/Vitality#0")], 1);
        assert!(!v.keys().any(|(s, i)| s == RANKS && i.contains("Randomized")));
        assert_eq!(split_rank_key("/Lotus/Upgrades/Mods/Serration#5"), Some(("/Lotus/Upgrades/Mods/Serration", 5)));
    }

    #[test]
    fn diff_finds_added_removed_and_changed() {
        let before = parse(SAMPLE.as_bytes()).unwrap().values;
        let mut after = before.clone();
        after.insert(key("MiscItems", "/Lotus/Types/Recipes/Weapons/WeaponParts/BratonPrimeBarrel"), 1);
        after.insert(key("MiscItems", "/Lotus/Types/Game/Projections/T1VoidProjectionAtlasPrimeABronze"), 56);
        after.remove(&key("Suits", "/Lotus/Powersuits/Volt/Volt"));
        let mut changes = diff(&before, &after);
        changes.sort_by(|a, b| a.item.cmp(&b.item));
        assert_eq!(changes.len(), 3);
        assert!(changes.contains(&Change {
            section: "MiscItems".into(),
            item: "/Lotus/Types/Recipes/Weapons/WeaponParts/BratonPrimeBarrel".into(),
            before: 0,
            after: 1
        }));
        assert!(changes.iter().any(|c| c.section == "Suits" && c.before == 2 && c.after == 0));
        assert!(diff(&before, &before).is_empty());
    }

    #[test]
    fn real_fixture_flattens() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/inventory_sample.json");
        let Ok(bytes) = std::fs::read(&path) else { return };
        let inv = parse(&bytes).unwrap();
        let misc = inv.values.keys().filter(|(s, _)| s == "MiscItems").count();
        assert_eq!(misc, 822);
        assert!(inv.values.contains_key(&key(CURRENCY, "RegularCredits")));
    }
}
