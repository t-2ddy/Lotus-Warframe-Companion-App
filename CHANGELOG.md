# Changelog

All notable changes to Lotus. Versions follow [Semantic Versioning](https://semver.org); while
it's 0.x, anything may still change.

## [Unreleased]

### Added
- Inventory → Prime Sets: every prime set you hold a part of, shown by its main blueprint, with
  how many complete sets you have and the set's warframe.market price (WTS/WTB). Click a set for
  each part: how many you hold and its price. Only tradable copies count (blueprints and prime
  parts, not built warframe parts or weapons); resources like Orokin Cells aren't set parts.
  Filters for full sets and vaulted. Click the Sets owned or Set price column to sort: highest
  first, then lowest first, then back to name order.

### Fixed
- Relic reward overlay: showed nothing, because WFCD's item data (2026-10-23) lists every relic
  with no rewards. Relics without rewards now get them from WFCD's drop data
  (drops.warframestat.us), or from a snapshot that ships with the app
  (`scripts/gen_relic_rewards.py`). This also fixes relic rewards in Inventory → Relics.
- Item data: a downloaded file that's broken (doesn't parse, is empty, or has relics without
  rewards) no longer replaces the cached copy.
- Overlays showed twice: only one Lotus runs at a time now. Opening it again brings the running
  window forward. A second copy drew its own overlays and, without the live game log, read
  riven cards seconds late, so it sometimes showed the old roll as the new one.
- Riven overlays: a spliced Gas line read as "ecas" or "eas" (its icon or a letter misread) is
  now recognised, and when the screen's several reads disagree on a label, one that names a
  stat beats junk. A Kuva Sobek riven with Gas failed with "stat not recognised".
- Riven reroll overlay: a new roll caught mid-fade with a cut-off name ("Boar crt") is read again
  instead of being shown.

- Rivens: melee rivens could get a second "Damage to Grineer" (and other factions) under the
  wrong tag, depending on load order.

### Changed
- Foundry: item art and part circles scale with the window and are the same size on every
  card. Before, circles shrank to fit their set, so a 5-part card had smaller ones than a
  3-part card, and all of them jumped when the column count changed.
- Riven data: stats now also come from DE's own Public Export, which is ahead of Public Export
  Plus. Shotgun rivens roll Zoom again, and the spliced traits (Gas, Viral, Corrosive, Radiation,
  Blast, Magnetic, Status Damage, Weak Point Damage and Critical Chance, Ammo Efficiency,
  Magazine Reloaded when Holstered, Heavy Attack Damage and Wind Up, Parry Angle, Slam Damage,
  Damage to Orokin/Techrot/Scaldra) have their real ranges: they're graded like any stat
  instead of "collecting data". The grading sheet still doesn't cover spliced traits. The app
  checks DE's export at the first start with this version.
- Riven names: where a spliced trait shares an older stat's name part ("laci"/"nus"), the name
  counts it once, as the older stat.
- Riven reroll overlay: after choosing between the old and new roll, the kept riven is read
  again and shown as the current one every time.

## [0.1.7] — 2026-10-23

### Fixed
- Build: the image cache tests each use their own folder, so one test's cleanup can no longer
  delete another's files mid-run (`downloads_once_then_serves_from_disk` failed on CI).

## [0.1.6] — 2026-10-23

### Changed
- Rivens: every riven now uses disposition 1.2 (4/5 dots), matching the game, where DE set all
  weapons to the same disposition. This applies to the Rivens page, the popup and the riven
  overlays. Sort by disposition is gone from the Rivens page, since every riven ties.

### Removed
- Riven overlay: no longer tries a disposition 0.05–0.1 away when a riven's numbers don't fit
  the data's value. With every weapon at 1.2, that could only accept a misread.

## [0.1.5] — 2026-10-21

### Added
- Mastery Helper, Easy: hovering a card shows on the rank bar how much XP the item adds, and the
  percentage it takes you to (or "rank up").
- Mastery Helper, With platinum: click a card for the set's price and each part's price. What to
  buy is highlighted: the missing parts, or the whole set when you own none of its parts.
- Mastery Helper, From relics: click a card for the missing parts, the owned relics that drop
  them (with Intact chances) and their prices.
- Mastery Helper: hovering a row in Game content, Star chart or Intrinsics grows its slice of the
  ring.
- Foundry: a Wiki link on every card.

### Changed
- Mastery Helper: the rank header stays at the top of the page and shrinks as you scroll down.
- Mastery Helper, Easy: also lists sets ready to build and full sets (every part held, some still
  blueprints), after the items you own or are building. Chips read Inventory, Foundry, Ready to
  build and Full set.
- Foundry: a set's part circles stay on one row and shrink to fit.
- Foundry: a warframe part counts as owned whether it's built or still a blueprint (a Chassis
  Blueprint counts toward the Chassis). The tooltip says how many are blueprints, and "Ready to
  build" still needs the parts built.

## [0.1.4] — 2026-10-14

### Added
- A short "what's fixed" popup, shown once after Lotus updates (also under Tools → What's new).
- Tools: **Bring Warframe to front for overlays** (on by default). When a reward screen, reroll or
  chat riven opens while another window is in front, Warframe is brought forward so its cards can
  be read.

### Changed
- Riven overlay: stats are always shown at max rank (rank 8), like the Rivens page, so an
  unranked riven's numbers show what it gives fully upgraded. The panel still says the riven's
  own rank.
- Overlays: the game's log lines are now read as the game writes them (its debug output), instead
  of from EE.log, which the game writes in bursts up to 20 s late. If another app (AlecaFrame,
  DebugView) already reads that channel, Lotus falls back to EE.log; the Tools page shows which.
- Relic rewards overlay: the panels open when the game says the reward cards are up and close 15 s
  later (or when the reward screen closes). If you're tabbed out, the cards are read when you come
  back, and the panels show for the rest of the 15 s.
- Relic rewards overlay: no panels in Requiem fissures.
- Riven overlay: the reroll screen, its cycles and confirms and its closing come from the log
  again, and chat rivens are looked for only after the log names one. The constant screen reading
  is gone.
- Overlays: the panels no longer hide when another window takes the front for less than a
  second, and the cards are never read while another window covers the game.
- Riven overlay: cards still fading in read far better. Two of the four cleaned-up copies now
  stretch the card's own brightness range instead of a fixed one; on real screenshots dimmed to
  40%, cards worked out right went from 17 of 70 to 66.
- Riven overlay: a number OCR misread in a common way (a lost decimal point, a lost or extra 1, a
  1 read as a 7) is repaired when exactly one repair makes every number fit; if none or several
  do, that stat shows as "not read" and the rest of the riven still works out.
- Riven overlay: a number split over two lines (".5% Status Chance" and "+1") is joined back.
- Riven overlay: a faction line read without its number ("Damage to Corpus") still counts as a
  stat, shown as "not read", instead of failing the card; a negative whose "-" was lost with its
  icon ("77.7% Impact") is read as the negative; a negative whose label came out as a few letters
  ("Z m") is the one stat with those letters whose numbers fit (Zoom).
- Riven overlay: a weapon name misread by a letter or two ("Maqistar", "Catabo/yst") is matched
  to the weapon when no other weapon is as close.
- Riven overlay: when a weapon's disposition in the data is out of date (Coda Bassocyst: 0.55 on
  the wiki, 0.6 in game), a riven whose numbers all fit a disposition 0.05–0.1 away is worked out
  at that disposition, instead of one of its stats being shown as "not read".

### Fixed
- Riven overlay: a locked negative read as "0-35.6% Zoom" (lock icon as "0") was dropped. And
  when the copies of a card each read its name differently, the first copy's stats were used on
  their own, even when it was the one that missed a stat; now the stats most copies agree on are
  used, and the shown name is a whole one ("Cell-ampicron", not "npicron").
- Riven overlay: a locked trait whose lock icon was read as a "0" right before the plus
  ("0+126.4% Critical Chance") was dropped, its wrapped "Chance" was glued onto the stat above
  ("Electricity Chance"), and both showed ungraded. A plus now always counts as a sign.
- Riven overlay: the panels no longer close when the "Choose a trait to lock" menu opens on the
  reroll screen.
- Riven overlay: rerolling an unranked (or part-ranked) riven failed with "couldn't grade". The
  reroll screen shows every riven at max rank, but its numbers were worked out at the riven's own
  rank.
- Riven overlay: a new roll at the reroll screen could show wrong numbers (Burston Prime: +266.5%
  Damage and -47.4% Zoom for a card showing +258.8% and -36.4%). It was read while still fading
  in, and at the riven's known rank, numbers that didn't fit were stretched to fit instead of
  rejected. Now such a read is rejected, and a new roll whose name doesn't read yet is read again.
- Relic rewards overlay: nothing showed when you were the squad's host (the game logs the reward
  screen differently for the host).

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

[Unreleased]: https://github.com/t-2ddy/Lotus-Warframe-Companion-App/compare/v0.1.7...HEAD
[0.1.7]: https://github.com/t-2ddy/Lotus-Warframe-Companion-App/compare/v0.1.6...v0.1.7
[0.1.6]: https://github.com/t-2ddy/Lotus-Warframe-Companion-App/compare/v0.1.5...v0.1.6
[0.1.5]: https://github.com/t-2ddy/Lotus-Warframe-Companion-App/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/t-2ddy/Lotus-Warframe-Companion-App/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/t-2ddy/Lotus-Warframe-Companion-App/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/t-2ddy/Lotus-Warframe-Companion-App/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/t-2ddy/Lotus-Warframe-Companion-App/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/t-2ddy/Lotus-Warframe-Companion-App/releases/tag/v0.1.0
