//! The Mastery Helper page: total mastery XP, rank progress, summary boxes, and the
//! "best ways to level up mastery" lists (Easy, From relics, With platinum).
//!
//! Mastery sources (Warframe wiki, Mastery Rank):
//! - items: 200 per rank for Warframe-like items, 100 per rank for weapons (up to the item's max rank)
//! - star chart nodes: a per-node amount for the first completion, and again on The Steel Path
//! - junctions: 1,000 each, and again on The Steel Path
//! - Railjack and Drifter intrinsics: 1,500 per rank
//!
//! Rank n starts at 2,500 × n² XP up to rank 30; Legendary rank L at 2,250,000 + 147,500 × L.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::foundry::{self, FoundryItem, PartView};
use crate::pipeline::Shared;
use crate::translate::nodes::NodeData;
use crate::translate::wfcd::split_relic_name;
use crate::translate::{Flat, ItemIndex, Tab, XpCurve, ACCOUNT, INTRINSICS, MASTERY, MISSIONS, STEEL_PATH};

pub const JUNCTION_XP: i64 = 1000;
pub const INTRINSIC_XP: i64 = 1500;
const MAX_INTRINSIC_RANK: i64 = 10;
const RAILJACK_SKILLS: [&str; 5] = ["LPS_TACTICAL", "LPS_PILOTING", "LPS_GUNNERY", "LPS_ENGINEERING", "LPS_COMMAND"];
const DRIFTER_SKILLS: [&str; 4] = ["LPS_DRIFT_RIDING", "LPS_DRIFT_COMBAT", "LPS_DRIFT_OPPORTUNITY", "LPS_DRIFT_ENDURANCE"];
/// Every planet junction; used if the node data doesn't list them.
const DEFAULT_JUNCTIONS: i64 = 13;

/// Total mastery XP needed to reach a rank.
pub fn rank_threshold(rank: u32) -> i64 {
    let r = i64::from(rank);
    if rank <= 30 { 2500 * r * r } else { 2_250_000 + 147_500 * (r - 30) }
}

#[derive(Debug, Default, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Breakdown {
    pub items: i64,
    pub nodes: i64,
    pub steel_path_nodes: i64,
    pub junctions: i64,
    pub intrinsics: i64,
}

impl Breakdown {
    pub fn total(&self) -> i64 {
        self.items + self.nodes + self.steel_path_nodes + self.junctions + self.intrinsics
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub label: String,
    pub done: i64,
    pub total: i64,
}

fn progress(label: &str, done: i64, total: i64) -> Progress {
    Progress { label: label.to_string(), done, total }
}

/// One suggestion in the "best ways to level up" list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub path: String,
    pub name: String,
    pub image: Option<String>,
    pub tab: Tab,
    /// Account mastery XP still to gain from this item.
    pub xp_gain: i64,
    pub rank: u32,
    pub max_rank: u32,
    pub owned: bool,
    /// Its main blueprint is in the foundry right now.
    pub building: bool,
    pub favorite: bool,
    pub parts: Vec<PartView>,
    /// From relics: relic-dropped parts owned, out of how many the set has.
    pub relic_parts_owned: u32,
    pub relic_parts_total: u32,
    /// From relics: owned relics (name without refinement) that drop a still-missing part.
    pub relic_sources: Vec<String>,
    /// From relics: every relic part is already owned.
    pub ready: bool,
    /// With platinum: parts to buy and how many of each.
    pub buy: Vec<BuyPart>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuyPart {
    pub path: String,
    pub count: i64,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MasteryData {
    pub has_data: bool,
    pub rank: u32,
    pub total_xp: i64,
    pub rank_start: i64,
    pub next_rank: i64,
    /// Enough XP for the next rank: the rank test is available.
    pub ready_for_test: bool,
    pub breakdown: Breakdown,
    pub content: Vec<Progress>,
    pub star_chart: Vec<Progress>,
    pub intrinsics: Vec<Progress>,
    pub easy: Vec<Suggestion>,
    pub relics: Vec<Suggestion>,
    pub platinum: Vec<Suggestion>,
}

fn has(current: &Flat, section: &str, item: &str) -> i64 {
    current.get(&(section.to_string(), item.to_string())).copied().unwrap_or(0)
}

fn section<'a>(current: &'a Flat, name: &'a str) -> impl Iterator<Item = (&'a str, i64)> + 'a {
    current.iter().filter(move |((s, _), _)| s == name).map(|((_, item), v)| (item.as_str(), *v))
}

/// Builds the whole page. Pure, so it can be tested without Tauri.
pub fn build(index: &ItemIndex, nodes: &NodeData, current: &Flat, favorites: &HashSet<String>) -> MasteryData {
    let curves: HashMap<&str, (XpCurve, Tab)> =
        index.masterables().iter().map(|m| (m.path.as_str(), (m.curve, m.tab))).collect();

    // --- Total XP ---
    let mut b = Breakdown::default();
    // The total uses XPInfo only: that matched the AlecaFrame reference to within the account's
    // progress since the screenshot. Companions levelled only on the copy (Predasites, Vulpaphylas,
    // hound heads) show as mastered via `mastery_xp`, but whether the game adds them to the total
    // is unconfirmed (it would add ~19,900 XP on the test account) — see milestone 11.
    for m in index.masterables() {
        let rank = m.curve.rank_for_xp(has(current, MASTERY, &m.path), m.max_rank);
        b.items += i64::from(rank) * m.curve.mastery_per_rank();
    }
    // The wiki lists junctions as nodes worth 1,000 XP; they're counted separately below, so
    // keep them out of the node list (counting both double-counted 22,000 XP on a real account).
    let counted: Vec<_> = nodes.counted().filter(|n| !is_junction(&n.tag)).collect();
    for n in &counted {
        if has(current, MISSIONS, &n.tag) > 0 {
            b.nodes += i64::from(n.xp);
        }
        if has(current, STEEL_PATH, &n.tag) > 0 {
            b.steel_path_nodes += i64::from(n.xp);
        }
    }
    let junctions_done = section(current, MISSIONS).filter(|(t, _)| is_junction(t)).count() as i64;
    let sp_junctions_done = section(current, STEEL_PATH).filter(|(t, _)| is_junction(t)).count() as i64;
    b.junctions = JUNCTION_XP * (junctions_done + sp_junctions_done);
    let intrinsic = |skills: &[&str]| -> i64 {
        skills.iter().map(|s| has(current, INTRINSICS, s).clamp(0, MAX_INTRINSIC_RANK)).sum()
    };
    let (railjack, drifter) = (intrinsic(&RAILJACK_SKILLS), intrinsic(&DRIFTER_SKILLS));
    b.intrinsics = INTRINSIC_XP * (railjack + drifter);

    let total_xp = b.total();
    let rank = has(current, ACCOUNT, "PlayerLevel").max(0) as u32;
    let (rank_start, next_rank) = (rank_threshold(rank), rank_threshold(rank + 1));

    // --- Summary boxes ---
    let items = foundry::build(index, current, favorites);
    let group = |f: &dyn Fn(Tab, XpCurve) -> bool| -> (i64, i64) {
        let list: Vec<&FoundryItem> = items
            .iter()
            .filter(|i| curves.get(i.path.as_str()).is_some_and(|&(c, t)| f(t, c)))
            .collect();
        (list.iter().filter(|i| i.mastered).count() as i64, list.len() as i64)
    };
    let frames = group(&|t, c| t == Tab::Warframe || (t == Tab::Arch && c == XpCurve::Frame));
    let weapons = group(&|t, c| c == XpCurve::Weapon && t != Tab::Companion);
    let companions = group(&|t, _| t == Tab::Companion);
    let kdrives = group(&|t, c| t == Tab::Modular && c == XpCurve::Frame);
    let mut content = vec![
        progress("Warframes / Archwings", frames.0, frames.1),
        progress("Weapons", weapons.0, weapons.1),
        progress("Companions", companions.0, companions.1),
    ];
    if kdrives.1 > 0 {
        content.push(progress("K-Drives", kdrives.0, kdrives.1));
    }

    let junction_total = match nodes.counted().filter(|n| is_junction(&n.tag)).count() as i64 {
        0 => DEFAULT_JUNCTIONS,
        listed => listed,
    };
    let star_chart = vec![
        progress("Normal", counted.iter().filter(|n| has(current, MISSIONS, &n.tag) > 0).count() as i64, counted.len() as i64),
        progress("Junctions", junctions_done, junction_total),
        progress("Steel Path", counted.iter().filter(|n| has(current, STEEL_PATH, &n.tag) > 0).count() as i64, counted.len() as i64),
        progress("Steel Path junctions", sp_junctions_done, junction_total),
    ];
    let intrinsics = vec![
        progress("Railjack", railjack, MAX_INTRINSIC_RANK * RAILJACK_SKILLS.len() as i64),
        progress("Duviri", drifter, MAX_INTRINSIC_RANK * DRIFTER_SKILLS.len() as i64),
    ];

    // --- Best ways to level up ---
    // An item whose main blueprint is in the foundry will be owned soon; it belongs in Easy, not in
    // the relic or platinum lists. (Matches AlecaFrame, which marks these "Owned".)
    let building = |i: &FoundryItem| {
        i.parts.iter().any(|p| p.path.ends_with("Blueprint") && has(current, "PendingRecipes", &p.path) > 0)
    };
    let suggestion = |i: &FoundryItem| -> Suggestion {
        let per = curves.get(i.path.as_str()).map_or(100, |&(c, _)| c.mastery_per_rank());
        Suggestion {
            path: i.path.clone(),
            name: i.name.clone(),
            image: i.image.clone(),
            tab: i.tab,
            xp_gain: i64::from(i.max_rank.saturating_sub(i.rank)) * per,
            rank: i.rank,
            max_rank: i.max_rank,
            owned: i.owned > 0,
            building: building(i),
            favorite: i.favorite,
            parts: i.parts.clone(),
            relic_parts_owned: 0,
            relic_parts_total: 0,
            relic_sources: Vec::new(),
            ready: false,
            buy: Vec::new(),
        }
    };
    let to_get = |i: &&FoundryItem| i.owned == 0 && !i.mastered && !building(i);

    // Easy: owned or being built, not mastered.
    let mut easy: Vec<Suggestion> =
        items.iter().filter(|i| (i.owned > 0 || building(i)) && !i.mastered).map(&suggestion).collect();
    easy.sort_by(|a, b| b.xp_gain.cmp(&a.xp_gain).then_with(|| a.name.cmp(&b.name)));

    // From relics: sets closest to being built, where every relic part still missing drops from a
    // relic the account holds. Fewest missing parts first; ready-to-build sets at the top.
    let drops = relic_drops(index);
    let owned_relics = owned_relic_names(index, current);
    // Relics drop Warframe *part blueprints* (`…ChassisBlueprint`) while the recipe lists the built
    // part (`…ChassisComponent`). Map each part to what relics actually drop, and count owning
    // either the built part or its blueprint.
    let relic_key = |path: &str| -> Option<String> {
        if drops.contains_key(path) {
            return Some(path.to_string());
        }
        let blueprint = format!("{}Blueprint", path.strip_suffix("Component")?);
        drops.contains_key(blueprint.as_str()).then_some(blueprint)
    };
    let mut relics: Vec<Suggestion> = items
        .iter()
        .filter(to_get)
        // Prime sets only: WFCD's Requiem relic data also lists a few non-prime weapons (Aegrit,
        // Slaytra, Tatsu), which aren't sets you build from relics.
        .filter(|i| i.is_prime)
        .filter_map(|i| {
            // (part, what relics drop for it, how many owned counting its blueprint)
            let relic_parts: Vec<(&PartView, String, i64)> = i
                .parts
                .iter()
                .filter_map(|p| {
                    let key = relic_key(&p.path)?;
                    let blueprints = if key == p.path { 0 } else { has(current, "Recipes", &key) };
                    Some((p, key, p.owned + blueprints))
                })
                .collect();
            if relic_parts.is_empty() {
                return None;
            }
            let missing: Vec<&(&PartView, String, i64)> =
                relic_parts.iter().filter(|(p, _, owned)| *owned < i64::from(p.needed)).collect();
            let mut sources: Vec<String> = Vec::new();
            for (_, key, _) in &missing {
                let from: Vec<&str> =
                    drops[key.as_str()].iter().map(|(r, _)| *r).filter(|r| owned_relics.contains(r)).collect();
                if from.is_empty() {
                    return None; // a missing part no owned relic drops
                }
                sources.extend(from.iter().map(|r| r.to_string()));
            }
            sources.sort();
            sources.dedup();
            // Show part blueprints on the card too: a Chassis icon counts owned Chassis Blueprints,
            // so the icons agree with the "owned/total relic parts" count.
            let owned_with_blueprints: HashMap<&str, i64> =
                relic_parts.iter().map(|(p, _, owned)| (p.path.as_str(), *owned)).collect();
            let parts = i
                .parts
                .iter()
                .map(|p| PartView { owned: owned_with_blueprints.get(p.path.as_str()).copied().unwrap_or(p.owned), ..p.clone() })
                .collect();
            Some(Suggestion {
                relic_parts_owned: (relic_parts.len() - missing.len()) as u32,
                relic_parts_total: relic_parts.len() as u32,
                relic_sources: sources,
                ready: missing.is_empty(),
                parts,
                ..suggestion(i)
            })
        })
        .collect();
    relics.sort_by(|a, b| {
        let missing = |s: &Suggestion| s.relic_parts_total - s.relic_parts_owned;
        missing(a)
            .cmp(&missing(b))
            .then_with(|| (b.relic_parts_owned * a.relic_parts_total).cmp(&(a.relic_parts_owned * b.relic_parts_total)))
            .then_with(|| b.xp_gain.cmp(&a.xp_gain))
            .then_with(|| a.name.cmp(&b.name))
    });

    // With platinum: not owned, not mastered; the parts still missing, except farmable resources.
    let platinum: Vec<Suggestion> = items
        .iter()
        .filter(to_get)
        .filter(|i| !i.parts.is_empty())
        .filter_map(|i| {
            let buy: Vec<BuyPart> = i
                .parts
                .iter()
                .filter(|p| p.owned < i64::from(p.needed) && !is_resource(index, &p.path))
                .map(|p| BuyPart { path: p.path.clone(), count: i64::from(p.needed) - p.owned })
                .collect();
            (!buy.is_empty()).then(|| Suggestion { buy, ..suggestion(i) })
        })
        .collect();

    MasteryData {
        has_data: !current.is_empty(),
        rank,
        total_xp,
        rank_start,
        next_rank,
        ready_for_test: total_xp >= next_rank,
        breakdown: b,
        content,
        star_chart,
        intrinsics,
        easy,
        relics,
        platinum,
    }
}

/// Junction tags look like "EarthToVenusJunction".
fn is_junction(tag: &str) -> bool {
    tag.ends_with("Junction")
}

/// Farmable resources (Orokin Cells, Neurodes, …) aren't bought on warframe.market.
fn is_resource(index: &ItemIndex, path: &str) -> bool {
    index.get(path).is_some_and(|i| matches!(i.file.as_str(), "Resources" | "Misc" | "Fish" | "Gear"))
}

/// Part path → the relics (name without refinement) that drop it, with the Intact chance (%).
fn relic_drops(index: &ItemIndex) -> HashMap<&str, Vec<(&str, f64)>> {
    let mut out: HashMap<&str, Vec<(&str, f64)>> = HashMap::new();
    for (relic, rewards) in index.relics() {
        for r in rewards {
            out.entry(r.path.as_str()).or_default().push((relic, r.chance));
        }
    }
    out
}

/// Relics the account holds (any refinement), by name without refinement.
fn owned_relic_names<'a>(index: &'a ItemIndex, current: &'a Flat) -> HashSet<&'a str> {
    section(current, "MiscItems")
        .filter(|(_, n)| *n > 0)
        .filter_map(|(path, _)| index.get(path).filter(|i| i.file == "Relics"))
        .map(|i| split_relic_name(&i.name).0)
        .collect()
}

#[tauri::command]
pub fn get_mastery(shared: State<'_, Arc<Shared>>) -> Result<MasteryData, String> {
    let index = shared.items.read().unwrap().clone();
    let nodes = shared.nodes.read().unwrap().clone();
    let guard = shared.store.lock().unwrap();
    let Some(store) = guard.as_ref() else { return Ok(MasteryData::default()) };
    let Some(account) = store.latest_account().map_err(|e| e.to_string())? else {
        return Ok(MasteryData::default());
    };
    let current = store.current(&account).map_err(|e| e.to_string())?;
    let favorites = store.favorites(&account).map_err(|e| e.to_string())?;
    Ok(build(&index, &nodes, &current, &favorites))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::parse;

    #[test]
    fn rank_thresholds() {
        assert_eq!(rank_threshold(27), 1_822_500);
        assert_eq!(rank_threshold(28) - rank_threshold(27), 137_500, "matches the reference screenshot");
        assert_eq!(rank_threshold(30), 2_250_000);
        assert_eq!(rank_threshold(31), 2_397_500, "Legendary 1");
    }

    fn index() -> ItemIndex {
        ItemIndex::build(&[
            (
                "Warframes".into(),
                br#"[{"uniqueName":"/Frost","name":"Frost Prime","masterable":true,"isPrime":true,
                    "components":[{"uniqueName":"/Lotus/Types/Recipes/FrostPrimeBlueprint","itemCount":1},
                                  {"uniqueName":"/Lotus/Types/Recipes/FrostPrimeChassis","itemCount":1},
                                  {"uniqueName":"/Cell","itemCount":2}]},
                    {"uniqueName":"/Volt","name":"Volt","masterable":true}]"#
                    .to_vec(),
            ),
            ("Primary".into(), br#"[{"uniqueName":"/Braton","name":"Braton","masterable":true}]"#.to_vec()),
            (
                "Components".into(),
                br#"[{"uniqueName":"/Lotus/Types/Recipes/FrostPrimeBlueprint","name":"Blueprint"},
                    {"uniqueName":"/Lotus/Types/Recipes/FrostPrimeChassis","name":"Chassis"}]"#
                    .to_vec(),
            ),
            ("Misc".into(), br#"[{"uniqueName":"/Cell","name":"Orokin Cell"}]"#.to_vec()),
            (
                "Relics".into(),
                br#"[{"uniqueName":"/R/A","name":"Lith F1 Intact","rewards":[
                        {"chance":25.33,"item":{"uniqueName":"/Lotus/Types/Recipes/FrostPrimeBlueprint","name":"Frost Prime Blueprint"}},
                        {"chance":11,"item":{"uniqueName":"/Lotus/Types/Recipes/FrostPrimeChassis","name":"Frost Prime Chassis"}}]}]"#
                    .to_vec(),
            ),
        ])
    }

    #[test]
    fn totals_boxes_and_lists() {
        let nodes = NodeData::from_lua(
            r#"{ Name = "A", Planet = "Mercury", MasteryExp = 24, InternalName = "SolNode1", IsTracked = true },
               { Name = "B", Planet = "Mercury", MasteryExp = 18, InternalName = "SolNode2", IsTracked = true },
               { Name = "Venus Junction", Planet = "Earth", MasteryExp = 1000, InternalName = "EarthToVenusJunction", IsTracked = true },
               { Name = "Relay", Planet = "Mercury", MasteryExp = 0, InternalName = "MercuryHUB" },"#,
        );
        let raw = r#"{
            "PlayerLevel": 1,
            "Suits": [{"ItemType": "/Volt"}],
            "XPInfo": [{"ItemType": "/Volt", "XP": 100000}, {"ItemType": "/Braton", "XP": 450000}],
            "Missions": [{"Tag": "SolNode1", "Completes": 3, "Tier": 1}, {"Tag": "SolNode2", "Completes": 1},
                         {"Tag": "EarthToVenusJunction", "Completes": 1, "Tier": 1}],
            "PlayerSkills": {"LPS_TACTICAL": 9, "LPS_DRIFT_COMBAT": 10},
            "MiscItems": [{"ItemCount": 2, "ItemType": "/R/A"}, {"ItemCount": 1, "ItemType": "/Cell"}]
        }"#;
        let current = parse(raw.as_bytes()).unwrap().values;
        let d = build(&index(), &nodes, &current, &HashSet::new());

        // Volt rank 10 (100,000 = 1000 × 10²) → 2,000; Braton rank 30 → 3,000.
        assert_eq!(d.breakdown.items, 5_000);
        assert_eq!((d.breakdown.nodes, d.breakdown.steel_path_nodes), (42, 24), "junction not counted as a node");
        assert_eq!(d.breakdown.junctions, 2_000, "junction + its Steel Path");
        assert_eq!(d.star_chart[1], progress("Junctions", 1, 1), "junction total from the node data");
        assert_eq!(d.breakdown.intrinsics, 19 * 1_500);
        assert_eq!(d.total_xp, 5_000 + 66 + 2_000 + 28_500);
        assert_eq!((d.rank, d.rank_start, d.next_rank), (1, 2_500, 10_000));
        assert!(d.ready_for_test);

        assert_eq!(d.star_chart[0], progress("Normal", 2, 2));
        assert_eq!(d.star_chart[2], progress("Steel Path", 1, 2));
        assert_eq!(d.intrinsics[0], progress("Railjack", 9, 50));
        assert_eq!(d.content[0], progress("Warframes / Archwings", 0, 3), "Frost Prime, Volt, Plexus");

        assert_eq!(d.easy.len(), 1, "Volt: owned, not mastered");
        assert_eq!((d.easy[0].name.as_str(), d.easy[0].xp_gain), ("Volt", 20 * 200));

        assert_eq!(d.relics.len(), 1);
        let frost = &d.relics[0];
        assert_eq!((frost.relic_parts_owned, frost.relic_parts_total), (0, 2));
        assert_eq!(frost.relic_sources, vec!["Lith F1".to_string()], "both missing parts drop from the owned relic");

        let frost_buy = d.platinum.iter().find(|s| s.name == "Frost Prime").unwrap();
        assert_eq!(frost_buy.buy.len(), 2, "blueprint + chassis; Orokin Cell is a resource");
        assert!(!d.platinum.iter().any(|s| s.name == "Volt"), "owned items aren't bought");
        assert!(!frost.ready);
    }

    #[test]
    fn items_being_built_count_as_easy() {
        let raw = r#"{
            "PlayerLevel": 1,
            "Recipes": [{"ItemCount": 1, "ItemType": "/Lotus/Types/Recipes/FrostPrimeChassis"}],
            "PendingRecipes": [{"ItemType": "/Lotus/Types/Recipes/FrostPrimeBlueprint", "ItemId": {"$oid": "x"}}],
            "MiscItems": [{"ItemCount": 1, "ItemType": "/R/A"}]
        }"#;
        let current = parse(raw.as_bytes()).unwrap().values;
        let d = build(&index(), &NodeData::default(), &current, &HashSet::new());
        let frost = d.easy.iter().find(|s| s.name == "Frost Prime").expect("in the foundry, so Easy");
        assert!(frost.building && !frost.owned);
        assert!(!d.relics.iter().any(|s| s.name == "Frost Prime"), "not something to farm");
        assert!(!d.platinum.iter().any(|s| s.name == "Frost Prime"), "not something to buy");
    }

    #[test]
    fn all_relic_parts_owned_is_ready() {
        let raw = r#"{
            "Recipes": [{"ItemCount": 1, "ItemType": "/Lotus/Types/Recipes/FrostPrimeBlueprint"},
                        {"ItemCount": 1, "ItemType": "/Lotus/Types/Recipes/FrostPrimeChassis"}],
            "MiscItems": [{"ItemCount": 1, "ItemType": "/R/A"}]
        }"#;
        let current = parse(raw.as_bytes()).unwrap().values;
        let d = build(&index(), &NodeData::default(), &current, &HashSet::new());
        let frost = d.relics.iter().find(|s| s.name == "Frost Prime").unwrap();
        assert!(frost.ready);
        assert_eq!((frost.relic_parts_owned, frost.relic_parts_total), (2, 2));
        assert!(frost.relic_sources.is_empty(), "nothing left to open");
    }

    #[test]
    fn warframe_parts_match_their_relic_blueprints() {
        // Real shape: the recipe lists built parts (…Component); relics drop part blueprints (…Blueprint).
        let wr = "/Lotus/Types/Recipes/WarframeRecipes/";
        let warframes = format!(
            r#"[{{"uniqueName":"/Lavos","name":"Lavos Prime","masterable":true,"isPrime":true,"components":[
                {{"uniqueName":"{wr}LavosPrimeBlueprint","itemCount":1}},
                {{"uniqueName":"{wr}LavosPrimeChassisComponent","itemCount":1}},
                {{"uniqueName":"{wr}LavosPrimeHelmetComponent","itemCount":1}}]}}]"#
        );
        let relics = format!(
            r#"[{{"uniqueName":"/R/L","name":"Lith L1 Intact","rewards":[
                {{"chance":25.33,"item":{{"uniqueName":"{wr}LavosPrimeBlueprint","name":"Lavos Prime Blueprint"}}}},
                {{"chance":11,"item":{{"uniqueName":"{wr}LavosPrimeChassisBlueprint","name":"Lavos Prime Chassis Blueprint"}}}},
                {{"chance":2,"item":{{"uniqueName":"{wr}LavosPrimeHelmetBlueprint","name":"Lavos Prime Neuroptics Blueprint"}}}}]}}]"#
        );
        let index = ItemIndex::build(&[("Warframes".into(), warframes.into_bytes()), ("Relics".into(), relics.into_bytes())]);
        let raw = format!(
            r#"{{"Recipes": [{{"ItemCount": 1, "ItemType": "{wr}LavosPrimeBlueprint"}},
                             {{"ItemCount": 1, "ItemType": "{wr}LavosPrimeChassisBlueprint"}}],
                "MiscItems": [{{"ItemCount": 1, "ItemType": "/R/L"}}]}}"#
        );
        let d = build(&index, &NodeData::default(), &parse(raw.as_bytes()).unwrap().values, &HashSet::new());
        let lavos = d.relics.iter().find(|s| s.name == "Lavos Prime").unwrap();
        assert_eq!((lavos.relic_parts_owned, lavos.relic_parts_total), (2, 3), "chassis blueprint counts as the chassis");
        assert!(!lavos.ready, "neuroptics still missing");
        assert_eq!(lavos.relic_sources, vec!["Lith L1".to_string()]);
        let icon = |suffix: &str| lavos.parts.iter().find(|p| p.path.ends_with(suffix)).unwrap().owned;
        assert_eq!(icon("ChassisComponent"), 1, "the chassis icon shows the owned chassis blueprint");
        assert_eq!(icon("HelmetComponent"), 0);
    }

    #[test]
    fn closest_sets_first_and_only_with_owned_relics() {
        // Frost has 1 of 2 relic parts; no relic is owned for the missing one → not listed.
        let no_relic = r#"{"Recipes": [{"ItemCount": 1, "ItemType": "/Lotus/Types/Recipes/FrostPrimeBlueprint"}]}"#;
        let d = build(&index(), &NodeData::default(), &parse(no_relic.as_bytes()).unwrap().values, &HashSet::new());
        assert!(d.relics.is_empty(), "missing part, but no owned relic drops it");

        // Same, with the relic owned → listed as 1/2.
        let with_relic = r#"{"Recipes": [{"ItemCount": 1, "ItemType": "/Lotus/Types/Recipes/FrostPrimeBlueprint"}],
            "MiscItems": [{"ItemCount": 3, "ItemType": "/R/A"}]}"#;
        let d = build(&index(), &NodeData::default(), &parse(with_relic.as_bytes()).unwrap().values, &HashSet::new());
        let frost = &d.relics[0];
        assert_eq!((frost.relic_parts_owned, frost.relic_parts_total, frost.ready), (1, 2, false));
    }
}
