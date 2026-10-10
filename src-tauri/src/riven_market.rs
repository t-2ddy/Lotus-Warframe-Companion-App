//! Similar rivens on warframe.market, for the riven popup.
//!
//! - Weapon and stat slugs come from the v2 lists (`/v2/riven/weapons`, `/v2/riven/attributes`),
//!   whose `gameRef` is the game path / stat tag the app already uses. Cached 2 weeks.
//! - Listings come from v1 `/auctions/search` (v2 has no auction search yet), cheapest first.
//!   The search starts narrow and widens until it finds a few direct sales:
//!   same positives and negative → same positives → the two main positives → any riven for the weapon.
//! - Requests share the app's warframe.market rate limit; results are kept for 10 minutes.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::arbitration::cached_text;
use crate::market::{get, FetchError};

const V1: &str = "https://api.warframe.market/v1";
const V2: &str = "https://api.warframe.market/v2";
const SEARCH_TTL: Duration = Duration::from_secs(10 * 60);
/// Stop widening the search once this many direct sales are found.
const ENOUGH: usize = 5;
/// Listings returned to the page.
const SHOWN: usize = 40;

#[derive(Debug, Default)]
pub struct Slugs {
    /// Weapon game path → slug.
    pub weapons: HashMap<String, String>,
    /// Weapon game path → riven type ("rifle", "shotgun", "pistol", "melee", "zaw", "kitgun", "archgun").
    pub riven_types: HashMap<String, String>,
    /// Stat tag → (slug, name, unit).
    pub stats: HashMap<String, (String, String, Option<String>)>,
}

impl Slugs {
    fn stat_by_slug(&self, slug: &str) -> Option<(&str, Option<&str>)> {
        self.stats.values().find(|(s, _, _)| s == slug).map(|(_, n, u)| (n.as_str(), u.as_deref()))
    }
}

#[derive(Deserialize)]
struct List<T> {
    data: Vec<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawWeapon {
    slug: String,
    game_ref: String,
    #[serde(default)]
    riven_type: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAttr {
    slug: String,
    game_ref: String,
    unit: Option<String>,
    i18n: HashMap<String, RawName>,
}

#[derive(Deserialize)]
struct RawName {
    name: String,
}

pub fn parse_slugs(weapons: &str, attrs: &str) -> Result<Slugs, String> {
    let w: List<RawWeapon> = serde_json::from_str(weapons).map_err(|e| format!("riven weapons: {e}"))?;
    let a: List<RawAttr> = serde_json::from_str(attrs).map_err(|e| format!("riven attributes: {e}"))?;
    Ok(Slugs {
        riven_types: w.data.iter().map(|w| (w.game_ref.clone(), w.riven_type.clone())).collect(),
        weapons: w.data.into_iter().map(|w| (w.game_ref, w.slug)).collect(),
        stats: a
            .data
            .into_iter()
            .map(|a| {
                let name = a.i18n.get("en").map_or_else(|| a.slug.clone(), |n| n.name.clone());
                (a.game_ref, (a.slug, name, a.unit))
            })
            .collect(),
    })
}

/// The slug lists, loaded once per session from the 2-week cache in `dir`.
fn slugs(dir: &Path) -> Result<Arc<Slugs>, String> {
    static SLUGS: OnceLock<Mutex<Option<Arc<Slugs>>>> = OnceLock::new();
    let cell = SLUGS.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().unwrap();
    if let Some(s) = guard.as_ref() {
        return Ok(s.clone());
    }
    let ok = |t: &str| t.contains("\"gameRef\"");
    let weapons = cached_text(dir, "riven-weapons.json", &format!("{V2}/riven/weapons"), ok)
        .ok_or("couldn't load warframe.market's riven weapon list")?;
    let attrs = cached_text(dir, "riven-attributes.json", &format!("{V2}/riven/attributes"), ok)
        .ok_or("couldn't load warframe.market's riven stat list")?;
    let s = Arc::new(parse_slugs(&weapons, &attrs)?);
    *guard = Some(s.clone());
    Ok(s)
}

/// The game's riven type for a weapon (e.g. `LotusRifleRandomModRare`), from warframe.market's
/// weapon list (cached in `dir`).
pub fn riven_type_for(dir: &Path, weapon: &str) -> Option<String> {
    let kind = match slugs(dir).ok()?.riven_types.get(weapon)?.as_str() {
        "rifle" => "LotusRifleRandomModRare",
        "shotgun" => "LotusShotgunRandomModRare",
        "pistol" => "LotusPistolRandomModRare",
        "melee" => "PlayerMeleeWeaponRandomModRare",
        "zaw" => "LotusModularMeleeRandomModRare",
        "kitgun" => "LotusModularPistolRandomModRare",
        "archgun" => "LotusArchgunRandomModRare",
        _ => return None,
    };
    Some(kind.to_string())
}

// --- Auctions ---

#[derive(Debug, Clone, Deserialize)]
struct Payload {
    payload: Auctions,
}

#[derive(Debug, Clone, Deserialize)]
struct Auctions {
    auctions: Vec<RawAuction>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawAuction {
    id: String,
    buyout_price: Option<i64>,
    starting_price: Option<i64>,
    top_bid: Option<i64>,
    #[serde(default)]
    is_direct_sell: bool,
    #[serde(default)]
    closed: bool,
    #[serde(default)]
    private: bool,
    #[serde(default = "yes")]
    visible: bool,
    #[serde(default)]
    updated: String,
    owner: RawOwner,
    item: RawItem,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
struct RawOwner {
    #[serde(default)]
    ingame_name: String,
    #[serde(default)]
    status: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RawItem {
    #[serde(default)]
    name: String,
    #[serde(default)]
    re_rolls: u32,
    #[serde(default)]
    mod_rank: u32,
    #[serde(default)]
    mastery_level: u32,
    #[serde(default)]
    polarity: String,
    #[serde(default)]
    attributes: Vec<RawAttribute>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawAttribute {
    value: f64,
    positive: bool,
    url_name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ListingStat {
    pub text: String,
    pub positive: bool,
    /// The same stat is on the user's riven.
    pub shared: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub id: String,
    pub name: String,
    /// Buyout price, or the starting price for bid-only auctions.
    pub price: i64,
    pub direct: bool,
    pub top_bid: Option<i64>,
    pub stats: Vec<ListingStat>,
    pub rerolls: u32,
    pub rank: u32,
    pub mastery: u32,
    pub polarity: String,
    pub seller: String,
    /// ingame, online or offline.
    pub status: String,
    pub updated: String,
    pub url: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Similar {
    /// How the listings match, e.g. "Same positives and negative".
    pub level: String,
    /// Listings found at that level (before trimming to the shown count).
    pub total: usize,
    pub lowest: Option<i64>,
    /// Median of the direct-sale prices.
    pub median: Option<i64>,
    pub listings: Vec<Listing>,
    /// The same search on the warframe.market website.
    pub web_url: String,
}

fn stat_text(value: f64, name: &str, unit: Option<&str>) -> String {
    let n = (value * 10.0).round() / 10.0;
    let num = if n.fract() == 0.0 { format!("{}", n.abs() as i64) } else { format!("{:.1}", n.abs()) };
    let sign = if n < 0.0 { "-" } else { "+" };
    match unit {
        Some("multiply") => format!("x{num} {name}"),
        Some("percent") => format!("{sign}{num}% {name}"),
        Some("seconds") => format!("{sign}{num}s {name}"),
        _ => format!("{sign}{num} {name}"),
    }
}

fn title(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// Search query string for v1 and the website.
fn query(weapon: &str, positives: &[&str], negative: Option<&str>) -> String {
    let mut q = format!("type=riven&weapon_url_name={weapon}&sort_by=price_asc");
    if !positives.is_empty() {
        q.push_str(&format!("&positive_stats={}", positives.join(",")));
    }
    if let Some(n) = negative {
        q.push_str(&format!("&negative_stats={n}"));
    }
    q
}

/// Recent searches by query.
static CACHE: Mutex<Option<HashMap<String, (Instant, Arc<Vec<RawAuction>>)>>> = Mutex::new(None);

fn search(q: &str) -> Result<Arc<Vec<RawAuction>>, String> {
    if let Some((at, list)) = CACHE.lock().unwrap().get_or_insert_with(HashMap::new).get(q) {
        if at.elapsed() < SEARCH_TTL {
            return Ok(list.clone());
        }
    }
    let bytes = match get(&format!("{V1}/auctions/search?{q}")) {
        Ok(b) => b,
        Err(FetchError::NotFound) => return Ok(Arc::new(Vec::new())),
        Err(FetchError::Retry(e)) => return Err(format!("warframe.market: {e}")),
    };
    let parsed: Payload = serde_json::from_slice(&bytes).map_err(|e| format!("warframe.market auctions: {e}"))?;
    let list: Vec<RawAuction> =
        parsed.payload.auctions.into_iter().filter(|a| a.visible && !a.closed && !a.private).collect();
    let list = Arc::new(list);
    CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(q.to_string(), (Instant::now(), list.clone()));
    Ok(list)
}

fn price(a: &RawAuction) -> i64 {
    a.buyout_price.or(a.starting_price).unwrap_or(0)
}

/// Builds the result from one level's listings.
fn summarise(slugs: &Slugs, weapon_name: &str, level: &str, q: &str, mine: &[String], list: &[RawAuction]) -> Similar {
    let mut sorted: Vec<&RawAuction> = list.iter().collect();
    // Direct sales first (cheapest first); bid-only auctions after, since their starting price
    // (often 1p) isn't what they sell for.
    sorted.sort_by_key(|a| (!a.is_direct_sell, price(a)));
    let mut direct: Vec<i64> = sorted.iter().filter(|a| a.is_direct_sell).map(|a| price(a)).collect();
    direct.sort_unstable();
    let listings = sorted
        .iter()
        .take(SHOWN)
        .map(|a| Listing {
            id: a.id.clone(),
            name: format!("{weapon_name} {}", title(&a.item.name)),
            price: price(a),
            direct: a.is_direct_sell,
            top_bid: a.top_bid,
            stats: a
                .item
                .attributes
                .iter()
                .map(|s| {
                    let (name, unit) = slugs.stat_by_slug(&s.url_name).unwrap_or((s.url_name.as_str(), None));
                    ListingStat { text: stat_text(s.value, name, unit), positive: s.positive, shared: mine.contains(&s.url_name) }
                })
                .collect(),
            rerolls: a.item.re_rolls,
            rank: a.item.mod_rank,
            mastery: a.item.mastery_level,
            polarity: title(&a.item.polarity),
            seller: a.owner.ingame_name.clone(),
            status: a.owner.status.clone(),
            updated: a.updated.clone(),
            url: format!("https://warframe.market/auction/{}", a.id),
        })
        .collect();
    Similar {
        level: level.to_string(),
        total: list.len(),
        lowest: direct.first().copied(),
        median: (!direct.is_empty()).then(|| direct[direct.len() / 2]),
        listings,
        web_url: format!("https://warframe.market/auctions/search?{q}"),
    }
}

/// Finds similar rivens: `positives` in the riven's order, `wanted` the positives the grading
/// sheet wants (used to pick the two main stats when widening).
pub fn find_similar(
    dir: &Path,
    weapon: &str,
    weapon_name: &str,
    positives: &[String],
    negative: Option<&str>,
    wanted: &[String],
) -> Result<Similar, String> {
    let slugs = slugs(dir)?;
    let weapon_slug = slugs.weapons.get(weapon).ok_or("this weapon isn't on warframe.market")?;
    let slug = |tag: &str| slugs.stats.get(tag).map(|(s, _, _)| s.as_str());
    let pos: Vec<&str> = positives.iter().filter_map(|t| slug(t)).collect();
    let neg = negative.and_then(slug);
    let mine: Vec<String> = pos.iter().chain(neg.iter()).map(|s| s.to_string()).collect();

    // Two main positives: wanted ones first, then in riven order.
    let mut main: Vec<&str> = wanted.iter().filter_map(|t| slug(t)).filter(|s| pos.contains(s)).collect();
    let rest: Vec<&str> = pos.iter().copied().filter(|s| !main.contains(s)).collect();
    main.extend(rest);
    main.truncate(2);

    let mut levels: Vec<(&str, Vec<&str>, Option<&str>)> = Vec::new();
    if neg.is_some() {
        levels.push(("Same positives and negative", pos.clone(), neg));
    }
    levels.push(("Same positives", pos.clone(), None));
    if pos.len() > 2 {
        levels.push(("Same two main positives", main, None));
    }
    levels.push(("Any riven for this weapon", Vec::new(), None));

    let mut last = None;
    for (label, p, n) in levels {
        let q = query(weapon_slug, &p, n);
        let list = search(&q)?;
        let direct = list.iter().filter(|a| a.is_direct_sell).count();
        let result = summarise(&slugs, weapon_name, label, &q, &mine, &list);
        if direct >= ENOUGH {
            return Ok(result);
        }
        // Keep the narrowest level that found anything, in case nothing reaches ENOUGH.
        if last.as_ref().is_none_or(|r: &Similar| r.total == 0) {
            last = Some(result);
        }
    }
    Ok(last.unwrap_or_default())
}

#[tauri::command]
pub async fn similar_rivens(
    app: AppHandle,
    weapon: String,
    weapon_name: String,
    positives: Vec<String>,
    negative: Option<String>,
    wanted: Vec<String>,
) -> Result<Similar, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("market");
    tauri::async_runtime::spawn_blocking(move || {
        let _ = std::fs::create_dir_all(&dir);
        find_similar(&dir, &weapon, &weapon_name, &positives, negative.as_deref(), &wanted)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEAPONS: &str = r#"{"apiVersion":"0.25.0","data":[{"id":"1","slug":"torid","gameRef":"/Lotus/Weapons/Torid","group":"primary","rivenType":"rifle","disposition":1.0,"reqMasteryRank":5,"i18n":{"en":{"name":"Torid"}}}]}"#;
    const ATTRS: &str = r#"{"apiVersion":"0.25.0","data":[
        {"id":"a","slug":"critical_damage","gameRef":"WeaponCritDamageMod","group":"default","unit":"percent","i18n":{"en":{"name":"Critical Damage"}}},
        {"id":"b","slug":"damage_vs_grineer","gameRef":"WeaponFactionDamageGrineer","group":"default","unit":"multiply","i18n":{"en":{"name":"Damage to Grineer"}}},
        {"id":"c","slug":"zoom","gameRef":"WeaponZoomFovMod","group":"default","unit":"percent","i18n":{"en":{"name":"Zoom"}}}]}"#;

    #[test]
    fn parses_slug_lists() {
        let s = parse_slugs(WEAPONS, ATTRS).unwrap();
        assert_eq!(s.weapons["/Lotus/Weapons/Torid"], "torid");
        assert_eq!(s.stats["WeaponZoomFovMod"].0, "zoom");
        assert_eq!(s.stat_by_slug("damage_vs_grineer"), Some(("Damage to Grineer", Some("multiply"))));
    }

    #[test]
    fn listing_stat_text() {
        assert_eq!(stat_text(182.7, "Critical Damage", Some("percent")), "+182.7% Critical Damage");
        assert_eq!(stat_text(-41.1, "Zoom", Some("percent")), "-41.1% Zoom");
        assert_eq!(stat_text(1.6, "Damage to Grineer", Some("multiply")), "x1.6 Damage to Grineer");
        assert_eq!(stat_text(2.0, "Punch Through", None), "+2 Punch Through");
    }

    #[test]
    fn summary_sorts_by_price_and_takes_median_of_direct_sales() {
        let s = parse_slugs(WEAPONS, ATTRS).unwrap();
        let json = r#"{"payload":{"auctions":[
            {"id":"x","buyout_price":700,"starting_price":700,"top_bid":null,"is_direct_sell":true,"visible":true,"closed":false,"private":false,"updated":"","owner":{"ingame_name":"A","status":"ingame"},"item":{"name":"satitis","re_rolls":3,"mod_rank":8,"mastery_level":9,"polarity":"naramon","attributes":[{"value":150.0,"positive":true,"url_name":"critical_damage"},{"value":-40.0,"positive":false,"url_name":"zoom"}]}},
            {"id":"y","buyout_price":500,"starting_price":500,"top_bid":null,"is_direct_sell":true,"visible":true,"closed":false,"private":false,"updated":"","owner":{"ingame_name":"B","status":"offline"},"item":{"name":"crita","attributes":[]}},
            {"id":"z","buyout_price":null,"starting_price":300,"top_bid":320,"is_direct_sell":false,"visible":true,"closed":false,"private":false,"updated":"","owner":{"ingame_name":"C","status":"online"},"item":{"name":"acri","attributes":[]}}]}}"#;
        let list: Payload = serde_json::from_str(json).unwrap();
        let r = summarise(&s, "Torid", "Same positives", "q", &["critical_damage".into()], &list.payload.auctions);
        assert_eq!(r.listings.iter().map(|l| l.price).collect::<Vec<_>>(), [500, 700, 300], "direct sales first");
        assert_eq!((r.lowest, r.median, r.total), (Some(500), Some(700), 3));
        assert_eq!(r.listings[1].name, "Torid Satitis");
        assert!(r.listings[1].stats[0].shared && !r.listings[1].stats[1].shared);
        assert_eq!(r.listings[1].stats[1].text, "-40% Zoom");
    }
}

#[cfg(test)]
mod live_check {
    /// Run with `MARKET_DIR=<folder> cargo test live_similar -- --ignored --nocapture` (downloads).
    #[test]
    #[ignore]
    fn live_similar() {
        let dir = std::path::PathBuf::from(std::env::var("MARKET_DIR").unwrap());
        let _ = std::fs::create_dir_all(&dir);
        let pos = ["WeaponFireIterationsMod".to_string(), "WeaponCritDamageMod".to_string()];
        let r = super::find_similar(
            &dir,
            "/Lotus/Weapons/ClanTech/Bio/BioWeapon",
            "Torid",
            &pos,
            Some("WeaponZoomFovMod"),
            &pos,
        );
        match r {
            Ok(r) => {
                println!("{} · {} found · lowest {:?} · median {:?}\n{}", r.level, r.total, r.lowest, r.median, r.web_url);
                for l in r.listings.iter().take(6) {
                    let stats: Vec<&str> = l.stats.iter().map(|s| s.text.as_str()).collect();
                    println!("{:>5}p {} {} R{} {}x {} [{}]", l.price, if l.direct { "buy" } else { "bid" }, l.name, l.rank, l.rerolls, l.status, stats.join(", "));
                }
            }
            Err(e) => println!("error: {e}"),
        }
    }
}
