# Changelog

All notable changes to Lotus. Versions follow [Semantic Versioning](https://semver.org); while
it's 0.x, anything may still change.

## [Unreleased]

## [0.1.3] — 2026-10-10

### Changed
- Riven overlay: each card is read four times at once (as shown, and three cleaned-up
  black-on-white copies) and every number and label takes the majority answer, so one bad read
  ("+1 91 .1%", "+34") no longer breaks a riven. Takes about as long as one read did.
- Riven overlay: misread stat labels ("Mulitshot", "Critica1 Damage") match the closest stat
  when only one is that close.
- Riven overlay: once a chat riven is worked out, a later misread of the same card (Torid
  Sati-caritox's "+111.7%" read as "+1 1 .70/0") no longer replaces it with an error.
- Riven overlay: whole percentages whose "%" OCR reads as "0/0" ("+109%" read as "+1 090/0")
  are read as 109%, not 1090.
- Riven overlay: riven stats never reach four digits before the decimal point, so a number
  that does is a misread: one that lost its point ("+1374%") is read as 137.4%, and any other
  makes the overlay read the card again instead of grading it.
- Rivens: weapons newer than the game's data export (Corufell Prime, Steflos Prime, Quassus
  Prime, Kuva Ghoulsaw) are taken from the Warframe wiki and join their family, so their rivens
  work out at the right disposition (Corufell Prime 0.5, not Corufell's 1.05).
- Rivens: "Chance to Gain Combo Count" (melee, negative only) is always the negative. A
  "-12.4%" roll was counted as a positive, so cards with it didn't work out, and owned rivens
  showed it as "+12.4%".
- Riven overlay: the reroll screen's current riven shows as soon as the screen opens, like a chat
  riven, instead of after the game's log catches up (often several seconds).
- Riven overlay: only shows for chat links and on the reroll screen, never on the mod screen's
  riven list (its riven cards were taken for a chat riven). A chat link opened from the mod
  screen still gets it.
- Riven overlay: on the reroll screen the new roll shows as soon as it's on screen, and after
  choosing a roll the overlay drops straight back to one panel for the kept riven. Both used to
  wait for the game's log (seconds late); the screen's CYCLE FOR / CONFIRM button now says when
  a cycle or a confirm happened. That includes having too little Kuva for the next cycle ("INSUFFICIENT
  KUVA" instead of "CYCLE FOR"), and unrelated dialogs in the log no longer keep the overlay up.
- Riven overlay: backing out of the reroll screen to the riven list hides the overlay within
  about half a second (it watches the screen's CYCLE/CONFIRM button instead of re-reading the
  card every half second).
- Riven overlay: a stat whose number OCR can't read no longer stops the riven being graded. A
  damaged number ("+1 f Electricity", the icon took the digits and the "%") or a positive the
  card read missed entirely (the riven's name lists every positive) is shown as "not read" with
  the range its number must be in, and the rest of the riven is graded. Chat cards are read
  again first in case a cleaner read comes through.
- Rivens: a stat the riven's type can't roll any more (DE removed it, e.g. Zoom on shotgun
  rivens) no longer stops the riven being read. It's shown with its number, marked "no longer
  rolls" and left ungraded; the rest of the riven is graded as usual.

## [0.1.2] — 2026-10-10

### Fixed
- Pages now open at the top instead of at the previous page's scroll position.
- Riven overlay: reads cards whose Slash label is hidden by its icon, both as a positive stat
  (worked out from the riven's name) and as the negative (tried as each damage type).
- Riven overlay: rivens for weapons whose disposition changed recently (e.g. Haalvu 0.5 → 0.6
  in September 2026) work out again. Dispositions now come from the Warframe wiki, which
  follows DE's changes; the game's data export doesn't.
- Riven overlay: a faction stat read before its number ("Damage to Infested" without "x0.98")
  is no longer shown as a new trait; the card is read again until the number shows.
- Riven overlay: when the game doesn't say which variant a riven is shown with and the base
  weapon's numbers don't fit, the variants are tried (yours first), e.g. Kompressa Prime.
- Riven overlay: low-rank rivens fit even though the card rounds their small numbers.
- Rivens: archgun rivens with Damage to Infested are graded instead of "collecting data"
  (archguns can't roll it any more, so the game data leaves it out, but older rivens keep it).

## [0.1.1] — 2026-10-10

Version bump only, to test the auto-update path from 0.1.0.

## [0.1.0] — 2026-10-10

First public beta.

### Added
- **Inventory capture:** reads your inventory at login and at mission end (EE.log triggers, a
  read-only memory read), and keeps a local history.
- **Home:** wallet, daily and weekly checklists (remove or add tasks, hide either list in
  Tools), arbitrations with tiers, live Void Fissures, and 30-day charts of platinum, Endo,
  credits and relics opened.
- **Foundry:** every masterable item with its parts and owned counts, mastered, owned and
  buildable status, favourites and filters.
- **Inventory:** parts, relics (sortable counts and rewards), mods (with warframe.market prices),
  arcanes and misc.
- **Mastery Helper:** rank progress, a breakdown by category with progress rings, and the fastest
  ways to gain mastery (easy, from relics, with platinum).
- **Rivens:** every owned riven with roll letters, grade, perfectness, the weapon's ideal stats
  and similar listings on warframe.market. Spliced traits show as "collecting data".
- **Ducanator:** prime parts by ducats per platinum (warframe.market's Ducats tool), with filters
  by part type, rarity, unmastered items, set ownership and spare copies.
- **Riven overlay:** grades riven cards from chat links and the reroll screen in game.
- **Relic rewards overlay:** plat, ducats, vaulted status and set progress under each Void
  Fissure reward card.
- **App:** first-run welcome screen, custom title bar, remembered window size, plain-language
  network errors and an offline banner, opt-in debug logging, and signed auto-updates from
  GitHub Releases.

[Unreleased]: https://github.com/t-2ddy/Lotus-Warframe-Companion-App/compare/v0.1.3...HEAD
[0.1.3]: https://github.com/t-2ddy/Lotus-Warframe-Companion-App/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/t-2ddy/Lotus-Warframe-Companion-App/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/t-2ddy/Lotus-Warframe-Companion-App/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/t-2ddy/Lotus-Warframe-Companion-App/releases/tag/v0.1.0
