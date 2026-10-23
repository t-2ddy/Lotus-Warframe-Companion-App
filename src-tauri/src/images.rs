//! Local cache for images from two community servers:
//! - item images from WFCD (`cdn.warframestat.us`, GitHub Pages)
//! - full mod cards from the Warframe wiki (`wiki.warframe.com`), cached under names starting
//!   with [`WIKI_PREFIX`]
//!
//! Rules, so the app is a polite client of shared community servers:
//! - each image is downloaded once and kept in `<app data>/images/`
//! - only when it's first shown (the UI asks for it), never in bulk
//! - at most [`WORKERS`] downloads at a time, across both servers
//! - after HTTP 429, a server error or a network failure, pause downloads from that server and back off
//! - every request carries the app's User-Agent (see `net.rs`)

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::net;
pub use crate::translate::wfcd::WIKI_PREFIX;

const BASE_URL: &str = "https://cdn.warframestat.us/img/";
const WIKI_URL: &str = "https://wiki.warframe.com/images/";

/// Which server an image name is fetched from; each has its own back-off.
fn source(name: &str) -> &'static str {
    if name.starts_with(WIKI_PREFIX) { "wiki" } else { "wfcd" }
}

fn url_for(name: &str) -> String {
    match name.strip_prefix(WIKI_PREFIX) {
        Some(file) => format!("{WIKI_URL}{}", net::encode_path_segment(file)),
        None => format!("{BASE_URL}{}", net::encode_path_segment(name)),
    }
}
/// Downloads running at once.
pub const WORKERS: usize = 4;
/// First pause after the server refuses or fails; doubles each time, up to [`MAX_BACKOFF`].
const FIRST_BACKOFF: Duration = Duration::from_secs(30);
const MAX_BACKOFF: Duration = Duration::from_secs(15 * 60);
/// Largest image accepted (the biggest WFCD images are a few hundred KB).
const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;

/// Result of one download attempt.
#[derive(Debug, PartialEq, Eq)]
pub enum Fetched {
    Image(Vec<u8>),
    NotFound,
    /// HTTP 429, a 5xx, or a network error: try again later.
    Unavailable(String),
}

/// Where images come from. The real one is HTTP; tests use a fake.
pub trait Fetch: Send + Sync {
    fn fetch(&self, name: &str) -> Fetched;
}

pub struct HttpFetch;

impl Fetch for HttpFetch {
    fn fetch(&self, name: &str) -> Fetched {
        match net::agent().get(&url_for(name)).call() {
            Ok(mut response) => match response.body_mut().with_config().limit(MAX_IMAGE_BYTES).read_to_vec() {
                Ok(bytes) => Fetched::Image(bytes),
                Err(e) => Fetched::Unavailable(e.to_string()),
            },
            Err(ureq::Error::StatusCode(404)) => Fetched::NotFound,
            Err(e) => Fetched::Unavailable(e.to_string()),
        }
    }
}

/// What the cache hands back for one request.
#[derive(Debug, PartialEq, Eq)]
pub enum Served {
    Image { bytes: Vec<u8>, mime: &'static str },
    NotFound,
    /// Not cached and downloads are paused or failed; the UI shows a placeholder.
    Unavailable,
}

struct Backoff {
    paused_until: Option<Instant>,
    next_pause: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self { paused_until: None, next_pause: FIRST_BACKOFF }
    }
}

/// Disk cache plus download rules. Thread-safe; shared by the download workers.
pub struct ImageCache {
    dir: PathBuf,
    fetcher: Box<dyn Fetch>,
    /// Back-off per server (see [`source`]).
    backoff: Mutex<HashMap<&'static str, Backoff>>,
    /// Names the server answered 404 for, so they aren't asked for again this session.
    missing: Mutex<HashSet<String>>,
}

impl ImageCache {
    pub fn new(dir: PathBuf, fetcher: Box<dyn Fetch>) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        Self { dir, fetcher, backoff: Mutex::new(HashMap::new()), missing: Mutex::new(HashSet::new()) }
    }

    /// Returns the image from disk if it's cached. Never touches the network.
    pub fn cached(&self, name: &str) -> Option<Served> {
        if !is_safe_name(name) {
            return Some(Served::NotFound);
        }
        if self.missing.lock().unwrap().contains(name) {
            return Some(Served::NotFound);
        }
        std::fs::read(self.path(name)).ok().map(|bytes| Served::Image { bytes, mime: mime_for(name) })
    }

    /// True while downloads from this image's server are paused after a refusal or failure.
    pub fn paused(&self, name: &str) -> bool {
        self.backoff
            .lock()
            .unwrap()
            .get(source(name))
            .and_then(|b| b.paused_until)
            .is_some_and(|t| Instant::now() < t)
    }

    /// Returns the image, downloading and saving it if needed. Blocks during the download,
    /// so callers run it on a download worker.
    pub fn get(&self, name: &str) -> Served {
        if let Some(served) = self.cached(name) {
            return served;
        }
        if self.paused(name) {
            return Served::Unavailable;
        }
        match self.fetcher.fetch(name) {
            Fetched::Image(bytes) => {
                let path = self.path(name);
                let tmp = path.with_extension("part");
                if std::fs::write(&tmp, &bytes).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
                self.backoff.lock().unwrap().entry(source(name)).or_default().next_pause = FIRST_BACKOFF;
                Served::Image { bytes, mime: mime_for(name) }
            }
            Fetched::NotFound => {
                self.missing.lock().unwrap().insert(name.to_string());
                Served::NotFound
            }
            Fetched::Unavailable(reason) => {
                let mut all = self.backoff.lock().unwrap();
                let b = all.entry(source(name)).or_default();
                let pause = b.next_pause;
                b.paused_until = Some(Instant::now() + pause);
                b.next_pause = (pause * 2).min(MAX_BACKOFF);
                eprintln!("[images] {name}: {reason}; pausing {} downloads for {} s", source(name), pause.as_secs());
                Served::Unavailable
            }
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

/// Image names come from WFCD data, and requests come from the page. Only plain file names are
/// allowed, so a request can never read or write outside the cache folder.
pub fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('.')
        && !name.contains("..")
        && !name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
}

fn mime_for(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else {
        "application/octet-stream"
    }
}

/// The cache plus its download queue; managed by Tauri and used by the `wfimg` protocol.
pub struct ImageService {
    pub cache: Arc<ImageCache>,
    pub queue: Mutex<Sender<Job>>,
}

impl ImageService {
    pub fn start(dir: PathBuf) -> Self {
        let cache = Arc::new(ImageCache::new(dir, Box::new(HttpFetch)));
        let queue = Mutex::new(start_workers(cache.clone()));
        Self { cache, queue }
    }

    /// Answers one image request: from disk if cached, otherwise via a download worker.
    pub fn request(&self, name: String, reply: Box<dyn FnOnce(Served) + Send>) {
        if let Some(served) = self.cache.cached(&name) {
            return reply(served);
        }
        if self.cache.paused(&name) {
            return reply(Served::Unavailable);
        }
        if let Err(failed) = self.queue.lock().unwrap().send(Job { name, reply }) {
            (failed.0.reply)(Served::Unavailable);
        }
    }
}

/// A download request waiting for a worker, with the callback that answers the page.
pub struct Job {
    pub name: String,
    pub reply: Box<dyn FnOnce(Served) + Send>,
}

/// Starts [`WORKERS`] download threads and returns the queue that feeds them.
pub fn start_workers(cache: Arc<ImageCache>) -> Sender<Job> {
    let (tx, rx) = channel::<Job>();
    let rx: Arc<Mutex<Receiver<Job>>> = Arc::new(Mutex::new(rx));
    for _ in 0..WORKERS {
        let (rx, cache) = (rx.clone(), cache.clone());
        std::thread::spawn(move || loop {
            let job = { rx.lock().unwrap().recv() };
            let Ok(job) = job else { return };
            (job.reply)(cache.get(&job.name));
        });
    }
    tx
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Returns a scripted answer per call and counts calls.
    struct FakeFetch {
        answers: Mutex<Vec<Fetched>>,
        calls: Arc<AtomicUsize>,
    }

    impl Fetch for FakeFetch {
        fn fetch(&self, _name: &str) -> Fetched {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.answers.lock().unwrap().remove(0)
        }
    }

    fn cache(answers: Vec<Fetched>) -> (ImageCache, Arc<AtomicUsize>, PathBuf) {
        // A counter, not just the time: tests run in parallel and can read the same clock tick,
        // then one test's cleanup deletes the other's folder mid-test (seen on CI).
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "wf-img-test-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let calls = Arc::new(AtomicUsize::new(0));
        let fetch = FakeFetch { answers: Mutex::new(answers), calls: calls.clone() };
        (ImageCache::new(dir.clone(), Box::new(fetch)), calls, dir)
    }

    #[test]
    fn downloads_once_then_serves_from_disk() {
        let (c, calls, dir) = cache(vec![Fetched::Image(b"png-bytes".to_vec())]);
        let first = c.get("ComponentFerrite.png");
        assert_eq!(first, Served::Image { bytes: b"png-bytes".to_vec(), mime: "image/png" });
        assert!(dir.join("ComponentFerrite.png").exists());
        assert_eq!(c.get("ComponentFerrite.png"), first);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "second request is served from disk");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn not_found_is_remembered() {
        let (c, calls, dir) = cache(vec![Fetched::NotFound]);
        assert_eq!(c.get("gone.png"), Served::NotFound);
        assert_eq!(c.get("gone.png"), Served::NotFound);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn refusal_pauses_that_server_only() {
        let (c, calls, dir) = cache(vec![
            Fetched::Unavailable("http status: 429".into()),
            Fetched::Image(b"card".to_vec()),
        ]);
        assert_eq!(c.get("a.png"), Served::Unavailable);
        assert!(c.paused("b.png"));
        assert_eq!(c.get("b.png"), Served::Unavailable, "no WFCD request while paused");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(c.backoff.lock().unwrap()["wfcd"].next_pause, FIRST_BACKOFF * 2, "backoff doubles");

        let wiki = format!("{WIKI_PREFIX}SerrationMod.png");
        assert!(!c.paused(&wiki), "the wiki isn't paused by a WFCD refusal");
        assert!(matches!(c.get(&wiki), Served::Image { .. }));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn urls_per_server() {
        assert_eq!(url_for("ComponentFerrite.png"), "https://cdn.warframestat.us/img/ComponentFerrite.png");
        assert_eq!(url_for(&format!("{WIKI_PREFIX}SerrationMod.png")), "https://wiki.warframe.com/images/SerrationMod.png");
        assert!(is_safe_name(&format!("{WIKI_PREFIX}SerrationMod.png")));
    }

    #[test]
    fn unsafe_names_never_reach_disk_or_network() {
        let (c, calls, dir) = cache(vec![]);
        for bad in ["../secret.png", "a/b.png", "C:\\x.png", ".hidden", "", "a\u{0}.png"] {
            assert_eq!(c.get(bad), Served::NotFound, "{bad:?}");
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
