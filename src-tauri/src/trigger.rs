//! Trigger: tail EE.log and turn the lines we care about into events.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Something in EE.log that the pipeline reacts to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogEvent {
    /// `Sys [Info]: Logged in <name>`: the in-game name (the inventory doesn't contain it).
    LoggedIn { name: String },
    /// `OnInventoryResults, body size=<bytes>`: fresh inventory JSON just arrived. Capture now.
    InventoryArrived { bytes: u64 },
    /// `OnInventoryResults completed in <ms>ms`: the game finished processing it.
    InventoryProcessed { ms: u64 },
    /// `CommitInventoryChangesCallback - Success!`: mission results were saved at mission end.
    /// After a mission the game reuses this reply ("Using cached inventory data") about 20 s
    /// later, so the inventory text is in memory during the end-of-mission screen. Capture now.
    MissionCommitted,
}

/// Matches one EE.log line against the known rules (confirmed in Phase 1).
pub fn parse_line(line: &str) -> Option<LogEvent> {
    if let Some(rest) = after(line, "OnInventoryResults, body size=") {
        return leading_number(rest).map(|bytes| LogEvent::InventoryArrived { bytes });
    }
    if let Some(rest) = after(line, "OnInventoryResults completed in ") {
        return leading_number(rest).map(|ms| LogEvent::InventoryProcessed { ms });
    }
    if line.contains("CommitInventoryChangesCallback - Success") {
        return Some(LogEvent::MissionCommitted);
    }
    if let Some(rest) = after(line, "Sys [Info]: Logged in ") {
        let name = clean_name(rest);
        return (!name.is_empty()).then_some(LogEvent::LoggedIn { name });
    }
    None
}

/// In-game names can end with a platform icon, written as a private-use Unicode character
/// (U+E000–U+F8FF); keep only the visible name.
fn clean_name(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control() && !('\u{E000}'..='\u{F8FF}').contains(c))
        .collect::<String>()
        .trim()
        .to_string()
}

/// The name from the last `Logged in` line in a log file, if any. Read once at startup, so the
/// name is known without waiting for the next login.
pub fn last_login_name(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    String::from_utf8_lossy(&bytes).lines().rev().find_map(|line| match parse_line(line) {
        Some(LogEvent::LoggedIn { name }) => Some(name),
        _ => None,
    })
}

fn after<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    line.find(marker).map(|i| &line[i + marker.len()..])
}

fn leading_number(s: &str) -> Option<u64> {
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The default EE.log location: `%LOCALAPPDATA%\Warframe\EE.log`.
pub fn default_log_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Warframe").join("EE.log"))
}

/// Follows a log file that another process keeps appending to, like `tail -f`.
///
/// Handles the game recreating EE.log on restart (the file shrinks or its creation time changes).
pub struct LogTail {
    path: PathBuf,
    offset: u64,
    created: Option<SystemTime>,
    partial: Vec<u8>,
}

impl LogTail {
    /// Starts at the current end of the file, so old lines don't fire events again.
    pub fn from_end(path: &Path) -> Self {
        let meta = std::fs::metadata(path).ok();
        Self {
            path: path.to_path_buf(),
            offset: meta.as_ref().map_or(0, |m| m.len()),
            created: meta.and_then(|m| m.created().ok()),
            partial: Vec::new(),
        }
    }

    /// Returns the complete lines written since the last poll. A missing file yields nothing.
    pub fn poll(&mut self) -> Vec<String> {
        let Ok(meta) = std::fs::metadata(&self.path) else {
            return Vec::new();
        };
        let created = meta.created().ok();
        if meta.len() < self.offset || (created.is_some() && created != self.created) {
            // New log file: the game restarted.
            self.offset = 0;
            self.partial.clear();
            self.created = created;
        }
        if meta.len() == self.offset {
            return Vec::new();
        }
        // std opens files with shared read/write access, so the game can keep writing.
        let Ok(mut file) = File::open(&self.path) else {
            return Vec::new();
        };
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut new = Vec::new();
        let Ok(n) = file.read_to_end(&mut new) else {
            return Vec::new();
        };
        self.offset += n as u64;
        self.partial.extend_from_slice(&new);

        let mut lines = Vec::new();
        while let Some(i) = self.partial.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=i).collect();
            let text = String::from_utf8_lossy(&line);
            lines.push(text.trim_end_matches(['\r', '\n']).to_string());
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_confirmed_lines() {
        assert_eq!(
            parse_line("13.641 Sys [Info]: OnInventoryResults, body size=933122"),
            Some(LogEvent::InventoryArrived { bytes: 933122 })
        );
        assert_eq!(
            parse_line("13.973 Sys [Info]: OnInventoryResults completed in 332ms"),
            Some(LogEvent::InventoryProcessed { ms: 332 })
        );
        assert_eq!(
            parse_line("13.237 Sys [Info]: Logged in SomeTenno"),
            Some(LogEvent::LoggedIn { name: "SomeTenno".into() })
        );
        assert_eq!(
            parse_line("13.237 Sys [Info]: Logged in SomeTenno\u{E000}\r"),
            Some(LogEvent::LoggedIn { name: "SomeTenno".into() }),
            "platform icon and line ending removed"
        );
        assert_eq!(parse_line("13.237 Sys [Info]: Logged in "), None);
        assert_eq!(
            parse_line("177.371 Sys [Info]: CommitInventoryChangesCallback - Success!(Attempt #0)"),
            Some(LogEvent::MissionCommitted)
        );
        assert_eq!(parse_line("177.156 Game [Info]: CommitInventoryChangesToDB"), None);
        assert_eq!(parse_line("13.246 Sys [Info]: SyncInventoryFromDB"), None);
        assert_eq!(parse_line("1.0 Sys [Info]: Clearing *all* inventory checkpoint info"), None);
    }

    #[test]
    fn finds_the_last_login_in_a_file() {
        let path = std::env::temp_dir().join(format!("wf-login-test-{}.log", std::process::id()));
        std::fs::write(&path, "1.0 Sys [Info]: Logged in FirstName\n2.0 Sys [Info]: SyncInventoryFromDB\n3.0 Sys [Info]: Logged in SecondName\n4.0 other\n").unwrap();
        assert_eq!(last_login_name(&path).as_deref(), Some("SecondName"));
        std::fs::write(&path, "no logins here\n").unwrap();
        assert_eq!(last_login_name(&path), None);
        let _ = std::fs::remove_file(&path);
        assert_eq!(last_login_name(&path), None, "missing file");
    }

    #[test]
    fn tails_appends_and_restarts() {
        let path = std::env::temp_dir().join(format!("wf-tail-test-{}.log", std::process::id()));
        std::fs::write(&path, "old line\n").unwrap();
        let mut tail = LogTail::from_end(&path);
        assert!(tail.poll().is_empty(), "old lines are skipped");

        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(f, "first\r\nsecond half").unwrap();
        f.flush().unwrap();
        assert_eq!(tail.poll(), vec!["first"]);
        write!(f, "-done\n").unwrap();
        f.flush().unwrap();
        assert_eq!(tail.poll(), vec!["second half-done"]);
        drop(f);

        // Game restart: the log is replaced by a shorter file.
        std::fs::write(&path, "new\n").unwrap();
        assert_eq!(tail.poll(), vec!["new"]);
        let _ = std::fs::remove_file(&path);
    }
}
