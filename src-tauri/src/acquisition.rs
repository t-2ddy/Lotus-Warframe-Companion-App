//! The Foundry's item popup: the full recipe (each part's own blueprint cost too) and where to
//! get every part: the mission, relic, bounty, enemy, vendor, Clan Dojo lab or Market.
//!
//! Sources:
//! - WFCD `drops`, made from DE's drop tables: missions, relics, bounties, enemies, syndicate ranks.
//! - The Warframe wiki's data modules (CC BY-SA 3.0, credited in the app): `Module:Blueprints/data`
//!   (part recipes), `Module:Research/data` (Clan Dojo labs) and `Module:Vendors/data` (vendors
//!   and their prices). Cached and refreshed every 2 weeks, like the node data.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use serde::Serialize;
use tauri::State;

use crate::foundry::{self, PART_SECTIONS};
use crate::pipeline::Shared;
use crate::translate::lua::{parse_module, Lua};
use crate::translate::nodes::MAX_AGE;
use crate::translate::wfcd::Drop;
use crate::translate::{Flat, ItemIndex};

const WIKI_URL: &str = "https://wiki.warframe.com/w/Module:";
/// (module, cache file name)
const MODULES: [(&str, &str); 3] =
    [("Blueprints/data", "blueprints.lua"), ("Research/data", "research.lua"), ("Vendors/data", "vendors.lua")];

/// Where each vendor stands. The wiki's vendor data has names, not places.
const VENDOR_PLACES: [(&str, &str); 37] = [
    ("Hok", "Cetus, Earth"),
    ("Fisher Hai-Luk", "Cetus, Earth"),
    ("Old Man Suumbaat", "Cetus, Earth"),
    ("Master Teasonai", "Cetus, Earth"),
    ("The Quills", "Cetus, Earth"),
    ("Konzu", "Cetus, Earth"),
    ("Legs", "Fortuna, Venus"),
    ("Rude Zuud", "Fortuna, Venus"),
    ("The Business", "Fortuna, Venus"),
    ("Smokefinger", "Fortuna, Venus"),
    ("Ventkids", "Fortuna, Venus"),
    ("Vox Solaris", "Fortuna, Venus (Little Duck)"),
    ("Son", "Necralisk, Deimos"),
    ("Daughter", "Necralisk, Deimos"),
    ("Father", "Necralisk, Deimos"),
    ("Grandmother", "Necralisk, Deimos"),
    ("Otak", "Necralisk, Deimos"),
    ("Necraloid", "Necralisk, Deimos (Loid)"),
    ("Cavalero", "Chrysalith, Zariman"),
    ("Archimedean Yonta", "Chrysalith, Zariman"),
    ("Bird 3", "Sanctum Anatomica, Deimos"),
    ("Eleanor", "Höllvania Central Mall, Höllvania"),
    ("Aoi", "Höllvania Central Mall, Höllvania"),
    ("Amir", "Höllvania Central Mall, Höllvania"),
    ("Quincy", "Höllvania Central Mall, Höllvania"),
    ("Cephalon Simaris", "Sanctuary, any Relay"),
    ("Conclave", "Teshin, any Relay"),
    ("Arbitration Honors", "Arbiters of Hexis, any Relay"),
    ("The Steel Path Honors", "Teshin, any Relay"),
    ("Steel Meridian", "Syndicate, any Relay"),
    ("Arbiters of Hexis", "Syndicate, any Relay"),
    ("Cephalon Suda", "Syndicate, any Relay"),
    ("The Perrin Sequence", "Syndicate, any Relay"),
    ("Red Veil", "Syndicate, any Relay"),
    ("New Loka", "Syndicate, any Relay"),
    ("Varzia", "Maroo's Bazaar, Mars"),
    ("Acrithis", "Dormizone, Duviri"),
];

/// The wiki modules, read.
#[derive(Debug, Default)]
pub struct WikiData {
    /// Built item name ("Nova Chassis") → its blueprint entry.
    blueprints: HashMap<String, Lua>,
    /// Research name ("Banshee Chassis") → lab name ("Tenno Lab").
    research: HashMap<String, String>,
    /// Lowercase offering name → the vendors selling it.
    offers: HashMap<String, Vec<Offer>>,
}

#[derive(Debug, Clone, PartialEq)]
struct Offer {
    vendor: String,
    link: String,
    price: Option<String>,
    /// Rank or quest needed ("Rank 3", "The Waverider").
    needs: Option<String>,
}

impl WikiData {
    pub fn is_empty(&self) -> bool {
        self.blueprints.is_empty() && self.research.is_empty() && self.offers.is_empty()
    }

    /// Reads the three modules' text (any may be missing).
    pub fn parse(blueprints: Option<&str>, research: Option<&str>, vendors: Option<&str>) -> Self {
        let mut data = WikiData::default();
        if let Some(root) = blueprints.and_then(parse_module) {
            // Grouped in tables: `Blueprints` (weapons and more) and `Suits` (Warframes).
            for (_, group) in root.entries() {
                for (name, entry) in group.entries() {
                    data.blueprints.insert(entry.s("Result").unwrap_or(name).to_string(), entry.clone());
                }
            }
        }
        if let Some(root) = research.and_then(parse_module) {
            let labs = root.get("Labs");
            for (name, entry) in root.get("Research").map_or(&[][..], Lua::entries) {
                let Some(lab) = entry.s("Lab") else { continue };
                let lab_name = labs.and_then(|l| l.get(lab)).and_then(|l| l.s("Name")).unwrap_or(lab);
                data.research.insert(name.clone(), lab_name.to_string());
            }
        }
        if let Some(root) = vendors.and_then(parse_module) {
            for (key, vendor) in root.get("Vendors").map_or(&[][..], Lua::entries) {
                let name = vendor.s("Name").unwrap_or(key).to_string();
                let link = vendor.s("Link").unwrap_or(&name).to_string();
                let currency = match vendor.get("Currency") {
                    Some(Lua::Str(c)) => Some(c.as_str()),
                    _ => None, // several currencies: the offer's price table names its own
                };
                for offer in vendor.get("Offerings").map_or(&[][..], Lua::list) {
                    let Some(item) = offer.at(0).and_then(Lua::str) else { continue };
                    let needs = match offer.get("Prereq") {
                        Some(Lua::Num(n)) => Some(format!("Rank {n}")),
                        Some(Lua::Str(s)) => Some(s.clone()),
                        _ => None,
                    };
                    data.offers.entry(item.to_lowercase()).or_default().push(Offer {
                        vendor: name.clone(),
                        link: link.clone(),
                        price: offer.at(2).and_then(|p| price_text(p, currency)),
                        needs,
                    });
                }
            }
        }
        data
    }
}

/// "5,000 Standing", "35,000 Credits", "25 Vitus Essence" or "100,000 Kuva + 5 Morphics".
fn price_text(price: &Lua, currency: Option<&str>) -> Option<String> {
    match price {
        Lua::Num(n) => Some(match currency {
            Some(c) => format!("{} {c}", group(*n as i64)),
            None => group(*n as i64),
        }),
        Lua::Table { map, .. } if !map.is_empty() => Some(
            map.iter()
                .filter_map(|(k, v)| v.num().map(|n| format!("{} {k}", group(n as i64))))
                .collect::<Vec<_>>()
                .join(" + "),
        ),
        _ => None,
    }
}

/// 35000 → "35,000".
fn group(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// Loads the cached modules, downloading any that are missing or older than [`MAX_AGE`]. A failed
/// download, or one that doesn't parse, keeps the cached copy.
pub fn load_or_refresh(dir: &Path) -> WikiData {
    let _ = std::fs::create_dir_all(dir);
    let mut texts = Vec::new();
    for (module, file) in MODULES {
        let path = dir.join(file);
        let fresh = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age < MAX_AGE);
        if !fresh {
            match download(module) {
                Ok(text) if parse_module(&text).is_some_and(|v| !v.entries().is_empty()) => {
                    let tmp = path.with_extension("lua.tmp");
                    if std::fs::write(&tmp, &text).is_ok() {
                        let _ = std::fs::rename(&tmp, &path);
                    }
                }
                Ok(_) => eprintln!("[wiki] Module:{module} didn't parse; keeping the cached copy"),
                Err(e) => eprintln!("[wiki] Module:{module} download failed: {e}"),
            }
        }
        texts.push(std::fs::read_to_string(&path).ok());
    }
    WikiData::parse(texts[0].as_deref(), texts[1].as_deref(), texts[2].as_deref())
}

fn download(module: &str) -> Result<String, String> {
    let url = format!("{WIKI_URL}{module}?action=raw");
    let mut response = crate::net::agent().get(&url).call().map_err(|e| e.to_string())?;
    response.body_mut().with_config().limit(16 * 1024 * 1024).read_to_string().map_err(|e| e.to_string())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDetail {
    pub path: String,
    pub name: String,
    pub image: Option<String>,
    /// A vaulted prime: its relics no longer drop (WFCD's per-relic flag is gone, the item's isn't).
    pub vaulted: bool,
    /// Building the item: its blueprint and parts. `None` for items that aren't built.
    pub recipe: Option<Recipe>,
    /// Ways to get the whole item (bought built, sold by a vendor).
    pub sources: Vec<Source>,
    /// The wiki data has loaded (vendors, labs and part recipes come from it).
    pub wiki_loaded: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipe {
    pub credits: i64,
    /// Seconds.
    pub time: i64,
    pub ingredients: Vec<Ingredient>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ingredient {
    pub path: Option<String>,
    pub name: String,
    pub image: Option<String>,
    pub overlay: Option<String>,
    pub needed: i64,
    /// Held: built, or as its blueprint for parts.
    pub owned: i64,
    /// "blueprint" (the item's own), "part", "item" (a whole item, e.g. a weapon) or "resource".
    pub kind: &'static str,
    /// What the part's blueprint costs to build.
    pub recipe: Option<Recipe>,
    pub sources: Vec<Source>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    /// "relic", "mission", "bounty", "enemy", "activity", "vendor", "research" or "market".
    pub kind: &'static str,
    /// The node, relic, enemy, vendor or lab: "Naamah", "Lith S8", "Captain Vor", "Hok".
    pub place: String,
    /// Where that is and what else to know: "Europa · Assassination", "Cetus, Earth · Neutral".
    pub detail: String,
    /// Drop chance in percent.
    pub chance: Option<f64>,
    pub cost: Option<String>,
    pub vaulted: bool,
    /// Wiki page with more.
    pub link: Option<String>,
}

fn source(kind: &'static str, place: impl Into<String>, detail: impl Into<String>) -> Source {
    Source { kind, place: place.into(), detail: detail.into(), chance: None, cost: None, vaulted: false, link: None }
}

fn vendor_place(name: &str) -> Option<&'static str> {
    VENDOR_PLACES.iter().find(|(v, _)| *v == name).map(|(_, p)| *p)
}

fn join(parts: &[&str]) -> String {
    parts.iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join(" · ")
}

/// Turns one WFCD drop into a source. Relics are grouped by the caller.
fn classify(drop: &Drop, vendors: &HashSet<&str>) -> Source {
    let loc = drop.location.as_str();
    let chance = Some(drop.chance).filter(|c| *c > 0.0);
    // "Planet/Node (Type), Rotation C" and bounties "Venus/Orb Vallis (Level 30 - 50 Orb Vallis Bounty), Rotation C"
    let (main, rotation) = match loc.rsplit_once(", Rotation ") {
        Some((m, r)) => (m, format!("Rotation {r}")),
        None => (loc, String::new()),
    };
    if let Some((head, kind)) = main.strip_suffix(')').and_then(|m| m.rsplit_once(" (")) {
        if kind.contains("Bounty") {
            let (planet, area) = head.split_once('/').unwrap_or(("", head));
            // "Level  30 - 50 Orb Vallis Bounty" → "Level 30 - 50"
            let level = kind
                .split_whitespace()
                .take_while(|w| *w == "Level" || *w == "-" || w.parse::<u32>().is_ok())
                .collect::<Vec<_>>()
                .join(" ");
            return Source {
                chance,
                link: Some(area.to_string()),
                ..source("bounty", format!("{area} bounty"), join(&[planet, &level, &rotation]))
            };
        }
        if let Some((planet, node)) = head.split_once('/') {
            return Source { chance, link: Some(node.to_string()), ..source("mission", node, join(&[planet, kind, &rotation])) };
        }
        // "Ostron (Hok), Neutral": the vendor in brackets, its rank after the comma.
    }
    if let Some((who, rank)) = loc.split_once(", ") {
        let vendor = who.rsplit_once(" (").and_then(|(_, v)| v.strip_suffix(')')).unwrap_or(who);
        if vendors.contains(vendor) || vendor_place(vendor).is_some() {
            let place = vendor_place(vendor).unwrap_or("");
            let rank = rank.strip_prefix("Complete ").map_or(rank.to_string(), |q| format!("after {q}"));
            return Source { link: Some(vendor.to_string()), ..source("vendor", vendor, join(&[place, &rank])) };
        }
        if !rotation.is_empty() {
            return Source { chance, link: Some(main.to_string()), ..source("activity", main, rotation) };
        }
    }
    const ACTIVITIES: [&str; 10] =
        ["Container", "Sorties", "Void Storm", "Rewards", "Surplus", "Archimedea", "Descendia", "Carrypod", "Caches", "Arbitrations"];
    if ACTIVITIES.iter().any(|a| loc.contains(a)) {
        return Source { chance, ..source("activity", loc, "") };
    }
    Source { chance, link: Some(loc.to_string()), ..source("enemy", loc, "Enemy drop") }
}

/// Every source of an item from its WFCD drops: relics grouped by name (Intact chance, open ones
/// first), the rest in their own order.
fn drop_sources(index: &ItemIndex, drops: &[Drop], vendors: &HashSet<&str>) -> Vec<Source> {
    let mut relics: Vec<Source> = Vec::new();
    let mut others: Vec<Source> = Vec::new();
    for d in drops {
        if let Some((name, refinement)) = d.location.split_once(" Relic") {
            let intact = refinement.contains("Intact") || !refinement.contains('(');
            match relics.iter_mut().find(|r| r.place == name) {
                Some(r) if intact => r.chance = Some(d.chance),
                Some(_) => {}
                None => {
                    let vaulted = index
                        .path_by_name(&format!("{name} Intact"))
                        .and_then(|p| index.get(p))
                        .is_some_and(|i| i.vaulted);
                    relics.push(Source {
                        chance: Some(d.chance),
                        vaulted,
                        link: Some(format!("{name} Relic")),
                        ..source("relic", name, "Void Relic")
                    });
                }
            }
            continue;
        }
        let s = classify(d, vendors);
        // One line per place: "Rotation A" and "Rotation C" of the same node become "Rotations A, C".
        let (base, rotation) = split_rotation(&s.detail);
        let rotation = rotation.map(str::to_string);
        let same = others.iter().position(|o| o.kind == s.kind && o.place == s.place && split_rotation(&o.detail).0 == base);
        match (same, rotation) {
            (Some(i), Some(r)) => {
                let o = &mut others[i];
                let mut all: Vec<String> =
                    split_rotation(&o.detail).1.map(|l| l.split(", ").map(str::to_string).collect()).unwrap_or_default();
                if !all.contains(&r) {
                    all.push(r);
                }
                all.sort();
                let label = if all.len() > 1 { "Rotations" } else { "Rotation" };
                o.detail = join(&[&base, &format!("{label} {}", all.join(", "))]);
                o.chance = match (o.chance, s.chance) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (a, b) => a.or(b),
                };
            }
            (Some(_), None) => {}
            (None, _) => others.push(s),
        }
    }
    relics.sort_by(|a, b| a.vaulted.cmp(&b.vaulted).then(b.chance.partial_cmp(&a.chance).unwrap_or(std::cmp::Ordering::Equal)));
    others.extend(relics);
    others
}

/// "Lua · Survival · Rotation A" → ("Lua · Survival", Some("A")); "Rotations A, B" → letters "A, B".
fn split_rotation(detail: &str) -> (String, Option<&str>) {
    for label in ["Rotations ", "Rotation "] {
        if let Some((base, letters)) = detail.rsplit_once(label) {
            return (base.trim_end_matches(" · ").to_string(), Some(letters));
        }
    }
    (detail.to_string(), None)
}

/// One line per vendor: the WFCD rank listing and the wiki's offer of the same thing merge, keeping
/// the price.
fn merge_vendors(list: Vec<Source>) -> Vec<Source> {
    let mut out: Vec<Source> = Vec::new();
    for s in list {
        match out.iter_mut().find(|o| s.kind == "vendor" && o.kind == "vendor" && o.place == s.place) {
            Some(o) => {
                if o.cost.is_none() && s.cost.is_some() {
                    *o = s;
                }
            }
            None => out.push(s),
        }
    }
    out
}

/// Weapons that come from an enemy, not a blueprint drop: Kuva Lich, Sister and Coda weapons.
fn nemesis_source(name: &str) -> Option<Source> {
    let (enemy, link) = [("Kuva ", "Kuva Lich", "Kuva Lich"), ("Tenet ", "Sister of Parvos", "Sisters of Parvos"), ("Coda ", "Technocyte Coda", "Technocyte Coda")]
        .into_iter()
        .find(|(prefix, _, _)| name.starts_with(prefix))
        .map(|(_, e, l)| (e, l))?;
    Some(Source { link: Some(link.to_string()), ..source("enemy", enemy, "Vanquish or convert one carrying this weapon") })
}

/// The wiki's sources for something named `name` ("Banshee Chassis", "Yareli"): vendors selling it
/// (or its blueprint or locator) and the Clan Dojo lab that researches it.
fn wiki_sources(wiki: &WikiData, name: &str) -> Vec<Source> {
    let mut out = Vec::new();
    for wanted in [format!("{name} Blueprint"), name.to_string(), format!("{name} Locator")] {
        for o in wiki.offers.get(&wanted.to_lowercase()).map_or(&[][..], Vec::as_slice) {
            let what = if wanted.ends_with("Locator") { "Locator: play its mission for the part" } else { "" };
            let place = vendor_place(&o.vendor).unwrap_or("");
            out.push(Source {
                cost: o.price.clone(),
                link: Some(o.link.clone()),
                ..source("vendor", o.vendor.clone(), join(&[place, o.needs.as_deref().unwrap_or(""), what]))
            });
        }
    }
    if let Some(lab) = wiki.research.get(name) {
        out.push(Source { link: Some("Research".to_string()), ..source("research", lab.clone(), "Clan Dojo research") });
    }
    out
}

/// A part's own recipe from the wiki: its entry ("Nova Chassis"), or the cost listed inside the
/// item's entry for parts without one (Aeolak's Barrel).
fn part_recipe(index: &ItemIndex, wiki: &WikiData, owned: &HashMap<String, i64>, item: &str, part: &str) -> Option<Recipe> {
    let entry = match wiki.blueprints.get(&format!("{item} {part}")) {
        Some(e) => e,
        None => {
            let parent = wiki.blueprints.get(item)?;
            let p = parent.get("Parts")?.list().iter().find(|p| p.s("Name") == Some(part))?;
            p.get("Cost")?
        }
    };
    let ingredients: Vec<Ingredient> = entry
        .get("Parts")?
        .list()
        .iter()
        .filter_map(|p| {
            let name = p.s("Name")?;
            let path = index.path_by_name(name).map(str::to_string);
            let info = path.as_deref().and_then(|p| index.get(p));
            Some(Ingredient {
                owned: path.as_deref().and_then(|p| owned.get(p)).copied().unwrap_or(0),
                image: info.and_then(|i| i.image.clone()),
                overlay: None,
                needed: p.n("Count").unwrap_or(1),
                kind: "resource",
                recipe: None,
                sources: Vec::new(),
                name: name.to_string(),
                path,
            })
        })
        .collect();
    Some(Recipe { credits: entry.n("Credits").unwrap_or(0), time: entry.n("Time").unwrap_or(0), ingredients })
}

fn market_blueprint(credits: i64) -> Source {
    Source { cost: Some(format!("{} Credits", group(credits))), link: Some("Market".to_string()), ..source("market", "Market", "Blueprint") }
}

/// Builds the popup for one masterable item. Pure, so it can be tested without Tauri.
pub fn detail(index: &ItemIndex, wiki: &WikiData, current: &Flat, path: &str) -> Option<ItemDetail> {
    let m = index.masterables().iter().find(|m| m.path == path)?;
    let item = foundry::build(index, current, &HashSet::new()).into_iter().find(|i| i.path == path)?;
    let owned = foundry::totals(current, &PART_SECTIONS);
    let vendors: HashSet<&str> = wiki.offers.values().flatten().map(|o| o.vendor.as_str()).collect();
    let build = index.build_of(path);

    let ingredients: Vec<Ingredient> = item
        .parts
        .iter()
        .map(|p| {
            let resource = crate::mastery::is_resource(index, &p.path);
            let whole_item = index.masterables().iter().any(|x| x.path == p.path);
            // The item's own blueprint, named "<Item> Blueprint".
            let main = p.name == format!("{} Blueprint", item.name);
            let short = p.name.strip_prefix(&format!("{} ", item.name)).unwrap_or(&p.name);
            let base = p.name.strip_suffix(" Blueprint").unwrap_or(&p.name);
            let mut sources = drop_sources(index, index.drops(&p.path), &vendors);
            let mut recipe = None;
            let kind = if resource {
                // Resources: the likeliest drops only.
                sources.sort_by(|a, b| b.chance.partial_cmp(&a.chance).unwrap_or(std::cmp::Ordering::Equal));
                sources.truncate(5);
                "resource"
            } else if main {
                sources.extend(wiki_sources(wiki, &item.name));
                // WFCD gives lab-researched blueprints a price too (Banshee), but those aren't in
                // the Market.
                let researched = sources.iter().any(|s| s.kind == "research");
                if let Some(c) = build.and_then(|b| b.bp_cost).filter(|_| !researched) {
                    sources.insert(0, market_blueprint(c));
                }
                if sources.is_empty() {
                    sources.extend(nemesis_source(&item.name));
                }
                "blueprint"
            } else if whole_item {
                sources.extend(wiki_sources(wiki, &p.name));
                if let Some(plat) = index.build_of(&p.path).and_then(|b| b.market_cost) {
                    sources.push(market_item(plat));
                }
                "item"
            } else {
                for s in wiki_sources(wiki, base) {
                    if !sources.iter().any(|o| o.kind == s.kind && o.place == s.place) {
                        sources.push(s);
                    }
                }
                let short_part = short.strip_suffix(" Blueprint").unwrap_or(short);
                recipe = part_recipe(index, wiki, &owned, &item.name, short_part);
                "part"
            };
            Ingredient {
                path: Some(p.path.clone()),
                name: p.name.clone(),
                image: p.image.clone(),
                overlay: p.overlay.clone(),
                needed: i64::from(p.needed),
                owned: p.owned,
                kind,
                recipe,
                sources: merge_vendors(sources),
            }
        })
        .collect();

    let recipe = (!ingredients.is_empty()).then(|| Recipe {
        credits: build.map_or(0, |b| b.credits),
        time: build.map_or(0, |b| b.time),
        ingredients,
    });
    // The whole item: bought built in the Market, or from a vendor, a drop or a lab when it has no
    // blueprint of its own.
    let mut sources = Vec::new();
    if let Some(plat) = build.and_then(|b| b.market_cost) {
        sources.push(market_item(plat));
    }
    if recipe.is_none() {
        sources.extend(drop_sources(index, index.drops(path), &vendors));
        sources.extend(wiki_sources(wiki, &item.name));
        if sources.is_empty() {
            sources.extend(nemesis_source(&item.name));
        }
    }
    Some(ItemDetail {
        path: m.path.clone(),
        name: item.name,
        image: item.image,
        vaulted: m.vaulted,
        recipe,
        sources: merge_vendors(sources),
        wiki_loaded: !wiki.is_empty(),
    })
}

fn market_item(platinum: i64) -> Source {
    Source { cost: Some(format!("{} Platinum", group(platinum))), link: Some("Market".to_string()), ..source("market", "Market", "Built, ready to use") }
}

#[tauri::command]
pub fn get_item_detail(shared: State<'_, Arc<Shared>>, path: String) -> Result<ItemDetail, String> {
    let index = shared.items.read().unwrap().clone();
    let wiki = shared.wiki.read().unwrap().clone();
    let current = {
        let guard = shared.store.lock().unwrap();
        match guard.as_ref().map(|s| s.latest_account().map(|a| (s, a))) {
            Some(Ok((s, Some(a)))) => s.current(&a).map_err(|e| e.to_string())?,
            _ => Flat::new(),
        }
    };
    detail(&index, &wiki, &current, &path).ok_or_else(|| format!("unknown item {path}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::parse;

    fn drop(location: &str, chance: f64) -> Drop {
        Drop { location: location.to_string(), chance }
    }

    #[test]
    fn classifies_drop_locations() {
        let vendors: HashSet<&str> = ["Red Veil"].into();
        let c = |l: &str| classify(&drop(l, 10.0), &vendors);
        let s = c("Europa/Naamah (Assassination)");
        assert_eq!((s.kind, s.place.as_str(), s.detail.as_str()), ("mission", "Naamah", "Europa · Assassination"));
        let s = c("Lua/Circulus (Survival), Rotation A");
        assert_eq!(s.detail, "Lua · Survival · Rotation A");
        let s = c("Venus/Orb Vallis (Level 30 - 50 Orb Vallis Bounty), Rotation C");
        assert_eq!((s.kind, s.place.as_str(), s.detail.as_str()), ("bounty", "Orb Vallis bounty", "Venus · Level 30 - 50 · Rotation C"));
        let s = c("Ostron (Hok), Neutral");
        assert_eq!((s.kind, s.place.as_str(), s.detail.as_str(), s.chance), ("vendor", "Hok", "Cetus, Earth · Neutral", None));
        let s = c("Red Veil, Exalted");
        assert_eq!((s.kind, s.place.as_str()), ("vendor", "Red Veil"));
        let s = c("Cephalon Simaris, Complete The Waverider");
        assert_eq!(s.detail, "Sanctuary, any Relay · after The Waverider");
        let s = c("Captain Vor");
        assert_eq!((s.kind, s.detail.as_str(), s.link.as_deref()), ("enemy", "Enemy drop", Some("Captain Vor")));
        assert_eq!(c("Sanctuary Onslaught, Rotation B").kind, "activity");
        assert_eq!(c("Rare Orokin Storage Container").kind, "activity");
    }

    #[test]
    fn groups_relics_at_intact() {
        let index = ItemIndex::build(&[(
            "Relics".into(),
            br#"[{"uniqueName":"/R/A9","name":"Axi A9 Intact","vaulted":true},{"uniqueName":"/R/N4","name":"Lith N4 Intact"}]"#.to_vec(),
        )]);
        let drops = [
            drop("Axi A9 Relic (Radiant)", 16.67),
            drop("Axi A9 Relic (Intact)", 25.33),
            drop("Lith N4 Relic (Radiant)", 10.0),
            drop("Lith N4 Relic (Intact)", 2.0),
        ];
        let s = drop_sources(&index, &drops, &HashSet::new());
        let got: Vec<(&str, Option<f64>, bool)> = s.iter().map(|s| (s.place.as_str(), s.chance, s.vaulted)).collect();
        assert_eq!(got, [("Lith N4", Some(2.0), false), ("Axi A9", Some(25.33), true)], "open relics first, Intact chances");
    }

    #[test]
    fn one_line_per_place() {
        let drops = [
            drop("Zariman/Tuvul Commons (Void Cascade), Rotation B", 4.6),
            drop("Zariman/Tuvul Commons (Void Cascade), Rotation A", 4.23),
            drop("Zariman/Tuvul Commons (Void Cascade), Rotation C", 8.33),
            drop("Ostron (Hok), Neutral", 100.0),
        ];
        let s = drop_sources(&ItemIndex::default(), &drops, &HashSet::new());
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].detail.as_str(), s[0].chance), ("Zariman · Void Cascade · Rotations A, B, C", Some(8.33)));
        let priced = Source { cost: Some("5,000 Standing".into()), ..source("vendor", "Hok", "Cetus, Earth") };
        let merged = merge_vendors(vec![s[1].clone(), priced.clone()]);
        assert_eq!(merged, [priced], "the priced offer wins");
        assert_eq!(nemesis_source("Kuva Nukor").unwrap().place, "Kuva Lich");
        assert!(nemesis_source("Nukor").is_none());
    }

    #[test]
    fn reads_wiki_modules() {
        let wiki = WikiData::parse(
            Some(r#"return { Blueprints = { ["Nova Chassis"] = { Credits = 15000, Time = 43200, BPCost = 15000, Result = "Nova Chassis",
                Parts = { { Count = 1000, Name = "Ferrite", Type = "Resource" } } } } }"#),
            Some(r#"local Data = { ["Labs"] = { ["Tenno"] = { Name = "Tenno Lab" } },
                ["Research"] = { ['Banshee Chassis'] = { Lab = 'Tenno', Credits = 10000 } } }
                for k,v in pairs(Data) do v.Name = k end
                return Data"#),
            Some(r#"return { Vendors = {
                ["Arbitration Honors"] = { Currency = "Vitus Essence", Link = "Arbitration Honors", Name = "Arbitration Honors",
                    Offerings = { { "Grendel Chassis Locator", "Key", 25 } } },
                Hok = { Currency = "Standing", Name = "Hok", Offerings = { { "Kuva", "Resource", { ["Kuva"] = 100000 }, 1, Prereq = 3 } } } } }"#),
        );
        assert_eq!(wiki.research.get("Banshee Chassis").map(String::as_str), Some("Tenno Lab"));
        let grendel = wiki_sources(&wiki, "Grendel Chassis");
        assert_eq!(grendel.len(), 1);
        assert_eq!((grendel[0].place.as_str(), grendel[0].cost.as_deref()), ("Arbitration Honors", Some("25 Vitus Essence")));
        assert!(grendel[0].detail.contains("Locator"));
        let hok = &wiki.offers["kuva"][0];
        assert_eq!((hok.price.as_deref(), hok.needs.as_deref()), (Some("100,000 Kuva"), Some("Rank 3")));
        let banshee = wiki_sources(&wiki, "Banshee Chassis");
        assert_eq!((banshee[0].kind, banshee[0].place.as_str()), ("research", "Tenno Lab"));
    }

    #[test]
    fn builds_a_detail() {
        let wr = "/Lotus/Types/Recipes/WarframeRecipes/Nova";
        let index = ItemIndex::build(&[
            (
                "Warframes".into(),
                format!(
                    r#"[{{"uniqueName":"/Nova","name":"Nova","masterable":true,"buildPrice":25000,"buildTime":259200,"marketCost":375,"bpCost":35000,
                    "components":[{{"uniqueName":"{wr}Blueprint","itemCount":1}},{{"uniqueName":"{wr}ChassisComponent","itemCount":1}},
                    {{"uniqueName":"/Lotus/Types/Items/MiscItems/OrokinCell","itemCount":1}}]}}]"#
                )
                .into_bytes(),
            ),
            (
                "Components".into(),
                format!(
                    r#"[{{"uniqueName":"{wr}Blueprint","name":"Blueprint"}},
                    {{"uniqueName":"{wr}ChassisComponent","name":"Chassis","drops":[{{"location":"Europa/Naamah (Assassination)","chance":38.72}}]}}]"#
                )
                .into_bytes(),
            ),
            (
                "Resources".into(),
                br#"[{"uniqueName":"/Lotus/Types/Items/MiscItems/OrokinCell","name":"Orokin Cell"},{"uniqueName":"/Ferrite","name":"Ferrite"}]"#.to_vec(),
            ),
        ]);
        let wiki = WikiData::parse(
            Some(r#"return { Blueprints = { ["Nova Chassis"] = { Credits = 15000, Time = 43200, Result = "Nova Chassis",
                Parts = { { Count = 1000, Name = "Ferrite", Type = "Resource" } } } } }"#),
            None,
            None,
        );
        let raw = r#"{"MiscItems":[{"ItemType":"/Ferrite","ItemCount":400}]}"#;
        let current = parse(raw.as_bytes()).unwrap().values;
        let d = detail(&index, &wiki, &current, "/Nova").unwrap();
        assert_eq!(d.sources[0].cost.as_deref(), Some("375 Platinum"));
        let r = d.recipe.unwrap();
        assert_eq!((r.credits, r.time), (25000, 259200));
        let kinds: Vec<&str> = r.ingredients.iter().map(|i| i.kind).collect();
        assert_eq!(kinds, ["blueprint", "part", "resource"]);
        assert_eq!(r.ingredients[0].sources[0].cost.as_deref(), Some("35,000 Credits"), "main blueprint from the Market");
        let chassis = &r.ingredients[1];
        assert_eq!((chassis.sources[0].kind, chassis.sources[0].place.as_str()), ("mission", "Naamah"));
        let sub = chassis.recipe.as_ref().unwrap();
        assert_eq!((sub.credits, sub.ingredients[0].name.as_str(), sub.ingredients[0].needed, sub.ingredients[0].owned), (15000, "Ferrite", 1000, 400));
    }
}
