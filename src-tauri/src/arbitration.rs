//! Arbitration schedule and tiers from browse.wf (credited on the Home page).
//!
//! - Schedule: `https://browse.wf/arbys.txt`, one line per hour, `<unix seconds>,<node tag>`
//!   (e.g. `1727884800,SettlementNode11`), computed years ahead.
//! - Tiers: `https://browse.wf/supplemental-data/arbyTiers.js`, the Arbitration Goons' rating per
//!   node (`SolNode450: "S",`). Nodes not listed are F tier, as on browse.wf.
//!
//! Both are cached and refreshed every 2 weeks; node names, planets, mission types and factions
//! come from the app's node data.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::Serialize;
use tauri::State;

use crate::pipeline::Shared;
use crate::translate::nodes::NodeData;

const SCHEDULE_URL: &str = "https://browse.wf/arbys.txt";
const TIERS_URL: &str = "https://browse.wf/supplemental-data/arbyTiers.js";
/// Re-download after this long; the schedule covers years, so this only picks up corrections.
pub const MAX_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);
/// Each arbitration lasts one hour.
pub const LENGTH_SECS: i64 = 3600;
/// Tier for nodes the tier list doesn't mention (browse.wf does the same).
pub const UNLISTED_TIER: &str = "F";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arbitration {
    /// Start time, unix seconds.
    pub start: i64,
    /// Node tag, e.g. `SolNode216`.
    pub tag: String,
}

/// Everything loaded from browse.wf.
#[derive(Debug, Default)]
pub struct ArbData {
    pub schedule: Vec<Arbitration>,
    /// Node tag → tier letter.
    pub tiers: HashMap<String, String>,
}

/// Parses `arbys.txt`; malformed lines are skipped. Sorted by start time.
pub fn parse(text: &str) -> Vec<Arbitration> {
    let mut out: Vec<Arbitration> = text
        .lines()
        .filter_map(|line| {
            let (ts, tag) = line.trim().split_once(',')?;
            Some(Arbitration { start: ts.trim().parse().ok()?, tag: tag.trim().to_string() })
        })
        .filter(|a| !a.tag.is_empty())
        .collect();
    out.sort_by_key(|a| a.start);
    out
}

/// Parses `arbyTiers.js`: lines like `SolNode450: "S",` inside `window.arbyTiers = { … }`.
/// Comments and anything else are skipped.
pub fn parse_tiers(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.trim().split_once(':')?;
            let key = key.trim().trim_matches('"');
            let tier = value.trim().trim_end_matches(',').trim().trim_matches('"');
            let valid_key = !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            let valid_tier = !tier.is_empty() && tier.len() <= 2 && tier.chars().all(|c| c.is_ascii_uppercase());
            (valid_key && valid_tier).then(|| (key.to_string(), tier.to_string()))
        })
        .collect()
}

/// Loads both files from the cache in `dir`, downloading any that are missing or older than
/// [`MAX_AGE`]. A failed download keeps using the cached copy.
pub fn load_or_refresh(dir: &Path) -> ArbData {
    let _ = std::fs::create_dir_all(dir);
    let schedule = cached_text(dir, "arbys.txt", SCHEDULE_URL, |t| parse(t).len() > 100);
    let tiers = cached_text(dir, "arbyTiers.js", TIERS_URL, |t| parse_tiers(t).len() > 5);
    ArbData {
        schedule: schedule.as_deref().map(parse).unwrap_or_default(),
        tiers: tiers.as_deref().map(parse_tiers).unwrap_or_default(),
    }
}

/// Returns a cached file's text, refreshing it first if it's old. `valid` rejects a download that
/// doesn't look like the expected data (an error page, an empty file).
pub(crate) fn cached_text(dir: &Path, file: &str, url: &str, valid: impl Fn(&str) -> bool) -> Option<String> {
    let path = dir.join(file);
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < MAX_AGE);
    if !fresh {
        match download(url) {
            Ok(text) if valid(&text) => {
                let tmp = dir.join(format!("{file}.tmp"));
                if std::fs::write(&tmp, &text).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
            }
            Ok(_) => eprintln!("[cache] {file}: download didn't look right; keeping the cached copy"),
            Err(e) => eprintln!("[cache] {file}: download failed: {e}"),
        }
    }
    std::fs::read_to_string(&path).ok()
}

fn download(url: &str) -> Result<String, String> {
    let mut response = crate::net::agent().get(url).call().map_err(|e| e.to_string())?;
    response.body_mut().with_config().limit(16 * 1024 * 1024).read_to_string().map_err(|e| e.to_string())
}

/// One arbitration as the page shows it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArbView {
    pub start_ms: i64,
    pub end_ms: i64,
    pub tag: String,
    /// S, A, B, C, D or F (Arbitration Goons via browse.wf).
    pub tier: String,
    pub node: String,
    pub planet: String,
    pub mission_type: String,
    pub enemy: String,
}

/// Arbitrations that overlap `[from_ms, to_ms)`.
pub fn between(data: &ArbData, nodes: &NodeData, from_ms: i64, to_ms: i64) -> Vec<ArbView> {
    let (from, to) = (from_ms / 1000, to_ms / 1000);
    // Binary search to the first one that could overlap, then walk forward.
    let start = data.schedule.partition_point(|a| a.start + LENGTH_SECS <= from);
    data.schedule[start..]
        .iter()
        .take_while(|a| a.start < to)
        .map(|a| {
            let node = nodes.get(&a.tag);
            ArbView {
                start_ms: a.start * 1000,
                end_ms: (a.start + LENGTH_SECS) * 1000,
                tag: a.tag.clone(),
                tier: data.tiers.get(&a.tag).cloned().unwrap_or_else(|| UNLISTED_TIER.to_string()),
                node: node.map_or_else(|| a.tag.clone(), |n| n.name.clone()),
                planet: node.map(|n| n.planet.clone()).unwrap_or_default(),
                mission_type: node.map(|n| n.mission_type.clone()).unwrap_or_default(),
                enemy: node.map(|n| n.enemy.clone()).unwrap_or_default(),
            }
        })
        .collect()
}

/// The page asks for a window (start of today to 3 days later, in its local time).
#[tauri::command]
pub fn get_arbitrations(from_ms: i64, to_ms: i64, shared: State<'_, Arc<Shared>>) -> Vec<ArbView> {
    let data = shared.arbitrations.read().unwrap().clone();
    let nodes = shared.nodes.read().unwrap().clone();
    between(&data, &nodes, from_ms, to_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lines_and_skips_junk() {
        let s = parse("1727888400,ClanNode10\n\nnot a line\n1727884800,SettlementNode11\r\n1727892000,\n");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0], Arbitration { start: 1727884800, tag: "SettlementNode11".into() }, "sorted by time");
    }

    #[test]
    fn parses_tier_file() {
        // Same shape as browse.wf's arbyTiers.js.
        let js = "// Credit to the Arbitration Goons for this data.\n\nwindow.arbyTiers = {\n\tSolNode450: \"S\",\n\tClanNode24: \"B\",\n\tSolNode46: \"C\"\n};\n";
        let t = parse_tiers(js);
        assert_eq!(t.len(), 3);
        assert_eq!(t["SolNode450"], "S");
        assert_eq!(t["SolNode46"], "C", "last entry without a trailing comma");
        assert!(!t.contains_key("window.arbyTiers = {"));
    }

    #[test]
    fn window_includes_the_one_in_progress() {
        let data = ArbData {
            schedule: parse("1000,SolNode1\n4600,SolNode2\n8200,SolNode3\n11800,SolNode4\n"),
            tiers: parse_tiers("SolNode2: \"S\",\n"),
        };
        let nodes = NodeData::from_lua(
            r#"{ Name = "Valefor", Planet = "Europa", Type = "Excavation", Enemy = "Corpus", InternalName = "SolNode2" },"#,
        );
        // Window starts at 5000 s: SolNode2 (4600-8200) is in progress, SolNode4 starts at the end.
        let v = between(&data, &nodes, 5_000_000, 11_800_000);
        let tags: Vec<&str> = v.iter().map(|a| a.tag.as_str()).collect();
        assert_eq!(tags, ["SolNode2", "SolNode3"]);
        assert_eq!((v[0].node.as_str(), v[0].planet.as_str(), v[0].mission_type.as_str()), ("Valefor", "Europa", "Excavation"));
        assert_eq!((v[0].start_ms, v[0].end_ms), (4_600_000, 8_200_000));
        assert_eq!(v[0].tier, "S");
        assert_eq!(v[1].tier, "F", "unlisted nodes are F tier");
        assert_eq!(v[1].node, "SolNode3", "unknown nodes fall back to the tag");
    }
}
