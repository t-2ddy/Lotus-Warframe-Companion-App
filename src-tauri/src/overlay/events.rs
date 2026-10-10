//! EE.log lines that drive the riven overlay (found by watching the log while using the game;
//! see `docs/plans/riven-overlay.md`).
//!
//! - Chat-link riven: `ThemedDetailedPurchaseDialog.lua: PopulateInfo->…/Randomized/<type>` names
//!   it; `…DBG: HudVis N` marks the dialog opening/closing (N = 1, or 2 over a riven list).
//!   The game writes EE.log in bursts (seconds late), so for chat rivens the screen leads and the
//!   log only supplies the riven type.
//! - Reroll screen: `Created /Lotus/Interface/OmegaRerollSelection.swf` opens it. Dialogs
//!   (`Dialog::CreateOkCancel(description=…)` then `Dialog::SendResult(4|5)`) carry each cycle
//!   ("Are you sure you want to cycle <Weapon Name> for 7,000?") and each confirm ("Cycle Riven
//!   into current selection?"). `DiegeticArtifactCards.lua: DBG: HudVis 0` closes the mod screen.
//! - The weapon is the `ResourceLoader … (/Lotus/Weapons/…)` line just before either screen.
//! - Void Fissures (see `docs/plans/relic-rewards.md`): `Mission name: …` starts a mission; each
//!   squad relic loads as `ResourceLoader … (/Lotus/Types/Game/Projections/…)`; the reward screen
//!   opens with `VoidProjections: OpenVoidProjectionRewardScreenRMI`, shows the cards at
//!   `ProjectionRewardChoice.lua: Got rewards` and closes with `Relic reward screen shut down`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenEvent {
    /// A weapon's resources started loading (the screen's "Fits in" weapon).
    WeaponLoaded { path: String },
    /// The item-details dialog is showing a riven (from a chat link), e.g. `LotusRifleRandomModRare`.
    RivenInspected { riven_type: String },
    /// The item-details dialog's visibility level changed (`HudVis N`): opening one writes its
    /// level (1, or 2 when opened over another dialog such as a chat riven list), closing writes
    /// the level below. The log can lag by seconds, so this only prompts a look at the screen.
    DetailsVisibility { level: u32 },
    /// The reroll screen opened.
    RerollOpened,
    /// The mod screen (which contains the reroll screen) closed.
    ModScreenClosed,
    /// A yes/no dialog opened with this text.
    DialogAsked { text: String },
    /// The dialog was answered: 4 = Yes, 5 = No.
    DialogAnswered { result: u32 },
    /// A mission is starting, e.g. "Kiliken (Venus) - Lith Fissure".
    MissionStarted { name: String },
    /// A squad member's relic loaded, e.g. `/Lotus/Types/Game/Projections/T1VoidProjectionIvaraPrimeABronze`.
    SquadRelic { path: String },
    /// The relic reward screen opened (the cards follow ~5 s later).
    RewardScreenOpened,
    /// The reward cards are showing.
    RewardsReady,
    /// The log named one player's reward (it doesn't for every player), as an item path.
    RewardLogged { path: String },
    /// The reward screen closed.
    RewardScreenClosed,
}

pub fn parse(line: &str) -> Option<ScreenEvent> {
    if let Some(rest) = after(line, "ThemedDetailedPurchaseDialog.lua: PopulateInfo->") {
        let path = rest.trim();
        if path.contains("/Randomized/") {
            let riven_type = path.rsplit('/').next().unwrap_or_default().to_string();
            return Some(ScreenEvent::RivenInspected { riven_type });
        }
        return None;
    }
    if let Some(rest) = after(line, "ThemedDetailedPurchaseDialog.lua: DBG: HudVis ") {
        return rest.trim().parse().ok().map(|level| ScreenEvent::DetailsVisibility { level });
    }
    if line.contains("Created /Lotus/Interface/OmegaRerollSelection.swf") {
        return Some(ScreenEvent::RerollOpened);
    }
    if line.contains("DiegeticArtifactCards.lua: DBG: HudVis 0") {
        return Some(ScreenEvent::ModScreenClosed);
    }
    if let Some(rest) = after(line, "Dialog::CreateOkCancel(description=") {
        let text = rest.split(", title=").next().unwrap_or(rest).trim().to_string();
        return Some(ScreenEvent::DialogAsked { text });
    }
    if let Some(rest) = after(line, "Dialog::SendResult(") {
        let result = rest.split(')').next()?.trim().parse().ok()?;
        return Some(ScreenEvent::DialogAnswered { result });
    }
    if let Some(rest) = after(line, "ThemedSquadOverlay.lua: Mission name: ") {
        return Some(ScreenEvent::MissionStarted { name: rest.trim().to_string() });
    }
    // Joining someone else's squad only writes the cached name: "Hellas (Mars) - Lith Fissure (SolNode58)".
    if let Some(rest) = after(line, "ThemedSquadOverlay.lua: Cached mission name=") {
        let name = rest.trim();
        let name = name.rsplit_once(" (").filter(|(_, node)| !node.contains(' ')).map_or(name, |(n, _)| n);
        return Some(ScreenEvent::MissionStarted { name: name.to_string() });
    }
    if line.contains("VoidProjections: OpenVoidProjectionRewardScreenRMI") {
        return Some(ScreenEvent::RewardScreenOpened);
    }
    if line.contains("ProjectionRewardChoice.lua: Got rewards") {
        return Some(ScreenEvent::RewardsReady);
    }
    if line.contains("ProjectionRewardChoice.lua: Relic reward screen shut down") {
        return Some(ScreenEvent::RewardScreenClosed);
    }
    if line.contains("VoidProjections: ") {
        if let Some(rest) = after(line, " gets reward ") {
            let path = rest.trim().replacen("/Lotus/StoreItems/", "/Lotus/", 1);
            return Some(ScreenEvent::RewardLogged { path });
        }
    }
    if line.contains("ResourceLoader") && line.contains("Found") {
        if let Some(start) = line.find("(/Lotus/Types/Game/Projections/") {
            let end = start + line[start..].find(')')?;
            return Some(ScreenEvent::SquadRelic { path: line[start + 1..end].to_string() });
        }
    }
    if line.contains("ResourceLoader") && line.contains("(/Lotus/Weapons/") && line.contains("Found") {
        let start = line.find("(/Lotus/Weapons/")? + 1;
        let end = start + line[start..].find(')')?;
        return Some(ScreenEvent::WeaponLoaded { path: line[start..end].to_string() });
    }
    None
}

/// "Are you sure you want to cycle Burston Crita-lexitis for 7,000?" → "Burston Crita-lexitis".
pub fn cycled_riven(dialog: &str) -> Option<&str> {
    let rest = dialog.strip_prefix("Are you sure you want to cycle ")?;
    Some(rest.rsplit_once(" for ")?.0.trim())
}

pub fn is_confirm(dialog: &str) -> bool {
    dialog.starts_with("Cycle Riven into current selection")
}

fn after<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    line.find(marker).map(|i| &line[i + marker.len()..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_logged_lines() {
        // Lines copied from the user's EE.log (2026-10-07).
        assert_eq!(
            parse("193.649 Script [Info]: ThemedDetailedPurchaseDialog.lua: PopulateInfo->/Lotus/StoreItems/Upgrades/Mods/Randomized/LotusRifleRandomModRare"),
            Some(ScreenEvent::RivenInspected { riven_type: "LotusRifleRandomModRare".into() })
        );
        assert_eq!(
            parse("899.170 Script [Info]: ThemedDetailedPurchaseDialog.lua: PopulateInfo->/Lotus/StoreItems/Types/Items/MiscItems/RivenIdentifier"),
            None,
            "not a riven"
        );
        assert_eq!(parse("575.254 Script [Info]: ThemedDetailedPurchaseDialog.lua: DBG: HudVis 0"), Some(ScreenEvent::DetailsVisibility { level: 0 }));
        assert_eq!(parse("3396.522 Script [Info]: ThemedDetailedPurchaseDialog.lua: DBG: HudVis 2"), Some(ScreenEvent::DetailsVisibility { level: 2 }));
        assert_eq!(parse("1685.253 Sys [Info]: Created /Lotus/Interface/OmegaRerollSelection.swf"), Some(ScreenEvent::RerollOpened));
        assert_eq!(parse("1701.679 Script [Info]: DiegeticArtifactCards.lua: DBG: HudVis 0"), Some(ScreenEvent::ModScreenClosed));
        assert_eq!(
            parse("1685.253 Sys [Info]: ResourceLoader 0x0000023BB8A4ACF0 (/Lotus/Weapons/Tenno/LongGuns/PrimeBurston/PrimeBurston) Found 1,091 items to load (0ms) [Heap: 1/2]"),
            Some(ScreenEvent::WeaponLoaded { path: "/Lotus/Weapons/Tenno/LongGuns/PrimeBurston/PrimeBurston".into() })
        );
        let ask = parse("1689.787 Script [Info]: Dialog.lua: Dialog::CreateOkCancel(description=Are you sure you want to cycle Burston Crita-lexitis for 7,000?, title= leftItem=/Menu/Confirm_Item_Yes, rightItem=/Menu/Confirm_Item_No)");
        let Some(ScreenEvent::DialogAsked { text }) = ask else { panic!("{ask:?}") };
        assert_eq!(cycled_riven(&text), Some("Burston Crita-lexitis"));
        assert!(is_confirm("Cycle Riven into current selection?"));
        assert_eq!(parse("1690.624 Script [Info]: Dialog.lua: Dialog::SendResult(4)"), Some(ScreenEvent::DialogAnswered { result: 4 }));
        assert_eq!(parse("1310.613 Script [Info]: Dialog.lua: SendResult_MENU_CANCEL()"), None);
    }

    #[test]
    fn parses_relic_run_lines() {
        // Lines copied from the user's EE.log (2026-10-09, Kiliken Lith fissure).
        assert_eq!(
            parse("3629.981 Script [Info]: ThemedSquadOverlay.lua: Mission name: Kiliken (Venus) - Lith Fissure"),
            Some(ScreenEvent::MissionStarted { name: "Kiliken (Venus) - Lith Fissure".into() })
        );
        assert_eq!(
            parse("3635.756 Sys [Info]: ResourceLoader 0x000002C601F152C0 (/Lotus/Types/Game/Projections/T1VoidProjectionIvaraPrimeABronze) Found 20 items to load (0ms) [Heap: 1/2]"),
            Some(ScreenEvent::SquadRelic { path: "/Lotus/Types/Game/Projections/T1VoidProjectionIvaraPrimeABronze".into() })
        );
        assert_eq!(
            parse("5868.484 Script [Info]: ThemedSquadOverlay.lua: Cached mission name=Hellas (Mars) - Lith Fissure (SolNode58)"),
            Some(ScreenEvent::MissionStarted { name: "Hellas (Mars) - Lith Fissure".into() }),
            "joining a squad only logs the cached name"
        );
        assert_eq!(parse("3837.189 Sys [Info]: VoidProjections: OpenVoidProjectionRewardScreenRMI"), Some(ScreenEvent::RewardScreenOpened));
        assert_eq!(parse("3842.539 Script [Info]: ProjectionRewardChoice.lua: Got rewards"), Some(ScreenEvent::RewardsReady));
        assert_eq!(
            parse("3842.346 Sys [Info]: VoidProjections: 589174613ade7fbd0b51c1e7 gets reward /Lotus/StoreItems/Types/Recipes/Components/FormaBlueprint"),
            Some(ScreenEvent::RewardLogged { path: "/Lotus/Types/Recipes/Components/FormaBlueprint".into() })
        );
        assert_eq!(parse("3842.343 Sys [Info]: VoidProjections: Client got reward info from 6114204186e16e3dae71231c"), None);
        assert_eq!(
            parse("3857.542 Script [Info]: ProjectionRewardChoice.lua: Relic reward screen shut down"),
            Some(ScreenEvent::RewardScreenClosed)
        );
    }
}
