//! Star chart nodes: which ones count toward mastery and how much XP each gives.
//!
//! Neither WFCD nor DE's Public Export has per-node mastery XP. The Warframe wiki does, in its
//! mission data module (`Module:Missions/data`, CC BY-SA 3.0, credited in the app). A snapshot
//! (`data/nodes.json`, made by `scripts/gen_nodes.py`) ships with the app; a fresh copy of the
//! module is downloaded every 2 weeks and parsed the same way.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

use serde::Deserialize;

const WIKI_URL: &str = "https://wiki.warframe.com/w/Module:Missions/data?action=raw";
/// Re-download the wiki module after this long.
pub const MAX_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);
const BUNDLED: &str = include_str!("../../data/nodes.json");

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Node {
    /// Internal tag, as in the inventory's `Missions` (e.g. `SolNode94`).
    pub tag: String,
    pub name: String,
    pub planet: String,
    /// Mission type, e.g. "Survival", "Defense".
    #[serde(default, rename = "type")]
    pub mission_type: String,
    /// Enemy faction, e.g. "Grineer" or "Infested / Grineer".
    #[serde(default)]
    pub enemy: String,
    /// Mastery XP for the first completion (and again on The Steel Path).
    pub xp: u32,
    /// Whether the node counts toward star chart progress.
    pub tracked: bool,
}

#[derive(Debug, Default)]
pub struct NodeData {
    nodes: HashMap<String, Node>,
}

#[derive(Deserialize)]
struct Bundled {
    nodes: Vec<Node>,
}

impl NodeData {
    /// The snapshot that ships with the app.
    pub fn bundled() -> Self {
        let parsed: Bundled = serde_json::from_str(BUNDLED).expect("bundled nodes.json is valid");
        Self::from_nodes(parsed.nodes)
    }

    fn from_nodes(list: Vec<Node>) -> Self {
        let mut nodes = HashMap::new();
        for n in list {
            nodes.entry(n.tag.clone()).or_insert(n);
        }
        Self { nodes }
    }

    /// Parses the wiki module: one node per line, `{ Name = "...", ..., MasteryExp = 3,
    /// InternalName = "SolNode223", ..., IsTracked = true }`.
    pub fn from_lua(lua: &str) -> Self {
        let list = lua
            .lines()
            .filter_map(|line| {
                let tag = string_field(line, "InternalName")?;
                Some(Node {
                    tag: tag.to_string(),
                    name: string_field(line, "Name").unwrap_or(tag).to_string(),
                    planet: string_field(line, "Planet").unwrap_or_default().to_string(),
                    mission_type: string_field(line, "Type").unwrap_or_default().to_string(),
                    enemy: enemy_field(line),
                    xp: bare_field(line, "MasteryExp").and_then(|v| v.parse().ok()).unwrap_or(0),
                    tracked: bare_field(line, "IsTracked") == Some("true"),
                })
            })
            .collect();
        Self::from_nodes(list)
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn get(&self, tag: &str) -> Option<&Node> {
        self.nodes.get(tag)
    }

    /// Nodes that count toward mastery: tracked and worth some XP.
    pub fn counted(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values().filter(|n| n.tracked && n.xp > 0)
    }
}

/// Finds `key = <value>` where `key` is a whole word (so `Name` doesn't match `InternalName`).
fn raw_field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(i) = line[from..].find(key) {
        let start = from + i;
        let before_ok = start == 0 || !line[..start].ends_with(|c: char| c.is_alphanumeric() || c == '_');
        let rest = line[start + key.len()..].trim_start();
        if before_ok {
            if let Some(value) = rest.strip_prefix('=') {
                return Some(value.trim_start());
            }
        }
        from = start + key.len();
    }
    None
}

fn string_field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let value = raw_field(line, key)?.strip_prefix('"')?;
    value.find('"').map(|end| &value[..end])
}

/// `Enemy` is either `"Grineer"` or a table like `{"Infested", "Grineer"}`.
fn enemy_field(line: &str) -> String {
    if let Some(single) = string_field(line, "Enemy") {
        return single.to_string();
    }
    let Some(table) = raw_field(line, "Enemy").and_then(|v| v.strip_prefix('{')) else {
        return String::new();
    };
    let body = &table[..table.find('}').unwrap_or(table.len())];
    body.split('"').skip(1).step_by(2).collect::<Vec<_>>().join(" / ")
}

fn bare_field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let value = raw_field(line, key)?;
    let end = value.find([',', '}', ' ']).unwrap_or(value.len());
    Some(&value[..end])
}

/// Loads node data: a cached copy of the wiki module if there is one (downloading a new one if
/// it's older than [`MAX_AGE`]), otherwise the bundled snapshot.
pub fn load_or_refresh(dir: &Path) -> NodeData {
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join("missions_data.lua");
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < MAX_AGE);
    if !fresh {
        match download() {
            Ok(text) => {
                // Only keep it if it parses into a plausible node list.
                if NodeData::from_lua(&text).counted().count() > 100 {
                    let tmp = path.with_extension("lua.tmp");
                    if std::fs::write(&tmp, &text).is_ok() {
                        let _ = std::fs::rename(&tmp, &path);
                    }
                }
            }
            Err(e) => eprintln!("[nodes] wiki download failed: {e}"),
        }
    }
    match std::fs::read_to_string(&path).map(|t| NodeData::from_lua(&t)) {
        Ok(data) if data.counted().count() > 100 => data,
        _ => NodeData::bundled(),
    }
}

fn download() -> Result<String, String> {
    let mut response = crate::net::agent().get(WIKI_URL).call().map_err(|e| e.to_string())?;
    response.body_mut().with_config().limit(8 * 1024 * 1024).read_to_string().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wiki_lines() {
        let lua = r#"
		{ Name = "Boethius", Link = "Boethius", Planet = "Mercury", Type = "Mobile Defense", MasteryExp = 3, InternalName = "SolNode223", NextNodes = { "Apollodorus" }, PreviousNodes = { "M Prime" }, IsTracked = true },
		{ Name = "Larunda Relay", Link = "Relay", Planet = "Mercury", MasteryExp = 0, InternalName = "MercuryHUB", CreditReward = 0 },
		{ Name = "Tolstoj", Planet = "Mercury", MasteryExp = 25, InternalName = "SolNode108", IsTracked = true },
        "#;
        let data = NodeData::from_lua(lua);
        assert_eq!(data.len(), 3);
        let b = data.get("SolNode223").unwrap();
        assert_eq!((b.name.as_str(), b.planet.as_str(), b.xp, b.tracked), ("Boethius", "Mercury", 3, true));
        assert!(!data.get("MercuryHUB").unwrap().tracked);
        let mut counted: Vec<&str> = data.counted().map(|n| n.tag.as_str()).collect();
        counted.sort();
        assert_eq!(counted, ["SolNode108", "SolNode223"]);
    }

    #[test]
    fn reads_mission_type_and_enemy() {
        let lua = r#"{ Name = "M Prime", Planet = "Mercury", Type = "Exterminate", Enemy = {"Infested", "Grineer"}, MasteryExp = 3, InternalName = "SolNode103", IsTracked = true },
                     { Name = "Zeugma", Planet = "Phobos", Type = "Survival", Enemy = "Infested", MasteryExp = 0, InternalName = "ClanNode11" },"#;
        let data = NodeData::from_lua(lua);
        let m = data.get("SolNode103").unwrap();
        assert_eq!((m.mission_type.as_str(), m.enemy.as_str()), ("Exterminate", "Infested / Grineer"));
        assert_eq!(data.get("ClanNode11").unwrap().enemy, "Infested");
        let bundled = NodeData::bundled();
        assert_eq!(bundled.get("ClanNode11").map(|n| n.mission_type.as_str()), Some("Survival"));
    }

    #[test]
    fn name_is_a_whole_word() {
        assert_eq!(string_field(r#"{ InternalName = "SolNode1", Name = "Galatea" }"#, "Name"), Some("Galatea"));
    }

    #[test]
    fn bundled_snapshot_loads() {
        let data = NodeData::bundled();
        assert!(data.counted().count() > 150);
        assert_eq!(data.get("SolNode108").map(|n| n.name.as_str()), Some("Tolstoj"));
    }
}
