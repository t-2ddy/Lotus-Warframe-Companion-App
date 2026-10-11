# Changelog

All notable changes to Lotus. Versions follow [Semantic Versioning](https://semver.org); while
it's 0.x, anything may still change.

## [Unreleased]

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

[Unreleased]: https://github.com/t-2ddy/Lotus-Warframe-Companion-App/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/t-2ddy/Lotus-Warframe-Companion-App/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/t-2ddy/Lotus-Warframe-Companion-App/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/t-2ddy/Lotus-Warframe-Companion-App/releases/tag/v0.1.0
