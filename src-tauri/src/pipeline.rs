//! Wires Trigger → Capture → Translate → Update together on a background thread and reports to the UI.

use std::path::PathBuf;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::acquisition::{self, WikiData};
use crate::arbitration::{self, ArbData};
use crate::riven_grades::{self, GradeSheet};
use crate::capture::{self, find_inventory, find_pid, ProcessSource, Scope, GAME_EXE};
use crate::gamelog::{self, FeedState, LineMerger};
use crate::translate::nodes::{self, NodeData};
use crate::translate::{self, wfcd, ItemIndex};
use crate::trigger::{default_log_path, last_login_name, parse_line, LogEvent, LogTail};
use crate::update::{Outcome, Store};

/// How often the watcher checks EE.log (live lines from the game's debug output don't wait).
const POLL: Duration = Duration::from_millis(100);
/// How often to check whether the game is running.
const GAME_CHECK: Duration = Duration::from_secs(2);
/// The inventory text is freed soon after it arrives, so retry quickly for a short window.
const CAPTURE_ATTEMPTS: u32 = 10;
const CAPTURE_RETRY: Duration = Duration::from_millis(500);

/// Heap memory only: about 0.3 s per scan, and where every copy found so far has lived.
/// (Adding mapped memory takes about 20 s and never found anything extra.)
const FAST_SCOPE: Scope = Scope::Heap;

/// Inventory sections counted in the capture summary.
const SECTIONS: [&str; 8] =
    ["Suits", "LongGuns", "Pistols", "Melee", "MiscItems", "Recipes", "PendingRecipes", "XPInfo"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSummary {
    pub captured_at_ms: u64,
    pub bytes: usize,
    pub address: String,
    pub region_kind: String,
    pub regions: usize,
    pub scanned_mb: u64,
    pub duration_ms: u64,
    pub sections: Vec<(String, usize)>,
    pub saved_to: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub game_running: bool,
    pub log_path: Option<String>,
    /// True while the game's lines arrive live (debug output); false when only EE.log is read.
    pub live_log: bool,
    pub last_event: Option<String>,
    pub last_capture: Option<CaptureSummary>,
    pub last_error: Option<String>,
}

/// State shared between the watcher thread and Tauri commands.
#[derive(Default)]
pub struct Shared {
    status: Mutex<Status>,
    /// Only one capture runs at a time (watcher and the manual button can overlap).
    capture_lock: Mutex<()>,
    /// The local SQLite database; `None` if it couldn't be opened.
    pub store: Mutex<Option<Store>>,
    /// WFCD item names; empty until the data is loaded.
    pub items: RwLock<Arc<ItemIndex>>,
    /// Star chart nodes and their mastery XP (Warframe wiki); the bundled snapshot until refreshed.
    pub nodes: RwLock<Arc<NodeData>>,
    /// In-game name from the most recent "Logged in" line; saved with the next captured account.
    login_name: Mutex<Option<String>>,
    /// Arbitration schedule and tiers (browse.wf); empty until loaded.
    pub arbitrations: RwLock<Arc<ArbData>>,
    /// Riven grading sheet (44bananas / Xennethkeisere); empty until loaded.
    pub riven_sheet: RwLock<Arc<GradeSheet>>,
    /// Part recipes, Clan Dojo labs and vendors (Warframe wiki); empty until loaded.
    pub wiki: RwLock<Arc<WikiData>>,
}

/// Opens the database, loads item data and starts the watcher. Call once from Tauri's `setup`.
pub fn start(app: AppHandle) {
    let shared = app.state::<Arc<Shared>>().inner().clone();
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = std::fs::create_dir_all(&dir);
        match Store::open(&dir.join("wf-companion.db")) {
            Ok(store) => *shared.store.lock().unwrap() = Some(store),
            Err(e) => eprintln!("[update] couldn't open database: {e}"),
        }

        // Cached item data first (instant), then refresh from WFCD in the background if it's old.
        let wfcd_dir = dir.join("wfcd");
        set_items(&app, &shared, ItemIndex::build(&wfcd::load_cached(&wfcd_dir)));
        let (app2, shared2) = (app.clone(), shared.clone());
        std::thread::spawn(move || {
            let files = wfcd::load_or_refresh(&wfcd_dir);
            set_items(&app2, &shared2, ItemIndex::build(&files));
        });

        // Riven grading sheet: cached copy, refreshed every 2 weeks.
        let (app5, shared5, sheet_dir) = (app.clone(), shared.clone(), dir.join("riven-grades"));
        std::thread::spawn(move || {
            let sheet = riven_grades::load_or_refresh(&sheet_dir);
            let weapons: usize = sheet.tabs.values().map(|t| t.len()).sum();
            eprintln!("[rivens] grading sheet: {weapons} weapons in {} tabs", sheet.tabs.len());
            *shared5.riven_sheet.write().unwrap() = Arc::new(sheet);
            let _ = app5.emit("riven-grades-updated", ());
        });

        // Arbitration schedule: cached copy, refreshed from browse.wf every 2 weeks.
        let (app4, shared4, arbys_dir) = (app.clone(), shared.clone(), dir.join("arbitrations"));
        std::thread::spawn(move || {
            let data = arbitration::load_or_refresh(&arbys_dir);
            eprintln!("[arbys] {} arbitrations scheduled, {} nodes tiered", data.schedule.len(), data.tiers.len());
            *shared4.arbitrations.write().unwrap() = Arc::new(data);
            let _ = app4.emit("arbitrations-updated", ());
        });

        // Wiki part recipes, labs and vendors (the Foundry's item popup): cached, refreshed every 2 weeks.
        let (shared6, wiki_dir) = (shared.clone(), dir.join("wiki"));
        std::thread::spawn(move || {
            let data = acquisition::load_or_refresh(&wiki_dir);
            eprintln!("[wiki] recipes, labs and vendors loaded: {}", if data.is_empty() { "none" } else { "ok" });
            *shared6.wiki.write().unwrap() = Arc::new(data);
        });

        // Node data: the bundled snapshot now, the wiki's current data once it's loaded.
        *shared.nodes.write().unwrap() = Arc::new(NodeData::bundled());
        let (app3, shared3, nodes_dir) = (app.clone(), shared.clone(), dir.join("nodes"));
        std::thread::spawn(move || {
            let data = nodes::load_or_refresh(&nodes_dir);
            eprintln!("[nodes] {} nodes, {} count toward mastery", data.len(), data.counted().count());
            *shared3.nodes.write().unwrap() = Arc::new(data);
            let _ = app3.emit("inventory-updated", ());
        });
    }
    std::thread::spawn(move || watch(app));
}

fn set_items(app: &AppHandle, shared: &Shared, index: ItemIndex) {
    if index.len() == 0 {
        return;
    }
    eprintln!("[translate] {} items loaded", index.len());
    *shared.items.write().unwrap() = Arc::new(index);
    let _ = app.emit("inventory-updated", ());
}

fn watch(app: AppHandle) {
    let shared = app.state::<Arc<Shared>>().inner().clone();
    let Some(log_path) = default_log_path() else {
        set_error(&app, &shared, "LOCALAPPDATA is not set; can't find EE.log".into());
        return;
    };
    shared.status.lock().unwrap().log_path = Some(log_path.display().to_string());
    // The current log may already hold a login from before the app started.
    if let Some(name) = last_login_name(&log_path) {
        remember_login(&app, &shared, name, true);
    }
    let mut tail = LogTail::from_end(&log_path);
    eprintln!("[trigger] watching {}", log_path.display());
    let feed = gamelog::start();
    let mut merger = LineMerger::default();
    let mut last_game_check = Instant::now() - GAME_CHECK;

    loop {
        if last_game_check.elapsed() >= GAME_CHECK {
            last_game_check = Instant::now();
            let running = find_pid(GAME_EXE).is_some();
            // Holding the debug channel is what counts: in menus the game can go quiet for a while.
            let live = running && gamelog::state() == FeedState::Listening;
            let changed = {
                let mut st = shared.status.lock().unwrap();
                let changed = st.game_running != running || st.live_log != live;
                st.game_running = running;
                st.live_log = live;
                changed
            };
            if changed {
                eprintln!("[trigger] game running: {running}, live log: {live}");
                crate::overlay::on_log_source(live);
                emit_status(&app, &shared);
            }
        }

        // Live lines as they come, for up to one poll interval; then the file's new lines.
        let until = Instant::now() + POLL;
        loop {
            match feed.recv_timeout(until.saturating_duration_since(Instant::now())) {
                Ok(line) => {
                    merger.live(&line);
                    on_line(&app, &shared, &line);
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    std::thread::sleep(until.saturating_duration_since(Instant::now()));
                    break;
                }
            }
        }
        for line in tail.poll() {
            if merger.file(&line) {
                on_line(&app, &shared, &line);
            }
        }
    }
}

/// Handles one game log line, whichever source it came from.
fn on_line(app: &AppHandle, shared: &Shared, line: &str) {
    crate::overlay::on_log_line(line);
    let Some(event) = parse_line(line) else { return };
    if let LogEvent::LoggedIn { name } = &event {
        remember_login(app, shared, name.clone(), false);
        eprintln!("[trigger] LoggedIn");
    } else {
        eprintln!("[trigger] {event:?}");
    }
    let label = match &event {
        LogEvent::LoggedIn { .. } => "LoggedIn".to_string(),
        other => format!("{other:?}"),
    };
    shared.status.lock().unwrap().last_event = Some(label);
    emit_status(app, shared);
    if matches!(event, LogEvent::InventoryArrived { .. } | LogEvent::MissionCommitted) {
        capture_with_retries(app, shared);
    }
}

fn capture_with_retries(app: &AppHandle, shared: &Shared) {
    let mut last_err = String::new();
    for attempt in 1..=CAPTURE_ATTEMPTS {
        match capture_once(app, shared, FAST_SCOPE) {
            Ok(s) => {
                eprintln!(
                    "[capture] attempt {attempt}: found {} bytes at {} ({}) in {} ms",
                    s.bytes, s.address, s.region_kind, s.duration_ms
                );
                return;
            }
            Err(e) => {
                eprintln!("[capture] attempt {attempt}: {e}");
                last_err = e;
            }
        }
        std::thread::sleep(CAPTURE_RETRY);
    }
    set_error(app, shared, format!("Capture failed after {CAPTURE_ATTEMPTS} attempts: {last_err}"));
}

/// Scans the running game once. On success saves the JSON, updates status and notifies the UI.
fn capture_once(app: &AppHandle, shared: &Shared, scope: Scope) -> Result<CaptureSummary, String> {
    let _guard = shared.capture_lock.lock().unwrap();
    let pid = find_pid(GAME_EXE).ok_or("Warframe is not running")?;
    let source = ProcessSource::open(pid, scope).map_err(|e| format!("Couldn't open the game process: {e}"))?;

    let started = Instant::now();
    let (snapshot, stats) = find_inventory(&source);
    let duration_ms = started.elapsed().as_millis() as u64;
    let scanned_mb = stats.bytes_scanned / (1024 * 1024);
    let Some(snapshot) = snapshot else {
        let reasons: Vec<String> = stats
            .rejections
            .iter()
            .map(|r| format!("{:#x}: {}", r.address, r.reason))
            .collect();
        save_rejections(app, &stats.rejections);
        return Err(format!(
            "No complete inventory found (checked {} memory blocks, {scanned_mb} MB, in {duration_ms} ms; {} possible copies){}",
            stats.regions,
            stats.candidates,
            if reasons.is_empty() { String::new() } else { format!("; rejected: {}", reasons.join("; ")) }
        ));
    };

    let saved_to = save_snapshot(app, &snapshot)?;
    let captured_at_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
    store_capture(app, shared, &snapshot.bytes, captured_at_ms as i64);
    let summary = CaptureSummary {
        captured_at_ms,
        bytes: snapshot.bytes.len(),
        address: format!("{:#x}", snapshot.address),
        region_kind: source.describe(snapshot.address),
        regions: stats.regions,
        scanned_mb,
        duration_ms,
        sections: count_sections(&snapshot.bytes),
        saved_to: saved_to.display().to_string(),
    };
    {
        let mut st = shared.status.lock().unwrap();
        st.last_capture = Some(summary.clone());
        st.last_error = None;
    }
    let _ = app.emit("inventory-captured", &summary);
    emit_status(app, shared);
    Ok(summary)
}

/// Keeps the name from a "Logged in" line until the next capture says which account it belongs to.
/// At startup (`for_latest`), the name from the current log is also saved for the latest account
/// if it doesn't have one yet, so it shows without waiting for a new capture.
fn remember_login(app: &AppHandle, shared: &Shared, name: String, for_latest: bool) {
    if for_latest {
        if let Some(store) = shared.store.lock().unwrap().as_ref() {
            if let Ok(Some(account)) = store.latest_account() {
                if store.account_name(&account).ok().flatten().is_none() {
                    let _ = store.set_account_name(&account, &name);
                    let _ = app.emit("inventory-updated", ());
                }
            }
        }
    }
    *shared.login_name.lock().unwrap() = Some(name);
}

/// Translate + Update: parse the capture, store what changed, and tell the UI to refresh.
/// Problems here are logged, not fatal: the capture itself already succeeded.
fn store_capture(app: &AppHandle, shared: &Shared, raw: &[u8], captured_at: i64) {
    let inv = match translate::parse(raw) {
        Ok(inv) => inv,
        Err(e) => {
            eprintln!("[translate] {e}");
            return;
        }
    };
    let mut guard = shared.store.lock().unwrap();
    let Some(store) = guard.as_mut() else { return };
    if let Some(name) = shared.login_name.lock().unwrap().as_deref() {
        let _ = store.set_account_name(&inv.account, name);
    }
    match store.apply(&inv, raw, captured_at) {
        Ok(Outcome::Baseline { .. }) => eprintln!("[update] first capture for this account stored as the starting point"),
        Ok(Outcome::Changed { changes, .. }) => eprintln!("[update] {} changes stored", changes.len()),
        Ok(Outcome::Unchanged) => eprintln!("[update] no changes since the last capture"),
        Err(e) => {
            eprintln!("[update] {e}");
            return;
        }
    }
    drop(guard);
    let _ = app.emit("inventory-updated", ());
}

/// Writes the raw JSON to `<app data>/latest_inventory.json` (via a temp file, so a crash can't leave half a file).
fn save_snapshot(app: &AppHandle, snapshot: &capture::Snapshot) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("latest_inventory.json");
    let tmp = dir.join("latest_inventory.json.tmp");
    std::fs::write(&tmp, &snapshot.bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Saves rejected candidates to `<app data>/debug/` for inspection (once per address).
/// They hold account data like the inventory itself, so they stay local.
fn save_rejections(app: &AppHandle, rejections: &[capture::Rejection]) {
    if rejections.is_empty() {
        return;
    }
    let Ok(dir) = app.path().app_data_dir().map(|d| d.join("debug")) else { return };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    for r in rejections {
        let path = dir.join(format!("rejected_{:x}.bin", r.address));
        if !path.exists() {
            let _ = std::fs::write(&path, &r.sample);
            eprintln!("[capture] saved rejected candidate to {}", path.display());
        }
    }
}

fn count_sections(doc: &[u8]) -> Vec<(String, usize)> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(doc) else {
        return Vec::new();
    };
    SECTIONS
        .iter()
        .filter_map(|&k| value.get(k)?.as_array().map(|a| (k.to_string(), a.len())))
        .collect()
}

fn set_error(app: &AppHandle, shared: &Shared, message: String) {
    shared.status.lock().unwrap().last_error = Some(message);
    emit_status(app, shared);
}

fn emit_status(app: &AppHandle, shared: &Shared) {
    let status = shared.status.lock().unwrap().clone();
    let _ = app.emit("pipeline-status", &status);
}

#[tauri::command]
pub fn get_status(shared: State<'_, Arc<Shared>>) -> Status {
    shared.status.lock().unwrap().clone()
}

/// The "Capture now" button. Runs off the UI thread because a scan can take a few seconds.
#[tauri::command]
pub async fn capture_now(app: AppHandle) -> Result<CaptureSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let shared = app.state::<Arc<Shared>>().inner().clone();
        let result = capture_once(&app, &shared, FAST_SCOPE);
        match &result {
            Ok(s) => eprintln!("[capture] manual: found {} bytes at {} ({})", s.bytes, s.address, s.region_kind),
            Err(e) => {
                eprintln!("[capture] manual: {e}");
                set_error(&app, &shared, e.clone());
            }
        }
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
