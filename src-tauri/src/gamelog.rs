//! Live game log: Warframe's lines as it writes them, from its debug output.
//!
//! The game writes EE.log in bursts, often 10–20 s late, but it also sends every line through
//! `OutputDebugString`. Windows hands those to one listener through the `DBWIN_BUFFER` shared
//! memory and the `DBWIN_BUFFER_READY` / `DBWIN_DATA_READY` events (the channel DebugView uses;
//! AlecaFrame reads the game this way too). Listening is read-only: nothing touches the game.
//!
//! Only one listener works at a time. If another one already holds the channel (AlecaFrame,
//! DebugView, a second Lotus), the pipeline relies on the EE.log file. The file is always read
//! too, so a line the channel missed still arrives (late).
//! [`LineMerger`] joins the two sources so that each line is handled once: the file's late copy
//! of a line already seen live is dropped.

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows::Win32::System::Memory::{CreateFileMappingW, MapViewOfFile, UnmapViewOfFile, FILE_MAP_READ, PAGE_READWRITE};
use windows::Win32::System::Threading::{CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject};

use crate::capture::{find_pid, GAME_EXE};

/// The shared buffer: a 4-byte process id, then the text up to a NUL.
const BUFFER_SIZE: usize = 4096;
/// How long one wait for a message lasts (also how often the game's process id is rechecked).
const WAIT_MS: u32 = 1000;
/// How long to wait before trying again when the channel is taken.
const RETRY: Duration = Duration::from_secs(10);
/// Live lines remembered to match the file's late copies (the file lags ~20 s, ~50 lines a second).
const REMEMBER: usize = 20_000;

/// Where the feed stands, for the Tools page and `overlay.log`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedState {
    Starting,
    /// Listening; the game's lines arrive live.
    Listening,
    /// Another program holds the debug channel; EE.log is used instead.
    Busy,
    /// The channel couldn't be opened.
    Failed(String),
}

static STATE: Mutex<FeedState> = Mutex::new(FeedState::Starting);

pub fn state() -> FeedState {
    STATE.lock().unwrap().clone()
}

fn set_state(s: FeedState) {
    let mut cur = STATE.lock().unwrap();
    if *cur != s {
        eprintln!("[gamelog] {s:?}");
        *cur = s;
    }
}

/// Starts the listener thread. Lines from the game arrive on the receiver, one per log line.
pub fn start() -> Receiver<String> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        // One Lotus at a time (say the installed app and a dev build): two listeners would each
        // get about half the lines. Wait for the other one to close.
        let _instance = loop {
            match unsafe { CreateMutexW(None, false, w!("Local\\LotusGameLogFeed")) } {
                Ok(h) if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS => break Owned(h),
                Ok(h) => {
                    drop(Owned(h));
                    set_state(FeedState::Busy);
                }
                Err(e) => set_state(FeedState::Failed(format!("instance lock: {e}"))),
            }
            std::thread::sleep(RETRY);
        };
        loop {
            match listen(&tx) {
                Ok(Stop::ReceiverGone) => return,
                Ok(Stop::Busy) => set_state(FeedState::Busy),
                Err(e) => set_state(FeedState::Failed(e)),
            }
            std::thread::sleep(RETRY);
        }
    });
    rx
}

enum Stop {
    Busy,
    ReceiverGone,
}

/// Closes a handle when dropped.
struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn listen(tx: &Sender<String>) -> Result<Stop, String> {
    unsafe {
        let buffer_ready = Owned(
            CreateEventW(None, false, false, w!("DBWIN_BUFFER_READY")).map_err(|e| format!("buffer event: {e}"))?,
        );
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(Stop::Busy);
        }
        let data_ready = Owned(
            CreateEventW(None, false, false, w!("DBWIN_DATA_READY")).map_err(|e| format!("data event: {e}"))?,
        );
        let mapping = Owned(
            CreateFileMappingW(INVALID_HANDLE_VALUE, None, PAGE_READWRITE, 0, BUFFER_SIZE as u32, w!("DBWIN_BUFFER"))
                .map_err(|e| format!("shared buffer: {e}"))?,
        );
        let view = MapViewOfFile(mapping.0, FILE_MAP_READ, 0, 0, BUFFER_SIZE);
        if view.Value.is_null() {
            return Err("couldn't map the shared buffer".into());
        }
        let buffer = std::slice::from_raw_parts(view.Value as *const u8, BUFFER_SIZE);
        set_state(FeedState::Listening);

        let mut game_pid = find_pid(GAME_EXE);
        let mut pid_checked = Instant::now();
        let stop = loop {
            if pid_checked.elapsed() >= Duration::from_secs(2) {
                game_pid = find_pid(GAME_EXE);
                pid_checked = Instant::now();
            }
            let _ = SetEvent(buffer_ready.0);
            if WaitForSingleObject(data_ready.0, WAIT_MS) != WAIT_OBJECT_0 {
                continue;
            }
            let Some((pid, text)) = read_message(buffer) else { continue };
            if Some(pid) != game_pid {
                continue;
            }
            if split_lines(&text).any(|line| tx.send(line.to_string()).is_err()) {
                break Stop::ReceiverGone;
            }
        };
        let _ = UnmapViewOfFile(view);
        Ok(stop)
    }
}

/// The sender's process id and its text (up to the first NUL).
fn read_message(buffer: &[u8]) -> Option<(u32, String)> {
    let pid = u32::from_le_bytes(buffer.get(..4)?.try_into().ok()?);
    let body = &buffer[4..];
    let end = body.iter().position(|&b| b == 0).unwrap_or(body.len());
    Some((pid, String::from_utf8_lossy(&body[..end]).into_owned()))
}

/// One debug message can hold several lines; empty ones are dropped.
fn split_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n').map(|l| l.trim_end_matches('\r')).filter(|l| !l.trim().is_empty())
}

/// The form two copies of a line are compared in: the file marks some lines with a leading `!`.
fn key(line: &str) -> &str {
    line.trim().trim_start_matches('!')
}

/// Joins the live feed and the EE.log file so each line is handled once, as early as possible.
///
/// Live lines go straight through and are remembered. A file line that was already seen live is
/// dropped; any other file line (the feed was busy or quiet, or missed it) goes through late
/// rather than not at all.
#[derive(Default)]
pub struct LineMerger {
    seen: HashMap<String, u32>,
    order: VecDeque<String>,
}

impl LineMerger {
    /// A line from the live feed. Always handled.
    pub fn live(&mut self, line: &str) {
        let k = key(line).to_string();
        *self.seen.entry(k.clone()).or_default() += 1;
        self.order.push_back(k);
        if self.order.len() > REMEMBER {
            if let Some(old) = self.order.pop_front() {
                self.forget(&old);
            }
        }
    }

    /// A line from the file: true if it should be handled (it wasn't seen live).
    pub fn file(&mut self, line: &str) -> bool {
        let k = key(line);
        if self.seen.contains_key(k) {
            let k = k.to_string();
            self.forget(&k);
            return false;
        }
        true
    }

    fn forget(&mut self, k: &str) {
        if let Some(n) = self.seen.get_mut(k) {
            *n -= 1;
            if *n == 0 {
                self.seen.remove(k);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_message() {
        let mut buf = vec![0u8; BUFFER_SIZE];
        buf[..4].copy_from_slice(&37700u32.to_le_bytes());
        let text = b"6003.390 Game [Warning]: one\r\n6003.395 Script [Info]: two\n";
        buf[4..4 + text.len()].copy_from_slice(text);
        let (pid, body) = read_message(&buf).unwrap();
        assert_eq!(pid, 37700);
        let lines: Vec<&str> = split_lines(&body).collect();
        assert_eq!(lines, ["6003.390 Game [Warning]: one", "6003.395 Script [Info]: two"]);
    }

    #[test]
    fn a_full_buffer_without_nul_is_read_whole() {
        let mut buf = vec![b'x'; BUFFER_SIZE];
        buf[..4].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(read_message(&buf).unwrap().1.len(), BUFFER_SIZE - 4);
    }

    #[test]
    fn file_copies_of_live_lines_are_dropped_once() {
        let mut m = LineMerger::default();
        m.live("5211.907 Script [Info]: ProjectionRewardChoice.lua: Got rewards");
        m.live("5211.908 Script [Info]: ProjectionRewardChoice.lua: Missing icon data!");
        m.live("5211.908 Script [Info]: ProjectionRewardChoice.lua: Missing icon data!");
        assert!(!m.file("5211.907 Script [Info]: ProjectionRewardChoice.lua: Got rewards"));
        // Two identical live lines match two file lines, not more.
        assert!(!m.file("5211.908 Script [Info]: ProjectionRewardChoice.lua: Missing icon data!"));
        assert!(!m.file("5211.908 Script [Info]: ProjectionRewardChoice.lua: Missing icon data!"));
        assert!(m.file("5211.908 Script [Info]: ProjectionRewardChoice.lua: Missing icon data!"));
        // A line the feed never had still goes through.
        assert!(m.file("5226.904 Script [Info]: ProjectionRewardChoice.lua: Relic reward screen shut down"));
    }

    #[test]
    fn the_files_bang_mark_is_ignored() {
        let mut m = LineMerger::default();
        m.live("5206.104 Sys [Info]: VoidProjections: OpenVoidProjectionRewardScreen - PostMigration: 0");
        assert!(!m.file("!5206.104 Sys [Info]: VoidProjections: OpenVoidProjectionRewardScreen - PostMigration: 0"));
    }

    /// With Warframe running: lines arrive from the debug channel.
    /// `cargo test gamelog::tests::live_feed -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_feed() {
        let rx = start();
        let until = Instant::now() + Duration::from_secs(75);
        let mut lines = Vec::new();
        while let Some(left) = until.checked_duration_since(Instant::now()) {
            if let Ok(l) = rx.recv_timeout(left) {
                lines.push(l);
            }
        }
        println!("state {:?}, {} lines; first: {:?}", state(), lines.len(), lines.first());
        assert_eq!(state(), FeedState::Listening);
        assert!(!lines.is_empty(), "no lines in 75 s (is the game running?)");
    }

    #[test]
    fn memory_is_bounded() {
        let mut m = LineMerger::default();
        for i in 0..REMEMBER + 10 {
            m.live(&format!("{i} line"));
        }
        assert_eq!(m.order.len(), REMEMBER);
        assert!(m.file("0 line"));
        assert!(!m.file(&format!("{} line", REMEMBER + 9)));
    }
}
