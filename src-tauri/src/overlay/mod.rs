//! The riven overlay (Tools page, off by default). See `docs/plans/riven-overlay.md`.
//!
//! - **Chat-link rivens:** when the game logs a riven opening in the item-details dialog, read
//!   the card off the screen and show a panel at the left of the game. It hides when the dialog
//!   closes.
//! - **Reroll screen:** the old riven's panel on the left, each new roll on the right. Cycles
//!   and confirms come from the log; the cards are read off the screen.
//! - **Relic rewards** (its own toggle; `docs/plans/relic-rewards.md`): the squad's relics come
//!   from the log at mission start, and their rewards' prices are fetched in the background. On
//!   the reward screen the cards are read off the screen, a panel goes under each, and the
//!   shown items' prices are refreshed live.
//!
//! Read-only throughout: EE.log, a copy of the screen, and our own click-through window on top.

pub mod card;
pub mod events;
pub mod ocr;
pub mod relics;
pub mod screen;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::capture::{find_pid, GAME_EXE};
use crate::pipeline::Shared;
use crate::rivens::{self, RivenView};
use card::{parse_card, Card};
use crate::market::{Market, PriceKey, PriceStatus};
use crate::translate::{Flat, ItemIndex};
use events::{cycled_riven, is_confirm, ScreenEvent};
use relics::{Candidate, RewardView};
use screen::{GameWindow, Region};

pub const WINDOW: &str = "riven-overlay";
const SETTING: &str = "riven_overlay";
const RELIC_SETTING: &str = "relic_overlay";
/// Tools → Debug logging: write `overlay.log` and the `overlay-debug/` images (off by default).
const DEBUG_SETTING: &str = "debug_logging";
/// Price freshness for relic rewards: fetched ahead within a day; on the reward screen, live.
const PREFETCH_MAX_AGE_MS: i64 = 24 * 60 * 60 * 1000;
const LIVE_MAX_AGE_MS: i64 = 10 * 60 * 1000;
/// The reward screen lasts about 20 s; stop regardless after this.
const REWARDS_CAP: Duration = Duration::from_secs(45);
/// The reward screen always lasts its 5 s pause and 15 s choice (unless the log closes it
/// sooner): the screen check can't end it before this. The names band reads empty for a moment
/// while a card is being picked.
const REWARDS_MIN: Duration = Duration::from_secs(21);
/// A second mission name within this long is the same mission (cached name, then the host's).
const SAME_MISSION: Duration = Duration::from_secs(120);
/// A weapon load counts for the screen that opens within this long.
const WEAPON_WINDOW: Duration = Duration::from_secs(3);
/// A chat-link card's log lines (riven type, "Fits in" weapon) count if logged no earlier than
/// this before the card was seen. Earlier ones belong to the riven before it.
const LOG_LEAD: Duration = Duration::from_millis(1500);
/// How long a chat-link card whose numbers don't fit waits for the log (its riven type) before
/// the panel says "couldn't grade", when its weapon wasn't recognised. Otherwise no wait.
const WAIT_FOR_LOG: Duration = Duration::from_secs(3);

/// Card text areas, as fractions of the game's centred 16:9 area (measured on 2560×1440
/// screenshots: `docs/alecaExamples/Tools`).
// Starts above the title: cards with wrapped labels grow upwards (error2.png's title is at 0.46).
const INSPECT_CARD: Region = Region { x: 0.425, y: 0.41, w: 0.15, h: 0.24 };
const REROLL_CENTRE: Region = Region { x: 0.425, y: 0.570, w: 0.15, h: 0.230 };
const REROLL_LEFT: Region = Region { x: 0.248, y: 0.622, w: 0.135, h: 0.145 };
const REROLL_RIGHT: Region = Region { x: 0.622, y: 0.603, w: 0.135, h: 0.165 };
/// The chat-link screen's "Fits in" box (bottom right): the weapon, variant included, the card's
/// numbers are for. Its name is at y 0.85–0.88; the area starts higher in case it wraps.
const FITS_IN: Region = Region { x: 0.853, y: 0.80, w: 0.10, h: 0.09 };
/// A chat-link card is re-read (every 120 ms) until two reads in a row agree and it works out, at
/// most this many times. The first read often catches the card fading in ("Catabo/yst",
/// "+70.4%" for "+10.4%", lines missing).
const SETTLE_READS: u32 = 8;
/// Looks at the "Fits in" box this many times (250 ms apart) while it hasn't been read.
const FITS_IN_TRIES: u32 = 8;

static ENABLED: AtomicBool = AtomicBool::new(false);
static RELICS_ENABLED: AtomicBool = AtomicBool::new(false);
/// Debug logging (Tools page): only then are `overlay.log` and `overlay-debug/` written.
static DEBUG: AtomicBool = AtomicBool::new(false);
/// `<app data>/overlay.log`: what the overlay saw and did (cleared at each start). Written only
/// with debug logging on.
static LOG_FILE: OnceLock<std::path::PathBuf> = OnceLock::new();
/// `<app data>/overlay-debug/`: the last image read for each card area (overwritten each read).
/// Written only with debug logging on.
static DEBUG_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();

/// Where to save the image just read, if debug logging is on (the folder is made when needed).
fn debug_dir() -> Option<&'static std::path::PathBuf> {
    let dir = DEBUG_DIR.get().filter(|_| DEBUG.load(Ordering::Relaxed))?;
    let _ = std::fs::create_dir_all(dir);
    Some(dir)
}

/// Writes a line to the console, and to `overlay.log` when debug logging is on.
fn log(msg: &str) {
    eprintln!("[overlay] {msg}");
    if let Some(path) = LOG_FILE.get().filter(|_| DEBUG.load(Ordering::Relaxed)) {
        use std::io::Write;
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{ms} {msg}");
        }
    }
}

macro_rules! olog {
    ($($t:tt)*) => { log(&format!($($t)*)) };
}
static SENDER: OnceLock<Mutex<Sender<Msg>>> = OnceLock::new();
/// The latest state, for the overlay page when it (re)loads.
static STATE: Mutex<Option<OverlayState>> = Mutex::new(None);

enum Msg {
    Log(ScreenEvent),
    ToggleInteractive,
    StopInteractive,
    Preview,
    PreviewRelics,
    /// New riven data was swapped in (`riven_data`).
    DataUpdated,
}

/// Feeds an EE.log line to the overlay (called by the log watcher for every line).
pub fn on_log_line(line: &str) {
    if let Some(event) = events::parse(line) {
        send(Msg::Log(event));
    }
}

/// New riven data is in use: log what changed and work the shown riven out again.
pub fn riven_data_changed(changes: &[String]) {
    olog!("riven data updated ({} changes)", changes.len());
    for c in changes.iter().take(40) {
        olog!("  {c}");
    }
    send(Msg::DataUpdated);
}

fn send(msg: Msg) {
    if let Some(tx) = SENDER.get() {
        let _ = tx.lock().unwrap().send(msg);
    }
}

// --- What the overlay page shows ---

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Panel {
    /// "left" or "right".
    pub side: &'static str,
    /// "Riven", "Current", "New roll".
    pub role: &'static str,
    /// The card's title as read (shown before the riven is worked out).
    pub title: Option<String>,
    /// "reading", "fetching" (read, waiting for the log or new data), "ready" or "failed".
    pub status: &'static str,
    pub message: Option<String>,
    /// Shown under a ready riven, e.g. which variant's disposition was assumed.
    pub note: Option<String>,
    /// The stat lines as read, shown while fetching and when it couldn't be graded.
    pub read: Vec<String>,
    /// Why it couldn't be worked out (for overlay.log and the misfits file).
    #[serde(skip)]
    pub detail: Option<String>,
    pub riven: Option<RivenView>,
    /// Found in the user's inventory (exact rolls).
    pub own: bool,
    /// The rank was worked out from the numbers, not read.
    pub rank_assumed: bool,
}

impl Panel {
    fn reading(side: &'static str, role: &'static str) -> Self {
        Panel {
            side,
            role,
            title: None,
            status: "reading",
            message: None,
            note: None,
            read: Vec::new(),
            detail: None,
            riven: None,
            own: false,
            rank_assumed: false,
        }
    }
    fn failed(side: &'static str, role: &'static str, message: &str) -> Self {
        Panel { status: "failed", message: Some(message.to_string()), ..Panel::reading(side, role) }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OverlayState {
    pub visible: bool,
    pub interactive: bool,
    /// "inspect", "reroll", "preview" or "rewards".
    pub mode: String,
    pub panels: Vec<Panel>,
    /// Relic reward screen: one panel per card.
    pub rewards: Vec<RewardView>,
    /// The user's platinum and ducats (last inventory capture), for the rewards footer.
    pub platinum: Option<i64>,
    pub ducats: Option<i64>,
}

// --- Sessions ---

/// A riven worked out from the screen.
#[derive(Debug, Clone)]
struct Found {
    card: Card,
    panel: Panel,
}

#[derive(Debug)]
enum Session {
    /// A riven opened from a chat link (found on screen; see `watch_inspect`). `seen`: when the
    /// card was first read.
    Inspect { card: Card, panel: Panel, seen: Instant },
    /// The reroll screen.
    Reroll { weapon: Option<String>, riven_type: Option<String>, old: Option<Found>, new: Option<Found>, step: RerollStep },
    /// The Tools page's preview.
    Preview { until: Instant, panel: Panel },
    /// The relic reward screen (or its preview).
    Rewards(RewardsSession),
}

#[derive(Debug)]
struct RewardsSession {
    preview: bool,
    /// The user's platinum and ducats.
    wallet: (Option<i64>, Option<i64>),
    opened: Instant,
    /// The cards have been read.
    read: bool,
    attempts: u32,
    /// Looks in a row that found no reward cards (after they were read).
    misses: u32,
    next_check: Instant,
    views: Vec<RewardView>,
}

impl RewardsSession {
    fn new(preview: bool) -> Self {
        RewardsSession { preview, wallet: (None, None), opened: Instant::now(), read: false, attempts: 0, misses: 0, next_check: Instant::now(), views: Vec::new() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RerollStep {
    ReadingOld { attempts: u32 },
    ReadingNew { attempts: u32 },
    ReadingKept { attempts: u32 },
    /// Watching that the cards are still on screen.
    Watching { misses: u32 },
}

struct Controller {
    app: AppHandle,
    shared: Arc<Shared>,
    session: Option<Session>,
    last_weapon: Option<(String, Instant)>,
    last_dialog: Option<String>,
    next_action: Instant,
    interactive: bool,
    shown: bool,
    game: Option<GameWindow>,
    game_checked: Instant,
    shortcut_registered: bool,
    /// Last OCR text logged per card area (to log only changes).
    last_text: std::collections::HashMap<&'static str, String>,
    /// Chat-link rivens: the lilac signature of the card last read, reads in a row with no card,
    /// and when to look next.
    watch_sig: Option<Vec<u8>>,
    watch_misses: u32,
    next_watch: Instant,
    /// Riven type from the latest `PopulateInfo` line (the log lags, so it may come after the card).
    log_type: Option<(String, Instant)>,
    /// The shown chat-link card's weapon, read from its "Fits in" box, and how many looks so far.
    fits_in: Option<String>,
    fits_tries: u32,
    /// The shown chat-link card is still being re-read (see [`SETTLE_READS`]): a failure shows as
    /// "reading" until it settles.
    settling: bool,
    settle_reads: u32,
    /// Cards already written to `riven_misfits.jsonl` (by title).
    misfits: std::collections::HashSet<String>,
    /// `<app data>/market` (warframe.market's weapon list gives a weapon's riven type).
    market_dir: std::path::PathBuf,
    /// Void Fissure tracking: the mission's era, the squad's relics ("Lith W2", the user's
    /// included) as they load, and rewards the log named.
    mission_era: Option<&'static str>,
    squad_relics: Vec<String>,
    collecting_relics: bool,
    logged_rewards: Vec<String>,
    mission_started: Option<Instant>,
    /// A real reward screen is up (its open line was logged). The relic picker between endless
    /// rounds logs a close line without an open one.
    reward_screen_up: bool,
}

fn region_name(r: Region) -> &'static str {
    match (r.x, r.y) {
        (x, y) if x == INSPECT_CARD.x && y == INSPECT_CARD.y => "inspect",
        (x, y) if x == REROLL_CENTRE.x && y == REROLL_CENTRE.y => "reroll-centre",
        (x, _) if x == REROLL_LEFT.x => "reroll-left",
        (x, _) if x == FITS_IN.x => "fits-in",
        _ => "reroll-right",
    }
}

/// Creates the overlay window (hidden) and starts the controller thread. Call once from setup.
pub fn start(app: &AppHandle, shared: Arc<Shared>) {
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = LOG_FILE.set(dir.join("overlay.log"));
        let _ = DEBUG_DIR.set(dir.join("overlay-debug"));
    }
    let setting_on = |name: &str| {
        shared.store.lock().unwrap().as_ref().and_then(|s| s.setting(name).ok().flatten()).is_some_and(|v| v == "on")
    };
    ENABLED.store(setting_on(SETTING), Ordering::Relaxed);
    RELICS_ENABLED.store(setting_on(RELIC_SETTING), Ordering::Relaxed);
    set_debug(setting_on(DEBUG_SETTING));

    let built = WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::App("/overlay".into()))
        .title("Game overlay")
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focusable(false)
        .focused(false)
        .visible(false)
        .inner_size(400.0, 400.0)
        .build();
    match built {
        Ok(w) => {
            let _ = w.set_ignore_cursor_events(true);
        }
        Err(e) => olog!("couldn't create the overlay window: {e}"),
    }

    let (tx, rx) = channel();
    let _ = SENDER.set(Mutex::new(tx));
    let market_dir = app.path().app_data_dir().map(|d| d.join("market")).unwrap_or_default();
    let app = app.clone();
    std::thread::spawn(move || {
        Controller {
            app,
            shared,
            session: None,
            last_weapon: None,
            last_dialog: None,
            next_action: Instant::now(),
            interactive: false,
            shown: false,
            game: None,
            game_checked: Instant::now() - Duration::from_secs(10),
            shortcut_registered: false,
            last_text: std::collections::HashMap::new(),
            watch_sig: None,
            watch_misses: 0,
            next_watch: Instant::now(),
            log_type: None,
            fits_in: None,
            fits_tries: 0,
            settling: true,
            settle_reads: 0,
            misfits: std::collections::HashSet::new(),
            market_dir,
            mission_era: None,
            squad_relics: Vec::new(),
            collecting_relics: false,
            logged_rewards: Vec::new(),
            mission_started: None,
            reward_screen_up: false,
        }
        .run(rx)
    });
}

/// Ctrl+Tab while the overlay is showing: make it clickable (or click-through again).
pub fn shortcut_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|_app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                send(Msg::ToggleInteractive);
            }
        })
        .build()
}

fn interact_shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL), Code::Tab)
}

impl Controller {
    fn run(mut self, rx: Receiver<Msg>) {
        loop {
            let busy = self.session.is_some() || ENABLED.load(Ordering::Relaxed) || RELICS_ENABLED.load(Ordering::Relaxed);
            let wait = if busy { Duration::from_millis(50) } else { Duration::from_secs(1) };
            let msg = match rx.recv_timeout(wait) {
                Ok(m) => Some(m),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            // A bug in one step must not leave a panel stuck on screen: log it, drop the
            // session, and keep running.
            let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.tick(msg))).is_ok();
            if !ok {
                olog!("error while handling the overlay; panel dropped");
                self.session = None;
                self.publish();
                self.update_visibility();
            }
        }
    }

    fn tick(&mut self, msg: Option<Msg>) {
        match msg {
            Some(Msg::Log(e)) => {
                if !matches!(e, ScreenEvent::WeaponLoaded { .. }) {
                    olog!("log: {e:?}");
                }
                self.on_event(e)
            }
            Some(Msg::ToggleInteractive) => self.toggle_interactive(),
            Some(Msg::StopInteractive) => {
                self.set_interactive(false);
                self.publish();
            }
            Some(Msg::Preview) => self.start_preview(),
            Some(Msg::PreviewRelics) => self.start_relic_preview(),
            Some(Msg::DataUpdated) => self.reidentify_inspect(),
            None => {}
        }
        if self.session.is_some() && Instant::now() >= self.next_action {
            self.step();
        }
        let watching = ENABLED.load(Ordering::Relaxed)
            && matches!(self.session, None | Some(Session::Inspect { .. }));
        if watching && Instant::now() >= self.next_watch {
            self.watch_inspect();
            // Quicker while a riven is up or one was opened recently (browsing a riven list).
            let active = self.session.is_some() || self.log_type.as_ref().is_some_and(|(_, at)| at.elapsed() < Duration::from_secs(90));
            let next = Instant::now() + Duration::from_millis(if active { 120 } else { 250 });
            self.next_watch = self.next_watch.max(next); // keep a longer wait set by the look itself
        }
        self.update_visibility();
    }

    /// Chat-link rivens follow the screen, not the (lagging) log: copy the card area, and when
    /// lilac riven text appears or changes, read it and show the panel; when it's gone, hide.
    fn watch_inspect(&mut self) {
        let Some(game) = self.game() else { return };
        let front = screen::foreground();
        if front != game.hwnd && front != self.overlay_hwnd() {
            return;
        }
        let (x, y, w, h) = INSPECT_CARD.on(&game);
        let Some(shot) = screen::grab(x, y, w, h) else { return };
        let Some(sig) = screen::card_signature(&shot) else {
            self.watch_sig = None;
            self.settling = true;
            self.settle_reads = 0;
            self.card_missing("riven card gone");
            return;
        };
        if self.watch_sig.as_ref().is_some_and(|s| screen::signatures_match(s, &sig)) {
            self.watch_misses = 0;
            return; // same card as last read
        }
        let info = format!("{w}x{h} at {x},{y}; game {}x{}", game.width, game.height);
        let Some(card) = self.ocr("inspect", &shot, &info) else {
            // Lilac but not a card: a card fading in, or riven names in a chat list behind a
            // closed card ("Tenet Envoy Hexatron"). Counts as no card; look again a bit later.
            self.card_missing("no riven card (other lilac text)");
            self.next_watch = Instant::now() + Duration::from_millis(300);
            return;
        };
        self.watch_misses = 0;
        let same = matches!(&self.session, Some(Session::Inspect { card: shown, .. }) if shown.same_as(&card));
        let complete = card.mastery.is_some();
        if !same {
            // Another riven (browsing a list without the card closing): settle it afresh.
            let other = !matches!(&self.session, Some(Session::Inspect { card: shown, .. }) if card::norm(&shown.title) == card::norm(&card.title));
            if other && !self.settling {
                self.settling = true;
                self.settle_reads = 0;
            }
            self.show_inspected(card);
        }
        // Keep reading until two reads agree on a complete card that works out (or give up after
        // SETTLE_READS); only then stop re-reading this card and let a failure show.
        self.settle_reads += 1;
        let ready = matches!(&self.session, Some(Session::Inspect { panel, .. }) if panel.status == "ready");
        if (same && complete && ready) || self.settle_reads >= SETTLE_READS {
            self.watch_sig = Some(sig);
            self.settle_reads = 0;
            if std::mem::replace(&mut self.settling, false) && !ready {
                self.reidentify_inspect();
            }
        } else {
            self.watch_sig = None;
        }
    }

    /// No card in the inspect area: hide the chat-riven panel after two looks in a row.
    fn card_missing(&mut self, why: &str) {
        if matches!(self.session, Some(Session::Inspect { .. })) {
            self.watch_misses += 1;
            if self.watch_misses >= 2 {
                self.end(why);
            }
        }
    }

    /// A new chat-link card is on screen.
    fn show_inspected(&mut self, card: Card) {
        let seen = Instant::now();
        self.fits_in = None;
        self.fits_tries = 0;
        self.look_at_fits_in(&card);
        let panel = self.identify_inspected(&card, seen);
        olog!("read {} ({})", card.title, panel.status);
        self.session = Some(Session::Inspect { card, panel, seen });
        self.publish();
    }

    /// Works the shown chat-link card out again (the log named its type or weapon, new data
    /// arrived, or it's been waiting). Publishes only when the panel changed.
    fn reidentify_inspect(&mut self) {
        let Some(Session::Inspect { card, panel: old, seen }) = &self.session else { return };
        let key = |p: &Panel| (p.status, p.note.clone(), p.riven.as_ref().map(|v| (v.weapon.clone(), v.disposition, v.rank, v.kind.clone())));
        let (card, seen, before) = (card.clone(), *seen, key(old));
        let panel = self.identify_inspected(&card, seen);
        if key(&panel) == before {
            return;
        }
        olog!("read {} ({}{})", card.title, panel.status, panel.note.as_deref().map(|n| format!("; {n}")).unwrap_or_default());
        self.session = Some(Session::Inspect { card, panel, seen });
        self.publish();
    }

    /// Identifies a chat-link card with what the log has said about it so far. While the card is
    /// new it may come back "fetching" (waiting for the log); after [`WAIT_FOR_LOG`] it settles.
    fn identify_inspected(&mut self, card: &Card, seen: Instant) -> Panel {
        let weapon = self.weapon_for_card(seen);
        let types = self.type_candidates(card, weapon.as_deref(), Some(seen));
        // Waiting only helps when the weapon wasn't recognised (the log may still name the type);
        // otherwise the weapon's own type was already tried, so settle at once.
        let data = rivens::data();
        let weapon_kind = card::weapon_path(&data, card, weapon.as_deref()).is_some_and(|p| data.kind_of(&p).is_some());
        let wait = !weapon_kind && seen.elapsed() < WAIT_FOR_LOG;
        let mut panel = self.identify(card, &types, weapon.as_deref(), None, "left", "Riven", wait);
        if self.settling && panel.status != "ready" {
            // Likely a read of the card fading in: say "reading" while it's read again.
            panel.status = "reading";
            panel.message = None;
        } else if panel.status == "failed" {
            self.record_misfit(card, &panel, weapon.as_deref(), &types);
        }
        panel
    }

    /// The card's "Fits in" weapon: read off the screen, else as the log named it for a card
    /// seen at `seen` (not an earlier riven's).
    fn weapon_for_card(&self, seen: Instant) -> Option<String> {
        self.fits_in
            .clone()
            .or_else(|| self.last_weapon.as_ref().filter(|(_, at)| *at + LOG_LEAD >= seen).map(|(p, _)| p.clone()))
    }

    /// Reads the "Fits in" box when the card's weapon has variants with different dispositions
    /// and it hasn't been read yet. Returns whether it was read just now.
    fn look_at_fits_in(&mut self, card: &Card) -> bool {
        let data = rivens::data();
        let needed = self.fits_in.is_none()
            && self.fits_tries < FITS_IN_TRIES
            && card::weapon_path(&data, card, None).is_some_and(|p| data.variants_differ(&p));
        if !needed {
            return false;
        }
        self.fits_tries += 1;
        let Some(game) = self.game() else { return false };
        let (x, y, w, h) = FITS_IN.on(&game);
        let Some(shot) = screen::grab(x, y, w, h) else { return false };
        let info = format!("{w}x{h} at {x},{y}; game {}x{}", game.width, game.height);
        let Some(lines) = self.ocr_lines("fits-in", &shot, &info) else { return false };
        self.fits_in = card::fits_in_weapon(&data, card, &lines);
        if let Some(p) = &self.fits_in {
            olog!("fits in {}", data.weapons.get(p).map_or(p.as_str(), |w| w.name.as_str()));
        }
        self.fits_in.is_some()
    }

    /// Riven types to try for a card, most trusted first: the weapon's own type (from the riven
    /// data), the log's (chat-link cards only, when logged for this card), warframe.market's
    /// weapon list, then the type whose stat labels match best.
    fn type_candidates(&self, card: &Card, weapon: Option<&str>, seen: Option<Instant>) -> Vec<String> {
        let data = rivens::data();
        let path = card::weapon_path(&data, card, weapon);
        let mut out: Vec<String> = Vec::new();
        let mut add = |t: Option<String>| {
            if let Some(t) = t.filter(|t| !out.contains(t)) {
                out.push(t);
            }
        };
        add(path.as_deref().and_then(|p| data.kind_of(p)).map(String::from));
        if let (Some(seen), Some((t, at))) = (seen, &self.log_type) {
            if *at + LOG_LEAD >= seen {
                add(Some(t.clone()));
            }
        }
        add(path.as_deref().and_then(|p| crate::riven_market::riven_type_for(&self.market_dir, p)));
        add(guess_type(card));
        out
    }

    /// Appends a card that couldn't be worked out to `<app data>/riven_misfits.jsonl` (once per
    /// title), and checks for new riven data.
    fn record_misfit(&mut self, card: &Card, panel: &Panel, weapon: Option<&str>, types: &[String]) {
        if !self.misfits.insert(card::norm(&card.title)) {
            return;
        }
        crate::riven_data::refresh_after_misfit();
        let Some(path) = LOG_FILE.get().and_then(|p| p.parent()).map(|d| d.join("riven_misfits.jsonl")) else { return };
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
        let entry = serde_json::json!({
            "at": ms,
            "title": card.title,
            "ocr": self.last_text.get("inspect"),
            "statLines": card.stat_lines,
            "stats": card.stats.iter().map(|s| serde_json::json!({ "value": s.value, "unit": format!("{:?}", s.unit), "label": s.label })).collect::<Vec<_>>(),
            "mastery": card.mastery,
            "rerolls": card.rerolls,
            "weapon": weapon,
            "types": types,
            "dataBuilt": rivens::data().built,
            "why": panel.detail,
        });
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(f, "{entry}");
        }
    }

    fn overlay_hwnd(&self) -> isize {
        self.app.get_webview_window(WINDOW).and_then(|w| w.hwnd().ok()).map_or(0, |h| h.0 as isize)
    }

    fn recent_weapon(&self) -> Option<String> {
        self.last_weapon.as_ref().filter(|(_, at)| at.elapsed() < WEAPON_WINDOW).map(|(p, _)| p.clone())
    }

    fn on_event(&mut self, event: ScreenEvent) {
        let enabled = ENABLED.load(Ordering::Relaxed);
        match event {
            ScreenEvent::WeaponLoaded { path } => {
                self.last_weapon = Some((path, Instant::now()));
                // The "Fits in" weapon of a card waiting for it (or shown with an assumed one).
                if matches!(&self.session, Some(Session::Inspect { panel, .. }) if panel.status == "fetching" || panel.note.is_some()) {
                    self.reidentify_inspect();
                }
            }
            ScreenEvent::RivenInspected { riven_type } if enabled => {
                // The card is usually on screen already; the log names its type (and, just
                // before, its weapon). Work it out again with them.
                self.log_type = Some((riven_type, Instant::now()));
                self.reidentify_inspect();
                self.next_watch = Instant::now();
            }
            ScreenEvent::DetailsVisibility { .. } => {
                // A dialog opened or closed (possibly seconds ago): look at the screen now.
                self.watch_sig = None;
                self.next_watch = Instant::now();
            }
            ScreenEvent::RerollOpened if enabled => {
                olog!("reroll screen opened");
                self.session = Some(Session::Reroll {
                    weapon: self.recent_weapon(),
                    riven_type: None,
                    old: None,
                    new: None,
                    step: RerollStep::ReadingOld { attempts: 0 },
                });
                self.after(Duration::from_millis(600));
                self.publish();
            }
            ScreenEvent::ModScreenClosed => {
                if matches!(self.session, Some(Session::Reroll { .. })) {
                    self.end("mod screen closed");
                }
            }
            ScreenEvent::DialogAsked { text } => self.last_dialog = Some(text),
            ScreenEvent::MissionStarted { name } => {
                // Logged once or twice per mission (cached name, then the host's name): only reset
                // for a different mission so relics already seen are kept.
                let fissure = name.contains("Fissure") || name.contains("Void Storm");
                let era = relics::era_of(&name);
                let recent = self.mission_started.is_some_and(|t| t.elapsed() < SAME_MISSION);
                if !(self.collecting_relics && fissure && era == self.mission_era && recent) {
                    self.squad_relics.clear();
                    self.logged_rewards.clear();
                }
                self.mission_started = Some(Instant::now());
                self.mission_era = era;
                self.collecting_relics = fissure;
                if fissure {
                    olog!("fissure mission: {name}");
                }
            }
            ScreenEvent::SquadRelic { path } if self.collecting_relics => {
                let index = self.shared.items.read().unwrap().clone();
                if let Some(base) = relics::relic_base(&index, &path) {
                    if !self.squad_relics.contains(&base) {
                        olog!("squad relic {base}");
                        if RELICS_ENABLED.load(Ordering::Relaxed) {
                            self.prefetch(&index, &base);
                        }
                        self.squad_relics.push(base);
                    }
                }
            }
            ScreenEvent::RewardScreenOpened => {
                // The cards show ~5 s later (and the log can lag): start looking now.
                self.collecting_relics = false;
                self.reward_screen_up = true;
                if !RELICS_ENABLED.load(Ordering::Relaxed) {
                    return;
                }
                if !matches!(&self.session, Some(Session::Rewards(r)) if !r.preview) {
                    olog!("reward screen opened (relics: {:?})", self.squad_relics);
                    self.session = Some(Session::Rewards(RewardsSession::new(false)));
                    self.after(Duration::from_millis(300));
                    self.publish();
                }
            }
            ScreenEvent::RewardsReady if RELICS_ENABLED.load(Ordering::Relaxed) => {
                self.collecting_relics = false;
                if !matches!(&self.session, Some(Session::Rewards(r)) if !r.preview) {
                    self.session = Some(Session::Rewards(RewardsSession::new(false)));
                }
                if matches!(&self.session, Some(Session::Rewards(r)) if !r.read) {
                    self.after(Duration::ZERO);
                }
            }
            ScreenEvent::RewardLogged { path } => self.logged_rewards.push(path),
            ScreenEvent::RewardScreenClosed => {
                if matches!(&self.session, Some(Session::Rewards(r)) if !r.preview) {
                    self.end("reward screen closed");
                }
                // Endless fissures: the squad picks new relics for the next round (the game
                // loads each pick). Start over for them.
                if std::mem::take(&mut self.reward_screen_up) && self.mission_era.is_some() {
                    self.squad_relics.clear();
                    self.logged_rewards.clear();
                    self.collecting_relics = true;
                }
            }
            ScreenEvent::DialogAnswered { result: 4 } => {
                let Some(text) = self.last_dialog.take() else { return };
                if let Some(name) = cycled_riven(&text) {
                    self.on_cycle(name.to_string(), enabled);
                } else if is_confirm(&text) {
                    if let Some(Session::Reroll { step, .. }) = &mut self.session {
                        *step = RerollStep::ReadingKept { attempts: 0 };
                        self.next_action = Instant::now() + Duration::from_millis(900);
                    }
                }
            }
            ScreenEvent::DialogAnswered { .. } => self.last_dialog = None,
            _ => {}
        }
    }

    /// A cycle was paid for: the riven named in the dialog is the old one; read the new roll.
    fn on_cycle(&mut self, title: String, enabled: bool) {
        if self.session.is_none() && enabled {
            // Started reading mid-screen (app opened while rerolling).
            self.session = Some(Session::Reroll {
                weapon: self.recent_weapon(),
                riven_type: None,
                old: None,
                new: None,
                step: RerollStep::ReadingNew { attempts: 0 },
            });
        }
        let own = self.lookup_own_by_title(&title);
        let Some(Session::Reroll { old, new, step, riven_type, .. }) = &mut self.session else { return };
        let stale = old.as_ref().is_none_or(|o| card::norm(&o.card.title) != card::norm(&title));
        if stale {
            if let Some((found, kind)) = own {
                *riven_type = Some(kind);
                *old = Some(found);
            }
        }
        *new = None;
        *step = RerollStep::ReadingNew { attempts: 0 };
        self.next_action = Instant::now() + Duration::from_millis(900);
        self.publish();
    }

    fn after(&mut self, d: Duration) {
        self.next_action = Instant::now() + d;
    }

    fn end(&mut self, why: &str) {
        olog!("hidden ({why})");
        self.session = None;
        self.watch_sig = None;
        self.watch_misses = 0;
        self.set_interactive(false);
        self.publish();
    }

    fn game(&mut self) -> Option<GameWindow> {
        if self.game_checked.elapsed() > Duration::from_secs(2) || self.game.is_none() {
            self.game_checked = Instant::now();
            self.game = find_pid(GAME_EXE).and_then(screen::find_game_window);
        }
        self.game
    }

    /// OCR one card area. `None` if the game isn't found or no card is there.
    fn read(&mut self, region: Region) -> Option<Card> {
        let Some(game) = self.game() else {
            olog!("read: game window not found");
            return None;
        };
        let (x, y, w, h) = region.on(&game);
        let Some(shot) = screen::grab(x, y, w, h) else {
            olog!("read: screen copy failed at {x},{y} {w}x{h}");
            return None;
        };
        let info = format!("{w}x{h} at {x},{y}; game {}x{}", game.width, game.height);
        self.ocr(region_name(region), &shot, &info)
    }

    /// OCRs a screen copy (upscaled 2x) into a card. Saves the image to `overlay-debug` and logs
    /// the text when it changes.
    fn ocr(&mut self, name: &'static str, shot: &screen::Image, info: &str) -> Option<Card> {
        self.ocr_lines(name, shot, info).and_then(|lines| parse_card(&lines))
    }

    /// OCRs a screen copy (upscaled 2x) into lines, saving the image and logging changed text.
    fn ocr_lines(&mut self, name: &'static str, shot: &screen::Image, info: &str) -> Option<Vec<String>> {
        let img = screen::upscale2(shot);
        if let Some(dir) = debug_dir() {
            let _ = screen::save_bmp(&img, &dir.join(format!("{name}.bmp")));
        }
        match ocr::read_lines(&img) {
            Ok(lines) => {
                let text = lines.join(" / ");
                if self.last_text.get(name) != Some(&text) {
                    olog!("ocr {name} ({info}): {}", if text.is_empty() { "(nothing)" } else { &text });
                    self.last_text.insert(name, text);
                }
                Some(lines)
            }
            Err(e) => {
                olog!("OCR: {e}");
                None
            }
        }
    }

    /// Works a card out: the user's own riven if it matches, else rebuilt from the numbers with
    /// each candidate type until one fits. With `wait`, a card that doesn't fit yet comes back
    /// "fetching" (the log may still name its type) instead of failing.
    #[allow(clippy::too_many_arguments)]
    fn identify(
        &self,
        card: &Card,
        types: &[String],
        weapon: Option<&str>,
        rank: Option<u32>,
        side: &'static str,
        role: &'static str,
        wait: bool,
    ) -> Panel {
        let data = rivens::data();
        let data = &*data;
        let sheet = self.shared.riven_sheet.read().unwrap().clone();
        let mut panel = Panel::reading(side, role);
        panel.title = Some(card.title.clone());
        panel.read = card.stat_lines.clone();
        // A weapon whose variants differ (Latron 1.4, Latron Prime 1.3) and the log hasn't said
        // which: show the base weapon's numbers now, with a note. The log often names the
        // variant 10+ s later, and the panel is corrected then.
        let variant_unknown = card::variant_unknown(data, card, weapon);
        let own = rivens::owned_rivens(&self.shared).map(|(o, _)| o).unwrap_or_default();
        let mut errors: Vec<String> = Vec::new();
        if let Some(view) = card::find_own(data, &sheet, &own, card, weapon) {
            panel.own = true;
            panel.riven = Some(view);
        } else {
            for kind in types {
                match card::rebuild(data, &sheet, kind, card, weapon, rank) {
                    Ok(r) => {
                        panel.rank_assumed = r.rank_assumed;
                        panel.riven = Some(r.view);
                        break;
                    }
                    Err(why) => errors.push(format!("{kind}: {why}")),
                }
            }
            if types.is_empty() {
                errors.push("riven type unknown".into());
            }
        }
        match &panel.riven {
            Some(v) => {
                panel.status = "ready";
                if variant_unknown {
                    panel.note = Some(format!(
                        "Numbers for {} (disposition {}); the game didn't say which variant it's shown with",
                        v.weapon, v.disposition
                    ));
                }
            }
            None if wait => panel.status = "fetching",
            None => {
                let detail = errors.join("; ");
                olog!("couldn't work out {}: {detail} (weapon {weapon:?}, stats {:?})", card.title, card.stats);
                panel.status = "failed";
                panel.message = Some(format!("Couldn't grade {}: {}", card.title, plain_reason(errors.first().map(String::as_str))));
                panel.detail = Some(detail);
            }
        }
        panel
    }

    /// Riven types to try at the reroll screen: the current riven's, then the usual candidates.
    fn reroll_types(&self, card: &Card, known: Option<&str>, weapon: Option<&str>) -> Vec<String> {
        let mut types: Vec<String> = known.map(String::from).into_iter().collect();
        for t in self.type_candidates(card, weapon, None) {
            if !types.contains(&t) {
                types.push(t);
            }
        }
        types
    }

    /// The user's riven with this title (for the reroll screen, where it's always theirs).
    fn lookup_own_by_title(&self, title: &str) -> Option<(Found, String)> {
        let data = rivens::data();
        let data = &*data;
        let sheet = self.shared.riven_sheet.read().unwrap().clone();
        let own = rivens::owned_rivens(&self.shared).ok()?.0;
        let weapon = self.recent_weapon().or_else(|| match &self.session {
            Some(Session::Reroll { weapon, .. }) => weapon.clone(),
            _ => None,
        });
        let named = Card { title: title.to_string(), ..Card::empty() };
        let disposition = card::shown_disposition(data, &named, weapon.as_deref());
        let want = card::norm(title);
        let (r, view) = own.iter().find_map(|r| {
            let v = rivens::view_with(data, &sheet, r, rivens::own_rank(r), disposition)?;
            (card::norm(&format!("{} {}", v.weapon, v.name)) == want).then_some((r, v))
        })?;
        let kind = r.item_type.rsplit('/').next().unwrap_or_default().to_string();
        let mut panel = Panel::reading("left", "Current");
        panel.status = "ready";
        panel.own = true;
        // The card as the game shows it, for telling it apart from the new roll.
        let stats = view.stats.iter().filter_map(|s| card::parse_stat(&s.text)).collect();
        panel.riven = Some(view);
        Some((Found { card: Card { title: title.to_string(), stats, ..Card::empty() }, panel }, kind))
    }

    /// Runs the session's next action. Each step returns the session to keep (or `None` to end
    /// it) and whether the panels changed.
    fn step(&mut self) {
        let Some(session) = self.session.take() else { return };
        let (next, changed) = match session {
            Session::Preview { until, panel } => {
                self.after(Duration::from_millis(250));
                if Instant::now() >= until { (None, true) } else { (Some(Session::Preview { until, panel }), false) }
            }
            Session::Inspect { card, panel, seen } => {
                // Waiting for the log, or the "Fits in" box not read yet: look again (and settle
                // once the wait is over).
                let fetching = panel.status == "fetching";
                let fits_read = self.look_at_fits_in(&card);
                let looking = self.fits_in.is_none() && self.fits_tries > 0 && self.fits_tries < FITS_IN_TRIES;
                self.session = Some(Session::Inspect { card, panel, seen });
                if fetching || fits_read {
                    self.reidentify_inspect();
                }
                self.after(Duration::from_millis(if fetching || looking { 250 } else { 1000 }));
                (self.session.take(), false)
            }
            Session::Reroll { weapon, riven_type, old, new, step } => self.reroll_step(weapon, riven_type, old, new, step),
            Session::Rewards(r) => self.rewards_step(r),
        };
        self.session = next;
        if self.session.is_none() {
            olog!("hidden (session over)");
            self.set_interactive(false);
        }
        if changed {
            self.publish();
        }
    }

    fn reroll_step(
        &mut self,
        weapon: Option<String>,
        mut riven_type: Option<String>,
        mut old: Option<Found>,
        mut new: Option<Found>,
        step: RerollStep,
    ) -> (Option<Session>, bool) {
        let rank = old.as_ref().and_then(|o| o.panel.riven.as_ref()).map(|v| v.rank);
        let (next, changed) = match step {
            RerollStep::ReadingOld { attempts } => match self.read(REROLL_CENTRE) {
                Some(card) => {
                    let types = self.reroll_types(&card, riven_type.as_deref(), weapon.as_deref());
                    let panel = self.identify(&card, &types, weapon.as_deref(), None, "left", "Current", false);
                    if let Some(v) = &panel.riven {
                        riven_type = Some(v.item_type.rsplit('/').next().unwrap_or_default().to_string());
                    }
                    olog!("reroll: current {} ({})", card.title, panel.status);
                    old = Some(Found { card, panel });
                    (RerollStep::Watching { misses: 0 }, true)
                }
                None if attempts >= 16 => (RerollStep::Watching { misses: 0 }, true),
                None => (RerollStep::ReadingOld { attempts: attempts + 1 }, false),
            },
            RerollStep::ReadingNew { attempts } => {
                // After a cycle the new roll is centred and the old one dimmed at the left (or the
                // other way round if the player switched back): take the card that isn't the old one.
                let is_old = |c: &Card| old.as_ref().is_some_and(|o| c.same_as(&o.card));
                let fresh = [REROLL_CENTRE, REROLL_RIGHT, REROLL_LEFT]
                    .into_iter()
                    .filter_map(|r| self.read(r))
                    .find(|c| !is_old(c));
                match fresh {
                    Some(card) => {
                        let types = self.reroll_types(&card, riven_type.as_deref(), weapon.as_deref());
                        let panel = self.identify(&card, &types, weapon.as_deref(), rank, "right", "New roll", false);
                        olog!("reroll: new {} ({})", card.title, panel.status);
                        new = Some(Found { card, panel });
                        (RerollStep::Watching { misses: 0 }, true)
                    }
                    None if attempts >= 20 => {
                        let panel = Panel::failed("right", "New roll", "Couldn't read the new roll");
                        new = Some(Found { card: Card::empty(), panel });
                        (RerollStep::Watching { misses: 0 }, true)
                    }
                    None => (RerollStep::ReadingNew { attempts: attempts + 1 }, false),
                }
            }
            RerollStep::ReadingKept { attempts } => match self.read(REROLL_CENTRE) {
                Some(card) => {
                    // The centred card is the one kept.
                    if let Some(mut kept) = new.take().filter(|n| n.card.same_as(&card)) {
                        kept.panel.side = "left";
                        kept.panel.role = "Current";
                        old = Some(kept);
                    }
                    (RerollStep::Watching { misses: 0 }, true)
                }
                None if attempts >= 12 => {
                    new = None;
                    (RerollStep::Watching { misses: 0 }, true)
                }
                None => (RerollStep::ReadingKept { attempts: attempts + 1 }, false),
            },
            RerollStep::Watching { misses } => {
                // Backing out to the riven list isn't logged: end when no card is on screen.
                if self.read(REROLL_CENTRE).is_some() {
                    (RerollStep::Watching { misses: 0 }, false)
                } else if misses + 1 >= 3 {
                    self.after(Duration::from_millis(500));
                    return (None, true);
                } else {
                    (RerollStep::Watching { misses: misses + 1 }, false)
                }
            }
        };
        let wait = if matches!(next, RerollStep::Watching { .. }) { 500 } else { 250 };
        self.after(Duration::from_millis(wait));
        (Some(Session::Reroll { weapon, riven_type, old, new, step: next }), changed)
    }

    fn rewards_step(&mut self, mut r: RewardsSession) -> (Option<Session>, bool) {
        if r.preview {
            if r.opened.elapsed() > Duration::from_secs(12) {
                return (None, true);
            }
            let before = r.views.clone();
            self.fill_prices(&mut r.views, PREFETCH_MAX_AGE_MS);
            self.after(Duration::from_millis(800));
            let changed = before != r.views;
            return (Some(Session::Rewards(r)), changed);
        }
        if r.opened.elapsed() > REWARDS_CAP {
            olog!("hidden (reward screen time limit)");
            return (None, true);
        }
        if !r.read {
            match self.read_rewards() {
                Some(cards) => {
                    let (index, current) = self.inventory();
                    let game = self.game().unwrap_or(GameWindow { hwnd: 0, x: 0, y: 0, width: 1920, height: 1080 });
                    r.views = relics::views(&index, &current, &game, &cards);
                    r.wallet = wallet(&current);
                    self.fill_prices(&mut r.views, LIVE_MAX_AGE_MS);
                    olog!("rewards: {}", r.views.iter().map(|v| v.name.as_str()).collect::<Vec<_>>().join(", "));
                    r.read = true;
                    r.next_check = Instant::now() + Duration::from_millis(1500);
                    self.after(Duration::from_millis(800));
                    return (Some(Session::Rewards(r)), true);
                }
                None => {
                    r.attempts += 1;
                    self.after(Duration::from_millis(300));
                    return (Some(Session::Rewards(r)), false);
                }
            }
        }
        let before = r.views.clone();
        self.fill_prices(&mut r.views, LIVE_MAX_AGE_MS);
        if Instant::now() >= r.next_check && r.opened.elapsed() >= REWARDS_MIN {
            // The log's close line ends the session. As a backup, once the screen's time is up,
            // end when the names band has had no card text for three looks in a row. Any text in two or more columns counts
            // as the screen still being up: card art can garble a name so it doesn't match.
            r.next_check = Instant::now() + Duration::from_millis(1500);
            if self.read_columns().is_some_and(|cols| cols.len() >= 2) {
                r.misses = 0;
            } else {
                r.misses += 1;
                if r.misses >= 3 {
                    olog!("hidden (reward cards gone)");
                    return (None, true);
                }
            }
        }
        self.after(Duration::from_millis(800));
        let changed = before != r.views;
        (Some(Session::Rewards(r)), changed)
    }

    /// WFCD items and the latest inventory capture.
    fn inventory(&self) -> (Arc<ItemIndex>, Flat) {
        let index = self.shared.items.read().unwrap().clone();
        let guard = self.shared.store.lock().unwrap();
        let current = guard
            .as_ref()
            .and_then(|s| {
                let account = s.latest_account().ok().flatten()?;
                s.current(&account).ok()
            })
            .unwrap_or_default();
        (index, current)
    }

    /// What the reward cards can be: the squad's relics' rewards (plus any reward the log
    /// named); the era's relics when no squad relic was seen.
    fn reward_candidates(&self, index: &ItemIndex) -> (Vec<Candidate>, Vec<Candidate>) {
        let bases = &self.squad_relics;
        let wide = relics::candidates(index, &[], self.mission_era);
        if bases.is_empty() {
            return (wide, Vec::new());
        }
        let mut narrow = relics::candidates(index, bases, None);
        for path in &self.logged_rewards {
            if !narrow.iter().any(|c| &c.path == path) {
                narrow.push(Candidate { name: index.display_name(path), path: path.clone(), rarity: "common" });
            }
        }
        (narrow, wide)
    }

    /// Reads the reward cards: each column's name matched to a candidate. `None` unless at least
    /// two cards (the screen never shows one) all matched.
    fn read_rewards(&mut self) -> Option<Vec<(f64, Candidate)>> {
        let cols = self.read_columns()?;
        if cols.len() < 2 {
            return None;
        }
        let index = self.shared.items.read().unwrap().clone();
        let (narrow, wide) = self.reward_candidates(&index);
        let mut out = Vec::new();
        for c in &cols {
            let found = relics::snap(&c.text, &narrow).or_else(|| relics::snap(&c.text, &wide))?;
            out.push((c.centre, found.clone()));
        }
        Some(out)
    }

    /// OCRs the reward names band into card columns (logged when the text changes).
    fn read_columns(&mut self) -> Option<Vec<relics::Column>> {
        let game = self.game()?;
        let (x, y, w, h) = relics::NAMES.on(&game);
        let shot = screen::grab(x, y, w, h)?;
        let img = screen::upscale2(&shot);
        if let Some(dir) = debug_dir() {
            let _ = screen::save_bmp(&img, &dir.join("rewards.bmp"));
        }
        let words = match ocr::read_words(&img) {
            Ok(w) => w,
            Err(e) => {
                olog!("OCR: {e}");
                return None;
            }
        };
        let cols = relics::columns(&words, f64::from(img.width));
        let text = cols.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join(" | ");
        if self.last_text.get("rewards") != Some(&text) {
            olog!("ocr rewards ({w}x{h} at {x},{y}; game {}x{}): {}", game.width, game.height, if text.is_empty() { "(nothing)" } else { &text });
            self.last_text.insert("rewards", text);
        }
        Some(cols)
    }

    fn market(&self) -> Option<Arc<Market>> {
        self.app.try_state::<Arc<Market>>().map(|m| m.inner().clone())
    }

    /// Queues a relic's rewards' prices (a day's freshness) while the mission plays.
    fn prefetch(&self, index: &ItemIndex, base: &str) {
        let Some(market) = self.market() else { return };
        let keys = index.relic_rewards(base).iter().map(|r| PriceKey { path: r.path.clone(), rank: 0 }).collect();
        market.request_within(&self.shared, keys, PREFETCH_MAX_AGE_MS);
    }

    /// Fills in the panels' prices from the market cache (queueing anything older than
    /// `max_age_ms`).
    fn fill_prices(&self, views: &mut [RewardView], max_age_ms: i64) {
        let Some(market) = self.market() else { return };
        // Each card's item, then each card's set (when it's part of one).
        let mut keys: Vec<PriceKey> = views.iter().map(|v| PriceKey { path: v.path.clone(), rank: 0 }).collect();
        keys.extend(views.iter().filter_map(|v| v.set_path.clone()).map(|path| PriceKey { path, rank: 0 }));
        let prices = market.request_within(&self.shared, keys, max_age_ms);
        let (items, sets) = prices.split_at(views.len());
        let mut sets = sets.iter();
        for v in views.iter_mut() {
            if v.set_path.is_some() {
                v.set_plat = sets.next().and_then(|p| p.wts);
            }
        }
        for (v, p) in views.iter_mut().zip(items.iter()) {
            v.plat = p.wts;
            v.price_status = match p.status {
                PriceStatus::Fresh => "fresh",
                PriceStatus::Stale => "stale",
                PriceStatus::Loading => "loading",
                PriceStatus::Untradable => "untradable",
            }
            .into();
            if v.slug.is_none() {
                v.slug = market.slug_for(&v.path);
            }
        }
    }

    /// The Tools page's relic preview: four rewards of one relic, laid out like the screen.
    fn start_relic_preview(&mut self) {
        let (index, current) = self.inventory();
        let mut relic_list: Vec<(&str, &[crate::translate::RelicReward])> = index.relics().filter(|(_, r)| r.len() >= 4).collect();
        relic_list.sort_by_key(|(name, _)| (*name != "Lith W2", name.to_string()));
        let Some((name, rewards)) = relic_list.first().copied() else {
            olog!("relic preview: no relic data yet");
            return;
        };
        let pitch = 0.1263;
        let cards: Vec<(f64, Candidate)> = rewards
            .iter()
            .take(4)
            .enumerate()
            .map(|(i, r)| {
                let centre = 0.5 + (i as f64 - 1.5) * pitch;
                (centre, relics::candidates(&index, &[name.to_string()], None).into_iter().find(|c| c.path == r.path).unwrap())
            })
            .collect();
        let game = self.game().or_else(|| main_monitor(&self.app)).unwrap_or(GameWindow { hwnd: 0, x: 0, y: 0, width: 1920, height: 1080 });
        let mut session = RewardsSession::new(true);
        session.views = relics::views(&index, &current, &game, &cards);
        session.wallet = wallet(&current);
        session.read = true;
        self.fill_prices(&mut session.views, PREFETCH_MAX_AGE_MS);
        self.session = Some(Session::Rewards(session));
        self.after(Duration::from_millis(800));
        self.publish();
    }

    fn start_preview(&mut self) {
        let data = rivens::data();
        let data = &*data;
        let sheet = self.shared.riven_sheet.read().unwrap().clone();
        let own = rivens::owned_rivens(&self.shared).map(|(o, _)| o).unwrap_or_default();
        let mut views: Vec<RivenView> = own.iter().filter_map(|r| rivens::view(data, &sheet, r, rivens::own_rank(r))).collect();
        views.sort_by_key(|v| match v.grade.as_deref() {
            Some("Great") => 0,
            Some("Good") => 1,
            _ => 2,
        });
        let panel = match views.into_iter().next() {
            Some(v) => Panel {
                status: "ready",
                riven: Some(v),
                own: true,
                ..Panel::reading("left", "Preview")
            },
            None => Panel::failed("left", "Preview", "No rivens captured yet"),
        };
        self.session = Some(Session::Preview { until: Instant::now() + Duration::from_secs(12), panel });
        self.after(Duration::from_millis(250));
        self.publish();
    }

    fn state(&self) -> OverlayState {
        let (rewards, (platinum, ducats)) = match &self.session {
            Some(Session::Rewards(r)) => (r.views.clone(), r.wallet),
            _ => (Vec::new(), (None, None)),
        };
        let (mode, panels) = match &self.session {
            None => ("", Vec::new()),
            Some(Session::Rewards(_)) => ("rewards", Vec::new()),
            Some(Session::Inspect { panel, .. }) => ("inspect", vec![panel.clone()]),
            Some(Session::Preview { panel, .. }) => ("preview", vec![panel.clone()]),
            Some(Session::Reroll { old, new, step, .. }) => {
                let mut panels = Vec::new();
                match old {
                    Some(o) => panels.push(o.panel.clone()),
                    None => panels.push(Panel::reading("left", "Current")),
                }
                match (new, step) {
                    (Some(n), _) => panels.push(n.panel.clone()),
                    (None, RerollStep::ReadingNew { .. }) => panels.push(Panel::reading("right", "New roll")),
                    _ => {}
                }
                ("reroll", panels)
            }
        };
        OverlayState { visible: self.shown, interactive: self.interactive, mode: mode.to_string(), panels, rewards, platinum, ducats }
    }

    fn publish(&mut self) {
        let state = self.state();
        *STATE.lock().unwrap() = Some(state.clone());
        let _ = self.app.emit_to(WINDOW, "riven-overlay-state", state);
    }

    /// Shows the window over the game while a session is open and the game (or the overlay,
    /// when clickable) is in front; hides it otherwise.
    fn update_visibility(&mut self) {
        let Some(window) = self.app.get_webview_window(WINDOW) else { return };
        let preview = matches!(&self.session, Some(Session::Preview { .. })) || matches!(&self.session, Some(Session::Rewards(r)) if r.preview);
        let target = if preview { self.game().or_else(|| main_monitor(&self.app)) } else { self.game() };
        let overlay_hwnd = self.overlay_hwnd();
        let front = screen::foreground();
        let in_front = target.is_some_and(|g| g.hwnd == front) || front == overlay_hwnd || preview;
        let want = self.session.is_some() && target.is_some() && (in_front || self.interactive);

        if want {
            let g = target.unwrap();
            let _ = window.set_position(PhysicalPosition::new(g.x, g.y));
            let _ = window.set_size(PhysicalSize::new(g.width.max(1) as u32, g.height.max(1) as u32));
        }
        if want != self.shown {
            olog!("window {} (session: {}, game found: {}, in front: {in_front})", if want { "shown" } else { "hidden" }, self.session.is_some(), target.is_some());
            self.shown = want;
            if want {
                let _ = window.show();
                self.register_shortcut(true);
            } else {
                let _ = window.hide();
                self.set_interactive(false);
                self.register_shortcut(false);
            }
            self.publish();
        }
    }

    fn register_shortcut(&mut self, on: bool) {
        if on == self.shortcut_registered {
            return;
        }
        let gs = self.app.global_shortcut();
        let result = if on { gs.register(interact_shortcut()) } else { gs.unregister(interact_shortcut()) };
        match result {
            Ok(()) => self.shortcut_registered = on,
            Err(e) => olog!("Ctrl+Tab: {e}"),
        }
    }

    fn toggle_interactive(&mut self) {
        if self.shown {
            let on = !self.interactive;
            self.set_interactive(on);
            self.publish();
        }
    }

    fn set_interactive(&mut self, on: bool) {
        if self.interactive == on {
            return;
        }
        self.interactive = on;
        let Some(window) = self.app.get_webview_window(WINDOW) else { return };
        let _ = window.set_ignore_cursor_events(!on);
        let _ = window.set_focusable(on);
        if on {
            let _ = window.set_focus();
        } else if let Some(g) = self.game {
            screen::focus(&g);
        }
    }
}

/// The user's platinum and ducats from an inventory capture.
fn wallet(current: &Flat) -> (Option<i64>, Option<i64>) {
    let get = |key: &str| current.get(&(crate::translate::CURRENCY.to_string(), key.to_string())).copied();
    (get("PremiumCredits"), get("PrimeTokens"))
}

/// The panel's wording for why a card couldn't be worked out (the first, most trusted type's
/// reason; the full detail goes to overlay.log).
fn plain_reason(first: Option<&str>) -> String {
    let why = first.map_or("", |e| e.split_once(": ").map_or(e, |(_, w)| w));
    if why.contains("don't fit") {
        "its numbers don't match the game data".into()
    } else if why.starts_with("unknown weapon") {
        format!("the weapon wasn't recognised ({})", why.trim_start_matches("unknown weapon "))
    } else if why.is_empty() || why.contains("riven type unknown") {
        "the riven type isn't known".into()
    } else {
        why.to_string()
    }
}

/// The riven type for a card when the log didn't say: the type whose stat labels match best.
fn guess_type(card: &Card) -> Option<String> {
    let data = rivens::data();
    let data = &*data;
    data.types
        .iter()
        .max_by_key(|(_, kind)| card.stats.iter().filter(|s| card::match_label(kind, &s.label).is_some()).count())
        .map(|(k, _)| k.clone())
}

/// Where the preview shows when the game isn't running: the main window's monitor.
fn main_monitor(app: &AppHandle) -> Option<GameWindow> {
    let m = app.get_webview_window("main")?.current_monitor().ok()??;
    let (p, s) = (m.position(), m.size());
    Some(GameWindow { hwnd: 0, x: p.x, y: p.y, width: s.width as i32, height: s.height as i32 })
}

// --- Commands for the Tools page and the overlay page ---

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSettings {
    pub riven_overlay: bool,
    pub relic_overlay: bool,
    pub debug_logging: bool,
}

#[tauri::command]
pub fn get_tool_settings() -> ToolSettings {
    ToolSettings {
        riven_overlay: ENABLED.load(Ordering::Relaxed),
        relic_overlay: RELICS_ENABLED.load(Ordering::Relaxed),
        debug_logging: DEBUG.load(Ordering::Relaxed),
    }
}

/// Turns debug logging on (a fresh `overlay.log`) or off (the log and the saved images are
/// removed, so nothing is left behind).
fn set_debug(on: bool) {
    DEBUG.store(on, Ordering::Relaxed);
    if let Some(path) = LOG_FILE.get() {
        if on {
            let _ = std::fs::write(path, "");
        } else {
            let _ = std::fs::remove_file(path);
        }
    }
    if !on {
        if let Some(dir) = DEBUG_DIR.get() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[tauri::command]
pub fn set_debug_logging(enabled: bool, shared: State<'_, Arc<Shared>>) -> Result<ToolSettings, String> {
    if let Some(store) = shared.store.lock().unwrap().as_ref() {
        store.set_setting(DEBUG_SETTING, if enabled { "on" } else { "off" }).map_err(|e| e.to_string())?;
    }
    set_debug(enabled);
    olog!("debug logging {}", if enabled { "on" } else { "off" });
    Ok(get_tool_settings())
}

/// The folder debug files are written to (for the Tools page).
#[tauri::command]
pub fn debug_folder() -> Option<String> {
    LOG_FILE.get().and_then(|p| p.parent()).map(|d| d.display().to_string())
}

#[tauri::command]
pub fn set_relic_overlay(enabled: bool, shared: State<'_, Arc<Shared>>) -> Result<ToolSettings, String> {
    if let Some(store) = shared.store.lock().unwrap().as_ref() {
        store.set_setting(RELIC_SETTING, if enabled { "on" } else { "off" }).map_err(|e| e.to_string())?;
    }
    RELICS_ENABLED.store(enabled, Ordering::Relaxed);
    olog!("relic rewards overlay {}", if enabled { "on" } else { "off" });
    Ok(get_tool_settings())
}

#[tauri::command]
pub fn preview_relic_overlay() {
    send(Msg::PreviewRelics);
}

#[tauri::command]
pub fn set_riven_overlay(enabled: bool, shared: State<'_, Arc<Shared>>) -> Result<ToolSettings, String> {
    if let Some(store) = shared.store.lock().unwrap().as_ref() {
        store.set_setting(SETTING, if enabled { "on" } else { "off" }).map_err(|e| e.to_string())?;
    }
    ENABLED.store(enabled, Ordering::Relaxed);
    olog!("riven overlay {}", if enabled { "on" } else { "off" });
    Ok(get_tool_settings())
}

#[tauri::command]
pub fn preview_riven_overlay() {
    send(Msg::Preview);
}

#[tauri::command]
pub fn overlay_state() -> OverlayState {
    STATE.lock().unwrap().clone().unwrap_or_default()
}

/// The overlay page's "Done" button: back to click-through.
#[tauri::command]
pub fn overlay_done() {
    send(Msg::StopInteractive);
}

#[cfg(test)]
mod screenshot_tests {
    use super::*;
    use std::path::PathBuf;

    fn shot(name: &str) -> Option<screen::Image> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/alecaExamples/Tools").join(name);
        path.exists().then(|| ocr::decode_file(&path).unwrap())
    }

    fn read(img: &screen::Image, region: Region) -> Option<Card> {
        let game = GameWindow { hwnd: 0, x: 0, y: 0, width: img.width as i32, height: img.height as i32 };
        let (x, y, w, h) = region.on(&game);
        let lines = ocr::read_lines(&screen::upscale2(&screen::crop(img, x, y, w, h))).unwrap();
        parse_card(&lines)
    }

    /// The lilac check spots riven cards and ignores the rest of the screen.
    #[test]
    fn lilac_signature_finds_riven_cards_only() {
        let crop = |img: &screen::Image, r: Region| {
            let game = GameWindow { hwnd: 0, x: 0, y: 0, width: img.width as i32, height: img.height as i32 };
            let (x, y, w, h) = r.on(&game);
            screen::crop(img, x, y, w, h)
        };
        if let Some(img) = shot("inspectRiven.png") {
            let card = screen::card_signature(&crop(&img, INSPECT_CARD)).expect("riven card is lilac");
            assert!(screen::signatures_match(&card, &card));
            let background = Region { x: 0.05, y: 0.465, w: 0.15, h: 0.185 };
            assert!(screen::card_signature(&crop(&img, background)).is_none());
        }
        if let (Some(a), Some(b)) = (shot("reroll (2).png"), shot("reroll (3).png")) {
            // Different cards centred: the signatures differ, so the new one gets read.
            let sa = screen::card_signature(&crop(&a, REROLL_CENTRE)).unwrap();
            let sb = screen::card_signature(&crop(&b, REROLL_CENTRE)).unwrap();
            assert!(!screen::signatures_match(&sa, &sb));
        }
    }

    /// The reward screen (1920x1080): the names band splits into the 4 cards and each snaps to
    /// the right item. Uses the app's cached WFCD data; skipped if it or the screenshot is absent.
    #[test]
    fn reads_the_relic_reward_screenshot() {
        let Some(img) = shot("voidMissionExtractionScreen.jpg") else { return };
        let Some(appdata) = std::env::var_os("APPDATA") else { return };
        let files = crate::translate::wfcd::load_cached(&PathBuf::from(appdata).join("io.lotus.companion/wfcd"));
        if files.is_empty() {
            return;
        }
        let index = ItemIndex::build(&files);
        let game = GameWindow { hwnd: 0, x: 0, y: 0, width: img.width as i32, height: img.height as i32 };
        let (x, y, w, h) = relics::NAMES.on(&game);
        let crop = screen::upscale2(&screen::crop(&img, x, y, w, h));
        let cols = relics::columns(&ocr::read_words(&crop).unwrap(), f64::from(crop.width));
        // A squad of four Meso relics, one holding each card's item (in game the log names them).
        let expected = ["Destreza Prime Handle", "Forma Blueprint", "Wukong Prime Systems Blueprint", "Mirage Prime Chassis Blueprint"];
        let mut squad: Vec<String> = expected
            .iter()
            .filter_map(|item| {
                let mut found: Vec<&str> = index.relics().filter(|(n, r)| n.starts_with("Meso ") && r.iter().any(|x| x.name == *item)).map(|(n, _)| n).collect();
                found.sort();
                found.first().map(|n| n.to_string())
            })
            .collect();
        squad.dedup();
        let cands = relics::candidates(&index, &squad, None);
        let names: Vec<String> = cols.iter().map(|c| relics::snap(&c.text, &cands).map_or(format!("?{}", c.text), |m| m.name.clone())).collect();
        assert_eq!(names, expected, "squad relics {squad:?}");
        // Without the squad's relics, the clean names still match from every Meso relic.
        let wide = relics::candidates(&index, &[], Some("Meso"));
        assert_eq!(relics::snap(&cols[2].text, &wide).unwrap().name, expected[2]);
    }

    /// The crop regions land on the cards in the user's 2560x1440 screenshots, and the whole
    /// read (OCR, parse, rebuild) gives the right rivens. Skipped if the screenshots are absent.
    #[test]
    fn regions_read_the_screenshots() {
        let data = rivens::data();
        let data = &*data;
        let sheet = crate::riven_grades::GradeSheet::default();
        if let Some(img) = shot("inspectRiven.png") {
            let card = read(&img, INSPECT_CARD).expect("inspect card");
            assert_eq!(card.title, "Torid Crita-acriata");
            let r = card::rebuild(data, &sheet, "LotusRifleRandomModRare", &card, Some("/Lotus/Weapons/ClanTech/Bio/BioWeapon"), None).unwrap();
            assert_eq!(r.view.stats[0].text, "+198.6% Critical Chance");
        }
        let prime = Some("/Lotus/Weapons/Tenno/LongGuns/PrimeBurston/PrimeBurston");
        if let Some(img) = shot("reroll (1).png") {
            let card = read(&img, REROLL_CENTRE).expect("reroll centre");
            assert_eq!(card.title, "Burston Crita-lexitis");
            assert_eq!(card.stats.len(), 4);
        }
        if let Some(img) = shot("reroll (2).png") {
            let centre = read(&img, REROLL_CENTRE).expect("new roll centred");
            let left = read(&img, REROLL_LEFT).expect("old roll at the left");
            assert_eq!(centre.title, "Burston Acri-hexasus");
            assert_eq!(left.title, "Burston Crita-lexitis");
            let new = card::rebuild(data, &sheet, "LotusRifleRandomModRare", &centre, prime, Some(8)).unwrap();
            let texts: Vec<&str> = new.view.stats.iter().map(|s| s.text.as_str()).collect();
            assert_eq!(texts, ["+143.2% Slash", "+159.2% Critical Damage", "+117.3% Status Chance", "-132.2% Puncture"]);
        }
        // Wrapped labels: "Magazine / Capacity", "(x2 for Heavy Attacks)", "for / Slide Attack",
        // and a title pushed up by the extra lines.
        if let Some(img) = shot("rivenTool/error1.png") {
            let card = read(&img, INSPECT_CARD).expect("Torid card");
            assert_eq!(card.title, "Torid Acri-caricron");
            let r = card::rebuild(data, &sheet, "LotusRifleRandomModRare", &card, Some("/Lotus/Weapons/ClanTech/Bio/BioWeapon"), None).unwrap();
            assert_eq!(r.view.stats.len(), 4);
            assert!(r.view.stats.iter().any(|s| s.text.contains("Magazine Capacity")), "{:?}", r.view.stats);
        }
        if let Some(img) = shot("rivenTool/error2.png") {
            let card = read(&img, INSPECT_CARD).expect("Magistar card");
            assert_eq!(card.title, "Magistar Acri-paracron");
            assert_eq!((card.stats.len(), card.mastery, card.rerolls), (4, Some(10), Some(113)));
            let magistar = Some("/Lotus/Weapons/Tenno/Melee/Maces/PaladinMace/PaladinMaceWeapon");
            let r = card::rebuild(data, &sheet, "LotusModularMeleeRandomModRare", &card, magistar, None)
                .or_else(|_| card::rebuild(data, &sheet, "PlayerMeleeWeaponRandomModRare", &card, magistar, None))
                .unwrap();
            let texts: Vec<&str> = r.view.stats.iter().map(|s| s.text.as_str()).collect();
            assert!(texts.iter().any(|t| t.contains("Heavy Attacks")), "{texts:?}");
            assert!(texts.iter().any(|t| t.contains("Slide Attack")), "{texts:?}");
        }
        // The "Fits in" box names the weapon the numbers are for.
        let fits_in = |img: &screen::Image, card: &Card| {
            let game = GameWindow { hwnd: 0, x: 0, y: 0, width: img.width as i32, height: img.height as i32 };
            let (x, y, w, h) = FITS_IN.on(&game);
            let lines = ocr::read_lines(&screen::upscale2(&screen::crop(img, x, y, w, h))).unwrap();
            card::fits_in_weapon(data, card, &lines)
        };
        if let Some(img) = shot("inspectRiven.png") {
            let card = read(&img, INSPECT_CARD).unwrap();
            assert_eq!(fits_in(&img, &card).as_deref(), Some("/Lotus/Weapons/ClanTech/Bio/BioWeapon"));
        }
        if let Some(img) = shot("rivenTool/error2.png") {
            let card = read(&img, INSPECT_CARD).unwrap();
            assert_eq!(fits_in(&img, &card).as_deref(), Some("/Lotus/Weapons/Tenno/Melee/Maces/PaladinMace/PaladinMaceWeapon"));
        }
        if let Some(img) = shot("reroll (3).png") {
            let right = read(&img, REROLL_RIGHT).expect("new roll at the right");
            assert_eq!(right.title, "Burston Acri-hexasus");
            assert!(!right.same_as(&read(&img, REROLL_CENTRE).unwrap()));
        }
    }
}
