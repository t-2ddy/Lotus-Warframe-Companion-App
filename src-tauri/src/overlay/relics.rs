//! The Void Fissure reward screen (see `docs/plans/relic-rewards.md`): reading the reward cards
//! off the screen, matching them to the items the squad's relics can drop, and what each one is
//! worth to the user.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::ocr::Word;
use super::screen::{GameWindow, Region};
use crate::foundry;
use crate::translate::wfcd::split_relic_name;
use crate::translate::{Flat, ItemIndex};

/// The reward names (bottom of each card), as a fraction of the centred 16:9 area. Measured on
/// `docs/alecaExamples/Tools/voidMissionExtractionScreen.jpg` (1920×1080: names at y 410–462,
/// 4 cards spanning x 478–1442).
pub const NAMES: Region = Region { x: 0.24, y: 0.365, w: 0.52, h: 0.08 };
/// Distance between card centres (242 of 1920 px). The cards are centred, so with an odd count
/// one sits in the middle, with an even count two straddle it.
const PITCH: f64 = 0.1263;
/// Card width, and where the panels start (just under the cards), in 16:9-area fractions.
const CARD_W: f64 = 0.122;
/// Panels go under the players' names (up to three per card), like AlecaFrame's.
const PANEL_TOP: f64 = 0.60;

pub const ERAS: [&str; 6] = ["Lith", "Meso", "Neo", "Axi", "Requiem", "Omnia"];

/// The fissure era in a mission name ("Kiliken (Venus) - Lith Fissure" → "Lith").
pub fn era_of(mission: &str) -> Option<&'static str> {
    ERAS.into_iter().find(|e| mission.split(|c: char| !c.is_alphanumeric()).any(|w| w == *e))
}

/// One reward card's text and where its column is (centre, as a 16:9-area fraction).
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub centre: f64,
    pub text: String,
}

/// Groups the OCR'd words into card columns, left to right.
///
/// Words on one OCR line are first joined into phrases (a gap wider than the text height starts
/// a new one; long names on neighbouring cards can share an OCR line). Each phrase is centred
/// on its card, so its centre picks the nearest card slot: either the odd layout (one card in
/// the middle) or the even one, whichever fits all phrases better.
pub fn columns(words: &[Word], img_w: f64) -> Vec<Column> {
    let to_area = |px: f64| NAMES.x + px / img_w * NAMES.w;
    // Phrases: (line, left px, right px, text).
    let mut sorted: Vec<&Word> = words.iter().filter(|w| !w.text.trim().is_empty()).collect();
    sorted.sort_by(|a, b| a.line.cmp(&b.line).then(a.x.total_cmp(&b.x)));
    let mut phrases: Vec<(usize, f64, f64, String)> = Vec::new();
    for w in sorted {
        match phrases.last_mut() {
            Some(p) if p.0 == w.line && w.x - p.2 < w.h.max(1.0) => {
                p.2 = w.x + w.w;
                p.3.push(' ');
                p.3.push_str(&w.text);
            }
            _ => phrases.push((w.line, w.x, w.x + w.w, w.text.clone())),
        }
    }
    if phrases.is_empty() {
        return Vec::new();
    }
    let centres: Vec<f64> = phrases.iter().map(|p| to_area((p.1 + p.2) / 2.0)).collect();
    let odd = [0.5 - PITCH, 0.5, 0.5 + PITCH];
    let even = [0.5 - 1.5 * PITCH, 0.5 - 0.5 * PITCH, 0.5 + 0.5 * PITCH, 0.5 + 1.5 * PITCH];
    let nearest = |slots: &[f64], c: f64| {
        slots.iter().enumerate().map(|(i, s)| (i, (c - s).abs())).min_by(|a, b| a.1.total_cmp(&b.1)).unwrap()
    };
    let cost = |slots: &[f64]| centres.iter().map(|c| nearest(slots, *c).1).sum::<f64>();
    let slots: &[f64] = if cost(&odd) < cost(&even) { &odd } else { &even };
    let mut texts: Vec<Vec<&str>> = vec![Vec::new(); slots.len()];
    for (p, c) in phrases.iter().zip(&centres) {
        let (i, d) = nearest(slots, *c);
        if d < PITCH * 0.45 {
            texts[i].push(&p.3); // phrases are in line order, so wrapped names join top to bottom
        }
    }
    slots
        .iter()
        .zip(texts)
        .filter(|(_, t)| !t.is_empty())
        .map(|(c, t)| Column { centre: *c, text: t.join(" ") })
        .collect()
}

/// An item a reward card can show.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub name: String,
    pub path: String,
    /// "common", "uncommon" or "rare" in its relic.
    pub rarity: &'static str,
}

/// Rarity from the Intact drop chance (25.33 / 11 / 2 %).
fn rarity(intact_chance: f64) -> &'static str {
    match intact_chance {
        c if c >= 20.0 => "common",
        c if c >= 8.0 => "uncommon",
        _ => "rare",
    }
}

const RARITY_ORDER: [&str; 3] = ["common", "uncommon", "rare"];

/// The base relic name ("Lith W2") for a squad relic's path (`…/T1VoidProjectionIvaraPrimeABronze`).
pub fn relic_base(index: &ItemIndex, path: &str) -> Option<String> {
    let name = index.get(path)?.name.clone();
    let (base, _) = split_relic_name(&name);
    (!index.relic_rewards(base).is_empty()).then(|| base.to_string())
}

/// Every item the given relics can drop (each once; an item in several relics keeps its
/// rarest rarity). With no relics, every relic of `era` (or every relic).
pub fn candidates(index: &ItemIndex, relics: &[String], era: Option<&str>) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let mut add = |rewards: &[crate::translate::RelicReward]| {
        for r in rewards {
            let rar = rarity(r.chance);
            // By path and name: "Forma Blueprint" and "2X Forma Blueprint" share one path.
            match out.iter_mut().find(|c| c.path == r.path && c.name == r.name) {
                Some(c) => {
                    let rank = |x: &str| RARITY_ORDER.iter().position(|y| *y == x);
                    if rank(rar) > rank(c.rarity) {
                        c.rarity = rar;
                    }
                }
                None => out.push(Candidate { name: r.name.clone(), path: r.path.clone(), rarity: rar }),
            }
        }
    };
    if relics.is_empty() {
        for (name, rewards) in index.relics() {
            if era.is_none_or(|e| e == "Omnia" || name.starts_with(&format!("{e} "))) {
                add(rewards);
            }
        }
    } else {
        for base in relics {
            add(index.relic_rewards(base));
        }
    }
    out
}

/// Letters only, lowercase ("2X Forma Blueprint" → "xformablueprint").
fn letters(s: &str) -> String {
    s.chars().filter(|c| c.is_alphabetic()).flat_map(char::to_lowercase).collect()
}

/// Edit distance from OCR text `a` to a name `b`. A letter of the name missing from the OCR
/// costs half: card art behind the text hides letters far more often than it adds them.
fn edit_distance(a: &str, b: &str) -> f64 {
    const MISSING: f64 = 0.5;
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<f64> = (0..=b.len()).map(|j| j as f64 * MISSING).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![(i + 1) as f64; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + if ca == cb { 0.0 } else { 1.0 };
            cur[j + 1] = sub.min(prev[j + 1] + 1.0).min(cur[j] + MISSING);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// The candidate an OCR'd card name is: the closest by edit distance. Accepted when it's close
/// (a fifth of the letters wrong: "Systerns"), or fairly close and clearly closer than any other
/// item (card art behind the text: "MiragåYe hassis Blueprint").
pub fn snap<'a>(text: &str, cands: &'a [Candidate]) -> Option<&'a Candidate> {
    let want = letters(text);
    if want.len() < 4 {
        return None;
    }
    let mut scored: Vec<(&Candidate, f64)> = cands.iter().map(|c| (c, edit_distance(&want, &letters(&c.name)))).collect();
    scored.sort_by(|a, b| a.1.total_cmp(&b.1));
    let (best, d) = *scored.first()?;
    let runner_up = scored.iter().find(|(c, _)| letters(&c.name) != letters(&best.name)).map_or(f64::MAX, |(_, d)| *d);
    let len = want.chars().count() as f64;
    let close = d <= (len / 5.0).max(2.0);
    let clear = d <= len / 3.0 && runner_up >= d + 2.0;
    (close || clear).then_some(best)
}

/// One set part, for the small parts row.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SetPart {
    /// Short name: "Neuroptics", "Barrel", "Blueprint".
    pub name: String,
    /// WFCD image, and the built item's image drawn over a blueprint.
    pub image: Option<String>,
    pub overlay: Option<String>,
    pub owned: i64,
    pub needed: u32,
    /// This card's item.
    pub this: bool,
}

/// One reward card's panel.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RewardView {
    pub name: String,
    pub path: String,
    pub rarity: String,
    /// Lowest sell price on warframe.market.
    pub plat: Option<i64>,
    /// "loading", "fresh", "stale" or "untradable".
    pub price_status: String,
    pub ducats: Option<u32>,
    pub vaulted: bool,
    /// How many of this item the user has (last inventory capture).
    pub owned: i64,
    /// How many the set needs (0: not part of a set, e.g. Forma).
    pub needed: u32,
    /// What it builds ("Wukong Prime"), and the set's path (its warframe.market set price).
    pub set_name: Option<String>,
    pub set_path: Option<String>,
    pub set_plat: Option<i64>,
    /// The set item is owned / fully ranked.
    pub crafted: bool,
    pub mastered: bool,
    pub set_parts: Vec<SetPart>,
    /// warframe.market slug, for the item's page.
    pub slug: Option<String>,
    /// Placement over the game window, as fractions of its width and height.
    pub x: f64,
    pub width: f64,
    pub top: f64,
}

/// Converts a 16:9-area position to a fraction of the whole game window (wider screens have
/// bars at the sides).
fn on_window(game: &GameWindow, ax: f64, ay: f64) -> (f64, f64) {
    let (ww, wh) = (f64::from(game.width.max(1)), f64::from(game.height.max(1)));
    let area_w = ww.min(wh * 16.0 / 9.0);
    let area_h = wh.min(ww * 9.0 / 16.0);
    (((ww - area_w) / 2.0 + ax * area_w) / ww, ((wh - area_h) / 2.0 + ay * area_h) / wh)
}

/// Builds the panels for the matched cards (prices are filled in separately).
pub fn views(index: &ItemIndex, current: &Flat, game: &GameWindow, cards: &[(f64, Candidate)]) -> Vec<RewardView> {
    let sets = foundry::build(index, current, &Default::default());
    let from_relics: HashSet<&str> = index.relics().flat_map(|(_, r)| r.iter().map(|x| x.path.as_str())).collect();
    let mut loose: HashMap<String, i64> = HashMap::new();
    for ((section, item), n) in current {
        if section == "Recipes" || section == "MiscItems" {
            *loose.entry(item.clone()).or_insert(0) += n;
        }
    }
    // What a set part drops from relics as. Warframe sets list the built part
    // ("…HelmetComponent") while relics drop its blueprint ("…HelmetBlueprint"). Resources
    // (Orokin Cells, Neurodes) don't drop from relics: `None`, so they aren't shown.
    let drop_of = |p: &foundry::PartView| -> Option<String> {
        if from_relics.contains(p.path.as_str()) {
            return Some(p.path.clone());
        }
        let bp = format!("{}Blueprint", p.path.strip_suffix("Component")?);
        from_relics.contains(bp.as_str()).then_some(bp)
    };
    // Owned: the part (built, or the blueprint itself) plus an unbuilt blueprint of a built part.
    let owned_of = |p: &foundry::PartView, drop: &str| p.owned + if drop == p.path { 0 } else { loose.get(drop).copied().unwrap_or(0) };
    cards
        .iter()
        .map(|(centre, c)| {
            let info = index.get(&c.path);
            let set = sets.iter().find(|s| s.parts.iter().any(|p| drop_of(p).as_deref() == Some(c.path.as_str())));
            let part = set.and_then(|s| s.parts.iter().find(|p| drop_of(p).as_deref() == Some(c.path.as_str())));
            let (x, top) = on_window(game, *centre, PANEL_TOP);
            let (left, _) = on_window(game, centre - CARD_W / 2.0, PANEL_TOP);
            RewardView {
                name: c.name.clone(),
                path: c.path.clone(),
                rarity: c.rarity.to_string(),
                plat: None,
                price_status: "loading".into(),
                ducats: info.and_then(|i| i.ducats),
                vaulted: set.is_some_and(|s| s.vaulted),
                owned: part.map_or_else(|| loose.get(&c.path).copied().unwrap_or(0), |p| owned_of(p, &c.path)),
                needed: part.map_or(0, |p| p.needed),
                set_name: set.map(|s| s.name.clone()),
                set_path: set.map(|s| s.path.clone()),
                set_plat: None,
                crafted: set.is_some_and(|s| s.owned > 0),
                mastered: set.is_some_and(|s| s.mastered),
                set_parts: set
                    .map(|s| {
                        s.parts
                            .iter()
                            .filter_map(|p| drop_of(p).map(|d| (p, d)))
                            .map(|(p, d)| SetPart {
                                name: short_part(&s.name, &p.name),
                                image: p.image.clone(),
                                overlay: p.overlay.clone(),
                                owned: owned_of(p, &d),
                                needed: p.needed,
                                this: d == c.path,
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                slug: None,
                x,
                width: 2.0 * (x - left),
                top,
            }
        })
        .collect()
}

/// "Wukong Prime Neuroptics Blueprint" in the Wukong Prime set → "Neuroptics".
fn short_part(set: &str, part: &str) -> String {
    let rest = part.strip_prefix(set).map_or(part, str::trim);
    let rest = if rest == "Blueprint" || rest.is_empty() { "Blueprint" } else { rest.strip_suffix(" Blueprint").unwrap_or(rest) };
    rest.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, line: usize, x: f64, w: f64) -> Word {
        Word { text: text.into(), line, x, y: 10.0 * line as f64, w, h: 40.0 }
    }

    #[test]
    fn groups_words_into_card_columns() {
        // 4 cards at 1920 px, 2x upscaled crop of NAMES (x from 0.24 of the width, 0.52 wide):
        // crop px = (screen x − 460.8) × 2.
        let px = |screen: f64| (screen - 460.8) * 2.0;
        let img_w = 0.52 * 1920.0 * 2.0;
        let words = vec![
            // Line 0: two long names share one OCR line.
            word("Wukong", 0, px(975.0), 130.0),
            word("Prime", 0, px(1045.0), 100.0),
            word("Systems", 0, px(1100.0), 160.0),
            word("Mirage", 0, px(1227.0), 120.0),
            word("Prime", 0, px(1290.0), 100.0),
            word("Chassis", 0, px(1350.0), 140.0),
            // Line 1: the first two cards' names and the wrapped halves.
            word("Destreza", 1, px(494.0), 170.0),
            word("Prime", 1, px(588.0), 100.0),
            word("Handle", 1, px(640.0), 115.0),
            word("Forma", 1, px(767.0), 110.0),
            word("Blueprint", 1, px(832.0), 155.0),
            word("Blueprint", 1, px(1040.0), 155.0),
            word("Blueprint", 1, px(1283.0), 155.0),
        ];
        let cols = columns(&words, img_w);
        let texts: Vec<&str> = cols.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["Destreza Prime Handle", "Forma Blueprint", "Wukong Prime Systems Blueprint", "Mirage Prime Chassis Blueprint"]);
    }

    #[test]
    fn three_cards_use_the_middle_slot() {
        let img_w = 1000.0;
        // Area x = 0.24 + px/1000*0.52: centres 0.3737, 0.5, 0.6263.
        let at = |area: f64| (area - 0.24) / 0.52 * 1000.0;
        let words = vec![word("Forma", 0, at(0.3737) - 30.0, 60.0), word("Forma", 0, at(0.5) - 30.0, 60.0), word("Forma", 0, at(0.6263) - 30.0, 60.0)];
        let cols = columns(&words, img_w);
        assert_eq!(cols.len(), 3);
        assert!((cols[1].centre - 0.5).abs() < 1e-9);
    }

    #[test]
    fn snaps_ocr_names_to_rewards() {
        let c = |n: &str| Candidate { name: n.into(), path: format!("/{n}"), rarity: "common" };
        let cands = vec![c("Wukong Prime Systems Blueprint"), c("Wukong Prime Blueprint"), c("Forma Blueprint"), c("2X Forma Blueprint")];
        assert_eq!(snap("Wukong Prime Systerns Blueprint", &cands).unwrap().name, "Wukong Prime Systems Blueprint");
        assert_eq!(snap("Forma Blueprint", &cands).unwrap().name, "Forma Blueprint");
        assert_eq!(snap("2 X Forma Blueprint", &cands).unwrap().name, "2X Forma Blueprint");
        assert!(snap("Mirage Prime Chassis Blueprint", &cands).is_none(), "not a candidate");
        assert!(snap("Owned", &cands).is_none());
        let shared_path = vec![
            Candidate { name: "2X Forma Blueprint".into(), path: "/Forma".into(), rarity: "common" },
            Candidate { name: "Forma Blueprint".into(), path: "/Forma".into(), rarity: "common" },
        ];
        assert_eq!(snap("Forma Blueprint", &shared_path).unwrap().name, "Forma Blueprint");
        let more = vec![c("Mirage Prime Chassis Blueprint"), c("Mirage Prime Neuroptics Blueprint"), c("Nyx Prime Chassis Blueprint")];
        assert_eq!(snap("MiragåYe hassis Blueprint", &more).unwrap().name, "Mirage Prime Chassis Blueprint", "garbled by card art");
    }

    #[test]
    fn set_parts_are_relic_drops() {
        let index = ItemIndex::build(&[
            (
                "Warframes".into(),
                br#"[{"uniqueName":"/WP","name":"Wukong Prime","masterable":true,"components":[
                    {"uniqueName":"/Lotus/Types/Recipes/WarframeRecipes/WukongPrimeBlueprint","itemCount":1},
                    {"uniqueName":"/Lotus/Types/Recipes/WarframeRecipes/WukongPrimeHelmetComponent","itemCount":1},
                    {"uniqueName":"/Cell","itemCount":5}]}]"#
                    .to_vec(),
            ),
            (
                "Relics".into(),
                br#"[{"uniqueName":"/R/W1","name":"Lith W1 Intact","rewards":[
                    {"chance":10,"item":{"uniqueName":"/Lotus/Types/Recipes/WarframeRecipes/WukongPrimeBlueprint","name":"Wukong Prime Blueprint"}},
                    {"chance":10,"item":{"uniqueName":"/Lotus/Types/Recipes/WarframeRecipes/WukongPrimeHelmetBlueprint","name":"Wukong Prime Neuroptics Blueprint"}}]}]"#
                    .to_vec(),
            ),
        ]);
        let raw = r#"{"Recipes": [{"ItemType": "/Lotus/Types/Recipes/WarframeRecipes/WukongPrimeHelmetBlueprint", "ItemCount": 1}],
            "MiscItems": [{"ItemType": "/Lotus/Types/Recipes/WarframeRecipes/WukongPrimeHelmetComponent", "ItemCount": 1},
                          {"ItemType": "/Cell", "ItemCount": 3}]}"#;
        let current = crate::translate::parse(raw.as_bytes()).unwrap().values;
        let game = GameWindow { hwnd: 0, x: 0, y: 0, width: 1920, height: 1080 };
        let card = Candidate {
            name: "Wukong Prime Neuroptics Blueprint".into(),
            path: "/Lotus/Types/Recipes/WarframeRecipes/WukongPrimeHelmetBlueprint".into(),
            rarity: "uncommon",
        };
        let v = &views(&index, &current, &game, &[(0.5, card)])[0];
        assert_eq!(v.set_name.as_deref(), Some("Wukong Prime"), "the blueprint card finds the set of the built part");
        assert_eq!((v.owned, v.needed), (2, 1), "built part plus the unbuilt blueprint");
        assert_eq!(v.set_parts.len(), 2, "no Orokin Cells: {:?}", v.set_parts);
        assert!(v.set_parts[1].this && !v.set_parts[0].this);
        assert_eq!(v.set_parts[1].owned, 2);
    }

    #[test]
    fn eras_and_short_part_names() {
        assert_eq!(era_of("Kiliken (Venus) - Lith Fissure"), Some("Lith"));
        assert_eq!(era_of("Strata Relay (Earth)"), None);
        assert_eq!(short_part("Wukong Prime", "Wukong Prime Neuroptics Blueprint"), "Neuroptics");
        assert_eq!(short_part("Wukong Prime", "Wukong Prime Blueprint"), "Blueprint");
        assert_eq!(short_part("Baza Prime", "Baza Prime Barrel"), "Barrel");
    }
}
