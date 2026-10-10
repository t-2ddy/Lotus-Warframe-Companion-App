//! Read-only views for the UI: currencies and recent changes, with readable names.

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::pipeline::Shared;
use crate::translate::{ItemIndex, ACCOUNT, CURRENCIES, CURRENCY};

/// How many recent changes the UI shows.
const RECENT_LIMIT: usize = 60;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Currency {
    pub label: String,
    pub value: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeView {
    pub captured_at: i64,
    pub section: String,
    pub name: String,
    pub item: String,
    pub image: Option<String>,
    pub before: i64,
    pub after: i64,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub account: Option<String>,
    /// In-game name, from EE.log's "Logged in" line.
    pub account_name: Option<String>,
    /// Mastery rank (`PlayerLevel`): 0–30, then 31+ for Legendary 1+.
    pub mastery_rank: Option<i64>,
    pub last_stored_at: Option<i64>,
    pub snapshots: i64,
    pub known_items: usize,
    pub currencies: Vec<Currency>,
    pub recent: Vec<ChangeView>,
}

/// Readable label for an inventory section.
fn section_label(section: &str) -> &str {
    match section {
        "MiscItems" => "Items",
        "Suits" => "Warframes",
        "LongGuns" => "Primary",
        "Pistols" => "Secondary",
        "Melee" => "Melee",
        "Recipes" => "Blueprints",
        "PendingRecipes" => "Foundry",
        "XPInfo" => "Mastery XP",
        "RawUpgrades" | "Upgrades" => "Mods",
        "Consumables" => "Gear",
        CURRENCY => "Currency",
        other => other,
    }
}

fn currency_label(key: &str) -> String {
    CURRENCIES.iter().find(|(k, _)| *k == key).map_or(key, |(_, label)| label).to_string()
}

fn item_name(index: &ItemIndex, section: &str, item: &str) -> String {
    if section == CURRENCY { currency_label(item) } else { index.display_name(item) }
}

#[tauri::command]
pub fn get_overview(shared: State<'_, Arc<Shared>>) -> Result<Overview, String> {
    let index = shared.items.read().unwrap().clone();
    let guard = shared.store.lock().unwrap();
    let Some(store) = guard.as_ref() else {
        return Ok(Overview { known_items: index.len(), ..Default::default() });
    };
    let Some(account) = store.latest_account().map_err(|e| e.to_string())? else {
        return Ok(Overview { known_items: index.len(), ..Default::default() });
    };

    let current = store.current(&account).map_err(|e| e.to_string())?;
    let currencies = CURRENCIES
        .iter()
        .filter_map(|(key, label)| {
            let value = *current.get(&(CURRENCY.to_string(), key.to_string()))?;
            Some(Currency { label: label.to_string(), value })
        })
        .collect();

    let mastery_rank = current.get(&(ACCOUNT.to_string(), "PlayerLevel".to_string())).copied();

    let recent = store
        .recent_changes(&account, RECENT_LIMIT)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|c| ChangeView {
            captured_at: c.captured_at,
            section: section_label(&c.section).to_string(),
            name: item_name(&index, &c.section, &c.item),
            image: index.get(&c.item).and_then(|i| i.image.clone()),
            item: c.item,
            before: c.before,
            after: c.after,
        })
        .collect();

    let (last_stored_at, snapshots) = store.snapshot_info(&account).map_err(|e| e.to_string())?;
    let account_name = store.account_name(&account).map_err(|e| e.to_string())?;
    Ok(Overview {
        account: Some(account),
        account_name,
        mastery_rank,
        last_stored_at,
        snapshots,
        known_items: index.len(),
        currencies,
        recent,
    })
}
