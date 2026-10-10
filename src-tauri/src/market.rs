//! warframe.market prices for owned mods.
//!
//! - Items are matched by the game's internal path: `/v2/items` lists each tradable item's
//!   `gameRef`, which is the same `/Lotus/...` path the inventory uses.
//! - Prices come from `/v2/orders/item/{slug}/top?rank=N`: the best sell and buy orders from
//!   players who are online or in game. WTS = lowest sell, WTB = highest buy.
//!
//! Rules, so the app is a polite client of a community site:
//! - prices are only requested for rows the user is looking at (the page asks as rows scroll in)
//! - one request at a time, at most about 3 per second ([`REQUEST_GAP`])
//! - each price is cached for 2 weeks in SQLite; the item list is re-downloaded every 2 weeks
//!   (user choice: no early refresh for new items, no manual refresh)
//! - newest requests first, so the rows on screen fill in before ones scrolled past
//! - after an error or a refusal, pause and back off (30 s, doubling up to 15 min)
//! - every request carries the app's User-Agent (see `net.rs`)

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};

use crate::net;
use crate::pipeline::Shared;
use crate::update::StoredPrice;

const API: &str = "https://api.warframe.market/v2";
/// Pause between requests: about 3 per second.
const REQUEST_GAP: Duration = Duration::from_millis(350);
/// How long a price counts as fresh: 2 weeks.
pub const PRICE_TTL_MS: i64 = 14 * 24 * 60 * 60 * 1000;
/// How long the cached item list is used before downloading it again: 2 weeks. Mods added in a
/// game update show as not tradable until then.
const ITEMS_MAX_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);
const FIRST_BACKOFF: Duration = Duration::from_secs(30);
const MAX_BACKOFF: Duration = Duration::from_secs(15 * 60);

/// One price to show: an item path at one rank.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceKey {
    pub path: String,
    pub rank: u32,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PriceStatus {
    /// Fetched within the last 2 weeks.
    Fresh,
    /// Older than 2 weeks; shown while a new one is fetched.
    Stale,
    /// Not fetched yet; queued.
    Loading,
    /// Not listed on warframe.market.
    Untradable,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PriceView {
    pub path: String,
    pub rank: u32,
    pub wts: Option<i64>,
    pub wtb: Option<i64>,
    pub status: PriceStatus,
    pub fetched_at: Option<i64>,
}

impl PriceView {
    fn new(key: &PriceKey, status: PriceStatus, price: Option<StoredPrice>) -> Self {
        Self {
            path: key.path.clone(),
            rank: key.rank,
            wts: price.and_then(|p| p.wts),
            wtb: price.and_then(|p| p.wtb),
            status,
            fetched_at: price.map(|p| p.fetched_at),
        }
    }
}

/// Parses `/v2/items` into game path → market slug.
pub fn parse_items(bytes: &[u8]) -> Result<HashMap<String, String>, String> {
    let doc: Value = serde_json::from_slice(bytes).map_err(|e| format!("item list: {e}"))?;
    let list = doc.get("data").and_then(Value::as_array).ok_or("item list: no data array")?;
    Ok(list
        .iter()
        .filter_map(|i| {
            let path = i.get("gameRef")?.as_str()?;
            let slug = i.get("slug")?.as_str()?;
            Some((path.to_string(), slug.to_string()))
        })
        .collect())
}

/// Parses `/v2/orders/item/{slug}/top` into (lowest sell, highest buy).
pub fn parse_top(bytes: &[u8]) -> Result<(Option<i64>, Option<i64>), String> {
    let doc: Value = serde_json::from_slice(bytes).map_err(|e| format!("orders: {e}"))?;
    let data = doc.get("data").ok_or("orders: no data")?;
    let prices = |side: &str| -> Vec<i64> {
        data.get(side)
            .and_then(Value::as_array)
            .map(|orders| orders.iter().filter_map(|o| o.get("platinum")?.as_i64()).collect())
            .unwrap_or_default()
    };
    Ok((prices("sell").into_iter().min(), prices("buy").into_iter().max()))
}

/// Pending price requests, newest first, each at most once.
#[derive(Default)]
struct Queue {
    order: VecDeque<PriceKey>,
    queued: HashSet<PriceKey>,
}

impl Queue {
    /// Adds a key at the front, or moves it there if it's already waiting.
    fn push_front(&mut self, key: PriceKey) {
        if !self.queued.insert(key.clone()) {
            self.order.retain(|k| k != &key);
        }
        self.order.push_front(key);
    }

    fn pop(&mut self) -> Option<PriceKey> {
        let key = self.order.pop_front()?;
        self.queued.remove(&key);
        Some(key)
    }
}

pub(crate) enum FetchError {
    NotFound,
    Retry(String),
}

/// Waits until [`REQUEST_GAP`] has passed since the last warframe.market request from any part of
/// the app (item prices and riven searches share the ~3 requests/second limit).
pub(crate) fn throttle() {
    static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap();
    if let Some(at) = *last {
        let wait = REQUEST_GAP.saturating_sub(at.elapsed());
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }
    *last = Some(std::time::Instant::now());
}

pub(crate) fn get(url: &str) -> Result<Vec<u8>, FetchError> {
    throttle();
    let response = net::agent()
        .get(url)
        .header("Accept", "application/json")
        .header("Language", "en")
        .header("Platform", "pc")
        .header("Crossplay", "true")
        .call();
    match response {
        Ok(mut r) => {
            r.body_mut().with_config().limit(16 * 1024 * 1024).read_to_vec().map_err(|e| FetchError::Retry(e.to_string()))
        }
        Err(ureq::Error::StatusCode(404)) => Err(FetchError::NotFound),
        Err(e) => Err(FetchError::Retry(e.to_string())),
    }
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}

/// The price service: a request queue and one background worker.
pub struct Market {
    dir: PathBuf,
    /// Game path → market slug; `None` until the item list has loaded.
    slugs: RwLock<Option<Arc<HashMap<String, String>>>>,
    /// Whether the item list has been checked for age this session.
    items_checked: Mutex<bool>,
    queue: Mutex<Queue>,
    wake: Condvar,
    /// Keys asked for with a shorter freshness than [`PRICE_TTL_MS`] (relic rewards), with the
    /// shortest one asked for.
    max_age: Mutex<HashMap<PriceKey, i64>>,
}

impl Market {
    /// Loads the cached item list (if any) and starts the worker.
    pub fn start(app: AppHandle, shared: Arc<Shared>, dir: PathBuf) -> Arc<Self> {
        let _ = std::fs::create_dir_all(&dir);
        let cached = std::fs::read(dir.join("items.json")).ok().and_then(|b| parse_items(&b).ok());
        let market = Arc::new(Self {
            dir,
            slugs: RwLock::new(cached.map(Arc::new)),
            items_checked: Mutex::new(false),
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
            max_age: Mutex::new(HashMap::new()),
        });
        let worker = market.clone();
        std::thread::spawn(move || worker.run(app, shared));
        market
    }

    /// The item's warframe.market slug (for links), if it's listed.
    pub fn slug_for(&self, path: &str) -> Option<String> {
        self.slug(path).flatten()
    }

    /// `None` = item list not loaded yet; `Some(None)` = not on warframe.market.
    fn slug(&self, path: &str) -> Option<Option<String>> {
        let slugs = self.slugs.read().unwrap();
        slugs.as_ref().map(|s| s.get(path).cloned())
    }

    fn enqueue(&self, key: PriceKey) {
        self.queue.lock().unwrap().push_front(key);
        self.wake.notify_one();
    }

    /// Answers a batch of price requests from the page: cached prices right away, everything
    /// missing or older than [`PRICE_TTL_MS`] queued for the worker.
    pub fn request(&self, shared: &Shared, keys: Vec<PriceKey>) -> Vec<PriceView> {
        self.request_within(shared, keys, PRICE_TTL_MS)
    }

    /// Like [`Market::request`], but a cached price only counts as fresh for `max_age_ms`
    /// (relic rewards: a day ahead of time, minutes on the reward screen).
    pub fn request_within(&self, shared: &Shared, keys: Vec<PriceKey>, max_age_ms: i64) -> Vec<PriceView> {
        if max_age_ms < PRICE_TTL_MS {
            let mut ages = self.max_age.lock().unwrap();
            for k in &keys {
                let e = ages.entry(k.clone()).or_insert(max_age_ms);
                *e = (*e).min(max_age_ms);
            }
        }
        let now = now_ms();
        let guard = shared.store.lock().unwrap();
        let store = guard.as_ref();
        let mut out = Vec::with_capacity(keys.len());
        for key in keys {
            let view = match self.slug(&key.path) {
                Some(None) => PriceView::new(&key, PriceStatus::Untradable, None),
                None => {
                    self.enqueue(key.clone());
                    PriceView::new(&key, PriceStatus::Loading, None)
                }
                Some(Some(slug)) => match store.and_then(|s| s.price(&slug, key.rank).ok().flatten()) {
                    Some(p) if now - p.fetched_at < max_age_ms => PriceView::new(&key, PriceStatus::Fresh, Some(p)),
                    Some(p) => {
                        self.enqueue(key.clone());
                        PriceView::new(&key, PriceStatus::Stale, Some(p))
                    }
                    None => {
                        self.enqueue(key.clone());
                        PriceView::new(&key, PriceStatus::Loading, None)
                    }
                },
            };
            out.push(view);
        }
        out
    }

    /// Downloads the item list if it's missing or older than [`ITEMS_MAX_AGE`] (checked once per session).
    fn ensure_items(&self) -> Result<(), String> {
        let mut checked = self.items_checked.lock().unwrap();
        if *checked && self.slugs.read().unwrap().is_some() {
            return Ok(());
        }
        let path = self.dir.join("items.json");
        let fresh = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age < ITEMS_MAX_AGE);
        if !fresh || self.slugs.read().unwrap().is_none() {
            let bytes = match get(&format!("{API}/items")) {
                Ok(b) => b,
                Err(FetchError::NotFound) => return Err("item list not found".into()),
                Err(FetchError::Retry(e)) => {
                    // Keep using an old list if there is one.
                    return if self.slugs.read().unwrap().is_some() { Ok(()) } else { Err(e) };
                }
            };
            let slugs = parse_items(&bytes)?;
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, &bytes).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
            eprintln!("[market] {} tradable items listed", slugs.len());
            *self.slugs.write().unwrap() = Some(Arc::new(slugs));
            std::thread::sleep(REQUEST_GAP);
        }
        *checked = true;
        Ok(())
    }

    fn run(self: Arc<Self>, app: AppHandle, shared: Arc<Shared>) {
        let mut backoff = FIRST_BACKOFF;
        loop {
            let key = {
                let mut q = self.queue.lock().unwrap();
                loop {
                    if let Some(k) = q.pop() {
                        break k;
                    }
                    q = self.wake.wait(q).unwrap();
                }
            };

            if let Err(e) = self.ensure_items() {
                eprintln!("[market] item list unavailable: {e}; pausing {} s", backoff.as_secs());
                self.enqueue(key);
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(MAX_BACKOFF);
                continue;
            }

            let Some(Some(slug)) = self.slug(&key.path) else {
                let _ = app.emit("market-price", PriceView::new(&key, PriceStatus::Untradable, None));
                continue;
            };

            // It may have been fetched since it was queued (the same row requested twice).
            let max_age = self.max_age.lock().unwrap().get(&key).copied().unwrap_or(PRICE_TTL_MS);
            let cached = shared.store.lock().unwrap().as_ref().and_then(|s| s.price(&slug, key.rank).ok().flatten());
            if let Some(p) = cached.filter(|p| now_ms() - p.fetched_at < max_age) {
                let _ = app.emit("market-price", PriceView::new(&key, PriceStatus::Fresh, Some(p)));
                continue;
            }

            let url = format!("{API}/orders/item/{}/top?rank={}", net::encode_path_segment(&slug), key.rank);
            let result = match get(&url) {
                Ok(bytes) => parse_top(&bytes).map_err(FetchError::Retry),
                Err(e) => Err(e),
            };
            match result {
                Ok((wts, wtb)) => {
                    let price = StoredPrice { wts, wtb, fetched_at: now_ms() };
                    if let Some(store) = shared.store.lock().unwrap().as_ref() {
                        let _ = store.put_price(&slug, key.rank, price);
                    }
                    let _ = app.emit("market-price", PriceView::new(&key, PriceStatus::Fresh, Some(price)));
                    self.max_age.lock().unwrap().remove(&key);
                    backoff = FIRST_BACKOFF;
                }
                Err(FetchError::NotFound) => {
                    let _ = app.emit("market-price", PriceView::new(&key, PriceStatus::Untradable, None));
                }
                Err(FetchError::Retry(e)) => {
                    eprintln!("[market] {slug} rank {}: {e}; pausing {} s", key.rank, backoff.as_secs());
                    self.enqueue(key);
                    std::thread::sleep(backoff);
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                    continue;
                }
            }
            std::thread::sleep(REQUEST_GAP);
        }
    }
}

/// Pages ask for prices of the rows they're showing (mods, parts, blueprints).
#[tauri::command]
pub fn request_prices(
    keys: Vec<PriceKey>,
    shared: State<'_, Arc<Shared>>,
    market: State<'_, Arc<Market>>,
) -> Vec<PriceView> {
    market.request(&shared, keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_item_list() {
        let json = br#"{"apiVersion":"0.1","data":[
            {"id":"1","slug":"serration","gameRef":"/Lotus/Upgrades/Mods/Rifle/WeaponDamageAmountMod","maxRank":10},
            {"id":"2","slug":"no_ref"}]}"#;
        let slugs = parse_items(json).unwrap();
        assert_eq!(slugs.len(), 1);
        assert_eq!(slugs["/Lotus/Upgrades/Mods/Rifle/WeaponDamageAmountMod"], "serration");
    }

    #[test]
    fn picks_lowest_sell_and_highest_buy() {
        let json = br#"{"data":{"sell":[{"platinum":40},{"platinum":36},{"platinum":45}],
            "buy":[{"platinum":20},{"platinum":22}]}}"#;
        assert_eq!(parse_top(json).unwrap(), (Some(36), Some(22)));
        let empty = br#"{"data":{"sell":[{"platinum":3}],"buy":[]}}"#;
        assert_eq!(parse_top(empty).unwrap(), (Some(3), None));
        assert!(parse_top(b"not json").is_err());
    }

    #[test]
    fn queue_is_newest_first_without_duplicates() {
        let k = |p: &str| PriceKey { path: p.into(), rank: 0 };
        let mut q = Queue::default();
        q.push_front(k("a"));
        q.push_front(k("b"));
        q.push_front(k("a")); // requested again: moves to the front, not added twice
        assert_eq!(q.pop(), Some(k("a")));
        assert_eq!(q.pop(), Some(k("b")));
        assert_eq!(q.pop(), None);
    }
}
