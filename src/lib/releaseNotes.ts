// Short bug-fix notes, shown once in the app after it updates to that version (newest first).
// One line per fix, in plain words. CHANGELOG.md has the details.
//
// Add an entry before each release: `npm run bump -- x.y.z` refuses a version without one.

export type ReleaseNote = { version: string; fixes: string[] };

export const RELEASE_NOTES: ReleaseNote[] = [
  {
    version: "1.0.0",
    fixes: [
      "New Chat page: save your Trade and Recruit messages and copy them into the game in one click, with item name and emoji suggestions.",
      "Part blueprints like Riot-848's Barrel show their real names instead of names made from game file paths.",
      "The New Strange quest items show as Scorched Beacon, Chroma Signal and Chroma Mark.",
    ],
  },
  {
    version: "0.1.13",
    fixes: [
      "Riven Ammo Efficiency shows the right way round: \"+55.5% Ammo Efficiency\" is a positive, on the Rivens page and in the overlay.",
    ],
  },
  {
    version: "0.1.12",
    fixes: [
      "Foundry's Incarnon filter shows every weapon that can get an Incarnon form, with all its variants, not only ones you've already unlocked.",
      "Riven ideal stats no longer mark Blast or the weak point stats as Best.",
    ],
  },
  {
    version: "0.1.11",
    fixes: [
      "Rivens with spliced stats get a grade again instead of \"Collecting data\".",
      "Combined elements and weak point stats count as good riven stats; damage to the new factions doesn't.",
      "Corrosive no longer shows twice (as \"Corro\") in a riven's ideal stats, and Status damage and Slam damage show by name.",
      "Right-clicking no longer opens the browser menu.",
    ],
  },
  {
    version: "0.1.10",
    fixes: [
      "New Mastery Helper tab, Crafting trees: every item built from other weapons, with what you've mastered along the way.",
      "Click one to see its crafting tree, and click any weapon in it for its recipe and where to get it.",
      "Filter the tab to your favorites or hide what you've already mastered.",
    ],
  },
  {
    version: "0.1.9",
    fixes: [
      "Click a Foundry card to see its full recipe and where to get every part: mission, relic, bounty, enemy, vendor, Dojo lab or Market.",
      "MOAs, Hounds, Predasites and Vulpaphylas only count as mastered once gilded; Mastery Helper shows how many more times to max them.",
      "Dual weapons like Akbronco need two copies of the single weapon again, instead of one copy counting twice.",
      "The Foundry opens instantly, and its cards lift on hover.",
      "Clearer Ducanator filters: choose what to show, and which parts to keep for building.",
      "Prime Sets lists only sets you own in full; the Warframe Market tab is now Trade Chat.",
    ],
  },
  {
    version: "0.1.8",
    fixes: [
      "The relic rewards overlay works again: the item data Lotus uses had lost every relic's rewards.",
      "Overlays no longer show twice: only one Lotus runs at a time, and opening it again brings it forward.",
      "Riven reroll overlay: the riven you keep is read again after every choice, and half-faded cards are read again.",
      "Riven overlays read a Gas line its icon garbled (\"ecas\") correctly.",
      "Riven stats are up to date: shotgun rivens roll Zoom again, and spliced traits like Gas and Status Damage are graded.",
      "Foundry pictures and part circles scale with the window and are the same size on every card.",
      "New Inventory tab, Prime Sets: sets you own, with set and part prices; click Sets owned or Set price to sort.",
    ],
  },
  {
    version: "0.1.7",
    fixes: [
      "No changes in the app itself: this release fixes a test that sometimes failed during the build.",
    ],
  },
  {
    version: "0.1.6",
    fixes: [
      "Rivens now use disposition 1.2 for every weapon, matching the game's change, so their numbers are right again.",
    ],
  },
  {
    version: "0.1.5",
    fixes: [
      "Foundry and the relic overlay now count a part you hold as a blueprint as owned.",
      "Mastery Helper: Easy now also lists sets you can build, and hovering a card shows its XP on the rank bar.",
      "Mastery Helper: click a From relics or With platinum card for part prices, relic drops and the set price.",
      "Mastery Helper: the rank bar stays at the top while you scroll.",
    ],
  },
  {
    version: "0.1.4",
    fixes: [
      "Overlays no longer show up late or flicker: Lotus now gets the game's log lines as they happen.",
      "Relic rewards overlay now works when you're the squad's host.",
      "Relic reward panels stay up for the whole 15 s choice, even if you alt-tab out and back.",
      "Rerolled rivens are no longer read mid-animation (which gave wrong numbers).",
      "Rerolling an unranked riven no longer fails to grade.",
      "The reroll panels stay open while the \"Choose a trait to lock\" menu is up.",
      "Locked traits on riven cards are no longer dropped or mixed into another stat.",
    ],
  },
];
