# Contributing to Lotus

Thanks for helping. Bug reports, data fixes and pull requests are all welcome.

## Reporting a bug

[Open an issue](https://github.com/t-2ddy/Lotus-Warframe-Companion-App/issues/new/choose) with
the bug report form. For overlay problems (a riven card or reward read wrong), turn on **Tools →
Debug logging** first, reproduce it, and attach `overlay.log` (Tools → Open folder). Check it
before posting: it contains the text of the cards the overlay read.

Security problems: please don't open a public issue. See [SECURITY.md](SECURITY.md).

## Building

**You need:** Windows 10 or 11, [Node](https://nodejs.org) 22 or later with npm,
[Rust](https://rustup.rs) (MSVC toolchain) with the Visual Studio C++ Build Tools, and Python 3
only if you regenerate the bundled data.

```sh
npm install
npm run tauri dev        # run the app
```

**Checks** (CI runs the same on every pull request):

```sh
npm run check                       # Svelte / TypeScript
npm run build                       # the frontend (the Rust crate embeds it)
cd src-tauri && cargo test --lib    # Rust tests
cd src-tauri && cargo clippy --lib  # lints (warnings don't fail CI yet)
```

Some overlay tests read reference screenshots that aren't in the repository. They skip
themselves when the files are missing.

The README's [How it fits together](README.md#how-it-fits-together) and
[Project layout](README.md#project-layout) sections explain where things live.

## Pull requests

1. Fork the repository and branch from `main`.
2. Keep each pull request to one change, and describe what it fixes or adds.
3. Write code like the code around it: naming, comments and structure. Rust isn't run through
   `cargo fmt` yet, so don't reformat files you didn't otherwise change.
4. Add or update tests for parsing, formulas and data handling.
5. Make sure the checks above pass, then open the pull request. Fill in the template.

**Ground rules** (they keep Lotus safe to use):
- **Read-only:** never write to the game's memory, inject code, hook the game, or send it
  keyboard or mouse input.
- **No uploads:** nothing leaves the player's PC except requests for public data.
- **Be polite to the data sources:** respect warframe.market's and others' rate limits, cache
  results, and send the app's User-Agent (see `src-tauri/src/net.rs`).
- **No personal data in the repository:** no inventory dumps, logs with account details, or
  screenshots showing someone's account name.

## Releases (maintainers)

```sh
npm run bump -- 0.2.0
git commit -am "Release v0.2.0"
git tag v0.2.0 && git push && git push --tags
```

The Release workflow builds and signs the installer and drafts a GitHub Release. Update
`CHANGELOG.md`, then publish the draft.

## Licence

By contributing, you agree that your contributions are licensed under the project's
[MIT licence](LICENSE).
