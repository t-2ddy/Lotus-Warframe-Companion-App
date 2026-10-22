// Short bug-fix notes, shown once in the app after it updates to that version (newest first).
// One line per fix, in plain words. CHANGELOG.md has the details.
//
// Add an entry before each release: `npm run bump -- x.y.z` refuses a version without one.

export type ReleaseNote = { version: string; fixes: string[] };

export const RELEASE_NOTES: ReleaseNote[] = [
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
