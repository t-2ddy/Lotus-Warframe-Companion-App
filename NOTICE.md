# Notices

Lotus's own code is under the [MIT licence](LICENSE). It also ships or uses the things below, which
belong to their owners and keep their own terms. Lotus is a free, non-commercial community project
and isn't affiliated with or endorsed by any of them.

## Warframe and its artwork

Warframe, the Lotus, and the game's names, icons and artwork are trademarks and copyright of
**[Digital Extremes Ltd.](https://www.digitalextremes.com)** Lotus uses some of that artwork in
its interface under Digital Extremes' allowance for free, non-commercial community projects:

- `static/icons/` (rank icons, mastered wreath, platinum, Endo, credits, relic eras, Vitus
  Essence, the Foundry icon)
- item images downloaded at run time from [cdn.warframestat.us](https://cdn.warframestat.us)

That artwork isn't covered by Lotus's MIT licence. Don't use it in anything sold.

## Bundled data

| File | From | Terms |
|---|---|---|
| `src-tauri/data/nodes.json` | [Warframe wiki](https://wiki.warframe.com) `Module:Missions/data`, reshaped by `scripts/gen_nodes.py` | [CC BY-SA 3.0](https://creativecommons.org/licenses/by-sa/3.0/): this file is shared under the same licence, with this attribution |
| `src-tauri/data/rivens.json` | [Warframe Public Export Plus](https://github.com/calamity-inc/warframe-public-export-plus) by calamity-inc, reshaped by `scripts/gen_rivens.py` | The source project's terms |
| Riven formulas in `src-tauri/src/rivens.rs` | Ported from [warframe-riven-info](https://github.com/calamity-inc/warframe-riven-info) by calamity-inc | The source project's terms |
| Daily and weekly tasks in `src/lib/Checklist.svelte` | [TipsyContent's Warframe checklist](https://tipsycontent.com/warframe/checklist) | Used with credit, for a community project |

## Downloaded at run time

[WFCD warframe-items](https://github.com/WFCD/warframe-items) (MIT),
[warframe.market](https://warframe.market),
[browse.wf](https://browse.wf),
Digital Extremes' public world state, and the community
["Good riven rolls" sheet](https://docs.google.com/spreadsheets/d/1zbaeJBuBn44cbVKzJins_E3hTDpnmvOk8heYN-G8yy8)
by 44bananas and Xennethkeisere. Each is used within its rate limits, with the app's User-Agent.

## Libraries

[Lucide](https://lucide.dev) icons (ISC). The Rust and npm dependencies are listed in
`src-tauri/Cargo.lock` and `package-lock.json`, each under its own licence.
