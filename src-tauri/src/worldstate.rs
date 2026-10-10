//! Live world state from Digital Extremes' public endpoint (`api.warframe.com/cdn/worldState.php`).
//!
//! Only Void Fissures are read for now: `ActiveMissions`, each with a node, a relic tier
//! (`Modifier`, `VoidT1`–`VoidT6`), activation/expiry times and `Hard: true` for Steel Path.
//! The endpoint is CDN-cached for ~10 s; the app fetches at most once a minute, and only while a
//! page asks (Home polls while it's open).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::pipeline::Shared;
use crate::translate::nodes::NodeData;

const URL: &str = "https://api.warframe.com/cdn/worldState.php";
/// Reuse the last download for this long.
const MIN_INTERVAL: Duration = Duration::from_secs(60);

/// A fissure as listed in the world state.
#[derive(Debug, Clone, PartialEq)]
pub struct Fissure {
    pub node: String,
    pub mission_type: String,
    pub modifier: String,
    pub activation_ms: i64,
    pub expiry_ms: i64,
    pub steel_path: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawWorldState {
    #[serde(default)]
    active_missions: Vec<RawMission>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawMission {
    #[serde(default)]
    node: String,
    #[serde(default)]
    mission_type: String,
    #[serde(default)]
    modifier: String,
    activation: RawDate,
    expiry: RawDate,
    #[serde(default)]
    hard: bool,
}

/// `{"$date": {"$numberLong": "1791398882441"}}`
#[derive(Deserialize)]
struct RawDate {
    #[serde(rename = "$date")]
    date: RawLong,
}

#[derive(Deserialize)]
struct RawLong {
    #[serde(rename = "$numberLong")]
    number_long: String,
}

impl RawDate {
    fn ms(&self) -> i64 {
        self.date.number_long.parse().unwrap_or(0)
    }
}

/// Parses the fissures out of a world state document.
pub fn parse_fissures(json: &str) -> Result<Vec<Fissure>, String> {
    let ws: RawWorldState = serde_json::from_str(json).map_err(|e| e.to_string())?;
    Ok(ws
        .active_missions
        .into_iter()
        .filter(|m| m.modifier.starts_with("VoidT"))
        .map(|m| Fissure {
            activation_ms: m.activation.ms(),
            expiry_ms: m.expiry.ms(),
            node: m.node,
            mission_type: m.mission_type,
            modifier: m.modifier,
            steel_path: m.hard,
        })
        .collect())
}

/// Relic era for a fissure modifier, and its sort order.
pub fn era(modifier: &str) -> (&'static str, u8) {
    match modifier {
        "VoidT1" => ("Lith", 1),
        "VoidT2" => ("Meso", 2),
        "VoidT3" => ("Neo", 3),
        "VoidT4" => ("Axi", 4),
        "VoidT5" => ("Requiem", 5),
        "VoidT6" => ("Omnia", 6),
        _ => ("Unknown", 9),
    }
}

/// Readable name for a world-state mission type, used when the node data doesn't know the node.
fn mission_name(mt: &str) -> String {
    let known = match mt {
        "MT_EXTERMINATION" => "Exterminate",
        "MT_TERRITORY" => "Interception",
        "MT_ARTIFACT" => "Disruption",
        "MT_INTEL" => "Spy",
        "MT_MOBILE_DEFENSE" => "Mobile Defense",
        "MT_CORRUPTION" => "Void Flood",
        "MT_VOID_CASCADE" => "Void Cascade",
        "MT_ARMAGEDDON" => "Void Armageddon",
        "MT_EVACUATION" => "Defection",
        "MT_RETRIEVAL" => "Hijack",
        "MT_LANDSCAPE" => "Free Roam",
        "MT_ALCHEMY" => "Alchemy",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_string();
    }
    // MT_MOBILE_DEFENSE → Mobile Defense
    mt.trim_start_matches("MT_")
        .split('_')
        .map(|w| {
            let w = w.to_lowercase();
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// One fissure as the page shows it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FissureView {
    pub tag: String,
    pub node: String,
    pub planet: String,
    pub mission_type: String,
    pub enemy: String,
    /// Lith, Meso, Neo, Axi, Requiem or Omnia.
    pub era: String,
    pub era_order: u8,
    pub activation_ms: i64,
    pub expiry_ms: i64,
    pub steel_path: bool,
}

pub fn views(fissures: &[Fissure], nodes: &NodeData) -> Vec<FissureView> {
    let mut out: Vec<FissureView> = fissures
        .iter()
        .map(|f| {
            let node = nodes.get(&f.node);
            let (era, era_order) = era(&f.modifier);
            FissureView {
                tag: f.node.clone(),
                node: node.map_or_else(|| f.node.clone(), |n| n.name.clone()),
                planet: node.map(|n| n.planet.clone()).unwrap_or_default(),
                // Fissures use the mission type the world state gives (it can differ from the node's).
                mission_type: mission_name(&f.mission_type),
                enemy: node.map(|n| n.enemy.clone()).unwrap_or_default(),
                era: era.to_string(),
                era_order,
                activation_ms: f.activation_ms,
                expiry_ms: f.expiry_ms,
                steel_path: f.steel_path,
            }
        })
        .collect();
    out.sort_by(|a, b| a.era_order.cmp(&b.era_order).then(a.expiry_ms.cmp(&b.expiry_ms)));
    out
}

/// Last download, so repeated requests within [`MIN_INTERVAL`] don't hit the server.
static CACHE: Mutex<Option<(Instant, Arc<Vec<Fissure>>)>> = Mutex::new(None);

fn fissures() -> Result<Arc<Vec<Fissure>>, String> {
    if let Some((at, list)) = CACHE.lock().unwrap().as_ref() {
        if at.elapsed() < MIN_INTERVAL {
            return Ok(list.clone());
        }
    }
    let downloaded = download().and_then(|text| parse_fissures(&text));
    let mut cache = CACHE.lock().unwrap();
    match downloaded {
        Ok(list) => {
            let list = Arc::new(list);
            *cache = Some((Instant::now(), list.clone()));
            Ok(list)
        }
        // Offline: keep showing the last list (the page drops expired ones).
        Err(e) => cache.as_ref().map(|(_, l)| l.clone()).ok_or(e),
    }
}

fn download() -> Result<String, String> {
    let mut response = crate::net::agent().get(URL).call().map_err(|e| e.to_string())?;
    response.body_mut().with_config().limit(16 * 1024 * 1024).read_to_string().map_err(|e| e.to_string())
}

/// Current Void Fissures, normal and Steel Path, sorted by relic era then time left.
#[tauri::command]
pub async fn get_fissures(shared: State<'_, Arc<Shared>>) -> Result<Vec<FissureView>, String> {
    let nodes = shared.nodes.read().unwrap().clone();
    let list = tauri::async_runtime::spawn_blocking(fissures).await.map_err(|e| e.to_string())??;
    Ok(views(&list, &nodes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"Time": 1, "ActiveMissions": [
        {"_id": {"$oid": "a"}, "Region": 12, "Seed": 1,
         "Activation": {"$date": {"$numberLong": "1000"}}, "Expiry": {"$date": {"$numberLong": "5000"}},
         "Node": "SolNode189", "MissionType": "MT_RESCUE", "Modifier": "VoidT4"},
        {"_id": {"$oid": "b"}, "Region": 10, "Seed": 2,
         "Activation": {"$date": {"$numberLong": "2000"}}, "Expiry": {"$date": {"$numberLong": "4000"}},
         "Node": "SolNode147", "MissionType": "MT_TERRITORY", "Modifier": "VoidT1", "Hard": true}
    ], "VoidStorms": []}"#;

    #[test]
    fn parses_fissures() {
        let f = parse_fissures(SAMPLE).unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!((f[0].activation_ms, f[0].expiry_ms, f[0].steel_path), (1000, 5000, false));
        assert!(f[1].steel_path);
    }

    #[test]
    fn views_are_named_and_sorted_by_era() {
        let nodes = NodeData::from_lua(
            r#"{ Name = "Naga", Planet = "Sedna", Type = "Rescue", Enemy = "Grineer", InternalName = "SolNode189" },"#,
        );
        let v = views(&parse_fissures(SAMPLE).unwrap(), &nodes);
        assert_eq!(v[0].era, "Lith", "Lith before Axi");
        assert_eq!((v[0].node.as_str(), v[0].mission_type.as_str()), ("SolNode147", "Interception"));
        assert_eq!((v[1].node.as_str(), v[1].planet.as_str(), v[1].era.as_str()), ("Naga", "Sedna", "Axi"));
    }

    #[test]
    fn mission_names() {
        assert_eq!(mission_name("MT_MOBILE_DEFENSE"), "Mobile Defense");
        assert_eq!(mission_name("MT_SURVIVAL"), "Survival");
        assert_eq!(mission_name("MT_CORRUPTION"), "Void Flood");
    }
}

#[cfg(test)]
mod live_check {
    /// Run with `WS_FILE=<path> cargo test live_world_state -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_world_state() {
        let text = std::fs::read_to_string(std::env::var("WS_FILE").unwrap()).unwrap();
        let f = super::parse_fissures(&text).unwrap();
        let v = super::views(&f, &crate::translate::nodes::NodeData::bundled());
        for x in &v {
            println!("{} {:8} {:20} {:10} {:16} {}", if x.steel_path { "SP" } else { "  " }, x.era, x.node, x.planet, x.mission_type, x.enemy);
        }
        assert!(!v.is_empty());
    }
}
