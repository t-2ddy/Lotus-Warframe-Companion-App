//! History for the Home page's "personal stats" charts.
//!
//! Every capture stores what changed, so a currency's history is its current value plus each
//! recorded change. The page buckets these into local days (it knows the time zone).
//!
//! Relics opened are worked out from the same changes: in each capture, a relic's counts across
//! its refinements (Intact … Radiant) are added up, and a net drop is that many opened. Refining
//! only moves a relic between refinements, so it doesn't count. Relics traded away do.

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use std::collections::BTreeMap;

use crate::pipeline::Shared;
use crate::translate::CURRENCY;

/// Charted currencies: inventory key and label, in page order.
const CHARTED: [(&str, &str); 3] = [("PremiumCredits", "Platinum"), ("FusionPoints", "Endo"), ("RegularCredits", "Credits")];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub at: i64,
    pub before: i64,
    pub after: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Series {
    pub label: String,
    pub current: i64,
    /// Oldest first.
    pub changes: Vec<Point>,
    /// A count per day (relics opened) rather than a running value: each point's `after - before`
    /// is what happened in that capture.
    pub per_day: bool,
}

/// Relic paths (`…/Projections/T1VoidProjectionMesaPrimeCBronze`) end in their refinement.
const REFINEMENTS: [&str; 4] = ["Bronze", "Silver", "Gold", "Platinum"];

/// Relics opened per capture, from the relic count changes (see the module notes).
pub fn relics_opened(changes: &[(i64, i64, String, i64, i64)]) -> Vec<Point> {
    let mut per_capture: BTreeMap<(i64, i64), BTreeMap<&str, i64>> = BTreeMap::new();
    for (snapshot, at, item, before, after) in changes {
        let relic = REFINEMENTS.iter().find_map(|r| item.strip_suffix(r)).unwrap_or(item);
        *per_capture.entry((*snapshot, *at)).or_default().entry(relic).or_insert(0) += after - before;
    }
    per_capture
        .into_iter()
        .filter_map(|((_, at), relics)| {
            let opened: i64 = relics.values().filter(|n| **n < 0).map(|n| -n).sum();
            (opened > 0).then_some(Point { at, before: 0, after: opened })
        })
        .collect()
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrencyHistory {
    /// First capture of this account (unix ms): there's no data before it.
    pub since: Option<i64>,
    pub series: Vec<Series>,
}

#[tauri::command]
pub fn get_currency_history(shared: State<'_, Arc<Shared>>) -> Result<CurrencyHistory, String> {
    let guard = shared.store.lock().unwrap();
    let Some(store) = guard.as_ref() else { return Ok(CurrencyHistory::default()) };
    let Some(account) = store.latest_account().map_err(|e| e.to_string())? else {
        return Ok(CurrencyHistory::default());
    };
    let current = store.current(&account).map_err(|e| e.to_string())?;
    let mut series = Vec::new();
    for (key, label) in CHARTED {
        let Some(&value) = current.get(&(CURRENCY.to_string(), key.to_string())) else { continue };
        let changes = store
            .item_history(&account, CURRENCY, key)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|(at, before, after)| Point { at, before, after })
            .collect();
        series.push(Series { label: label.to_string(), current: value, changes, per_day: false });
    }
    let relic_changes = store.section_history(&account, "MiscItems", "/Projections/").map_err(|e| e.to_string())?;
    let opened = relics_opened(&relic_changes);
    series.push(Series {
        label: "Relics opened".to_string(),
        current: opened.iter().map(|p| p.after).sum(),
        changes: opened,
        per_day: true,
    });
    Ok(CurrencyHistory { since: store.first_capture(&account).map_err(|e| e.to_string())?, series })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_opened_relics_but_not_refining() {
        let p = "/Lotus/Types/Game/Projections/T1VoidProjectionMesaPrimeC";
        let q = "/Lotus/Types/Game/Projections/T2VoidProjectionAtlasPrimeA";
        let ch = |s: i64, item: String, before: i64, after: i64| (s, s * 1000, item, before, after);
        let changes = vec![
            // Capture 1: refined one Intact to Radiant (not opened), opened two of another relic.
            ch(1, format!("{p}Bronze"), 3, 2),
            ch(1, format!("{p}Platinum"), 0, 1),
            ch(1, format!("{q}Bronze"), 5, 3),
            // Capture 2: opened the Radiant one, and got a new relic from a mission.
            ch(2, format!("{p}Platinum"), 1, 0),
            ch(2, format!("{q}Silver"), 0, 1),
            // Capture 3: only gains.
            ch(3, format!("{q}Bronze"), 3, 6),
        ];
        let opened = relics_opened(&changes);
        let got: Vec<(i64, i64)> = opened.iter().map(|p| (p.at, p.after)).collect();
        assert_eq!(got, vec![(1000, 2), (2000, 1)]);
    }
}
