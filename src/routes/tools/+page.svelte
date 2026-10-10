<script lang="ts">
  /** Tools: in-game helpers. Each one is off until switched on. */
  import { invoke } from "@tauri-apps/api/core";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { openWelcome } from "$lib/welcomeState.svelte";
  import { appUpdate, checkForUpdate, installUpdate } from "$lib/appUpdate.svelte";
  import { onMount } from "svelte";
  import RivenDataStatus from "$lib/RivenDataStatus.svelte";
  import { checklistHidden, setChecklistHidden, type ChecklistKind } from "$lib/checklistPrefs.svelte";

  const CHECKLISTS: { kind: ChecklistKind; label: string }[] = [
    { kind: "daily", label: "Hide daily checklist" },
    { kind: "weekly", label: "Hide weekly checklist" },
  ];

  type Settings = { rivenOverlay: boolean; relicOverlay: boolean; debugLogging: boolean };

  let settings = $state<Settings | null>(null);
  let error = $state<string | null>(null);
  let previewing = $state(false);
  let previewingRelics = $state(false);
  let relicError = $state<string | null>(null);
  let debugError = $state<string | null>(null);
  /** Where overlay.log and overlay-debug/ go. */
  let debugFolder = $state<string | null>(null);

  onMount(() => {
    invoke<Settings>("get_tool_settings")
      .then((s) => (settings = s))
      .catch((e) => (error = String(e)));
    invoke<string | null>("debug_folder").then((f) => (debugFolder = f));
  });

  async function setDebugLogging(enabled: boolean) {
    try {
      settings = await invoke<Settings>("set_debug_logging", { enabled });
      debugError = null;
    } catch (e) {
      debugError = String(e);
    }
  }

  async function openDebugFolder() {
    if (!debugFolder) return;
    const file = settings?.debugLogging ? `${debugFolder}\\overlay.log` : debugFolder;
    await revealItemInDir(file).catch((e) => (debugError = String(e)));
  }

  async function setOverlay(enabled: boolean) {
    try {
      settings = await invoke<Settings>("set_riven_overlay", { enabled });
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  async function setRelicOverlay(enabled: boolean) {
    try {
      settings = await invoke<Settings>("set_relic_overlay", { enabled });
      relicError = null;
    } catch (e) {
      relicError = String(e);
    }
  }

  async function previewRelics() {
    previewingRelics = true;
    await invoke("preview_relic_overlay").catch((e) => (relicError = String(e)));
    setTimeout(() => (previewingRelics = false), 12_000);
  }

  async function preview() {
    previewing = true;
    await invoke("preview_riven_overlay").catch((e) => (error = String(e)));
    setTimeout(() => (previewing = false), 12_000);
  }
</script>

<div class="page">
  <h1>Tools</h1>
  <p class="muted">
    In-game helpers. Each one is off unless you switch it on. New here?
    <button class="link" onclick={openWelcome}>Show the welcome screen</button>
  </p>

  <section class="tool">
    <div class="head">
      <h2>Riven overlay</h2>
      <label class="switch" title={settings?.rivenOverlay ? "Turn off" : "Turn on"}>
        <input
          type="checkbox"
          checked={settings?.rivenOverlay ?? false}
          disabled={!settings}
          onchange={(e) => setOverlay(e.currentTarget.checked)}
        />
        <span class="track"><span class="knob"></span></span>
      </label>
    </div>
    <p>
      When you open a riven from a chat link, or cycle one at the reroll screen, a panel appears beside it. It shows
      each stat's roll letter and bar, the riven's grade, similar rivens on warframe.market and the weapon's best
      attributes. At the reroll screen, your current riven is on the left and each new roll on the right. The panel
      hides when you close the riven or leave the screen.
    </p>
    <ul class="notes">
      <li>Warframe must run in <b>Borderless</b> or <b>Windowed</b> mode (Options → Display). Exclusive fullscreen draws over every overlay.</li>
      <li>The panel is click-through. Press <b>Ctrl+Tab</b> while it shows to click listings, and again (or Done) to go back to the game.</li>
      <li>It reads the game's log and the riven card on screen (Windows' built-in text recognition). Nothing is sent to the game. Your own rivens show exact rolls; other players' rolls are worked out from the card's numbers.</li>
      <li>English game text only for now.</li>
      <li>The weapon variant the numbers are for (Latron or Latron Prime) is read from the <b>Fits in</b> box at the bottom right. If that can't be read, the panel shows the base weapon's numbers with a note and updates when the game's log names the variant. A card whose numbers don't fit shows <b>Fetching riven data…</b> for up to 3 seconds first.</li>
      <li>Stat values and dispositions come from Warframe Public Export Plus. The app checks for new data daily and after a riven that doesn't fit.</li>
    </ul>
    <div class="actions">
      <button onclick={preview} disabled={previewing}>{previewing ? "Showing preview…" : "Preview"}</button>
      <span class="muted small">Shows the panel for 12 seconds with one of your rivens, over the game if it's running.</span>
    </div>
    <RivenDataStatus />
    {#if error}<p class="error">{error}</p>{/if}
  </section>

  <section class="tool">
    <div class="head">
      <h2>Relic rewards overlay</h2>
      <label class="switch" title={settings?.relicOverlay ? "Turn off" : "Turn on"}>
        <input
          type="checkbox"
          checked={settings?.relicOverlay ?? false}
          disabled={!settings}
          onchange={(e) => setRelicOverlay(e.currentTarget.checked)}
        />
        <span class="track"><span class="knob"></span></span>
      </label>
    </div>
    <p>
      At the end of a Void Fissure, a panel appears under each reward card. It shows the plat price, ducats, whether
      it's vaulted, how many you own against what the set needs, the set's parts as images with your counts, and
      whether you've built it. The panels hide when the reward screen closes.
    </p>
    <ul class="notes">
      <li>Prices for your squad's relics are fetched in the background while the mission plays, then refreshed for the cards on screen.</li>
      <li>Same as the riven overlay: <b>Borderless</b> or <b>Windowed</b> mode, click-through, <b>Ctrl+Tab</b> to click the warframe.market links.</li>
      <li>Owned counts come from your last inventory capture, so parts picked up earlier in the same mission aren't counted yet.</li>
      <li>Solo runs have no reward choice (the item is claimed automatically), so nothing shows.</li>
    </ul>
    <div class="actions">
      <button onclick={previewRelics} disabled={previewingRelics}>{previewingRelics ? "Showing preview…" : "Preview"}</button>
      <span class="muted small">Shows four rewards from one relic for 12 seconds, laid out like the reward screen.</span>
    </div>
    {#if relicError}<p class="error">{relicError}</p>{/if}
  </section>

  <section class="tool">
    <div class="head">
      <h2>App updates</h2>
    </div>
    <p>
      Lotus checks GitHub for a new version when it starts and every 6 hours, and offers to update in a bar at the
      top. Updates are signed, so only official releases install.
    </p>
    <div class="actions">
      <button onclick={() => checkForUpdate(true)} disabled={appUpdate.checking || appUpdate.installing}>
        {appUpdate.checking ? "Checking…" : "Check for updates"}
      </button>
      {#if appUpdate.available && !appUpdate.installing}
        <button onclick={installUpdate}>Update to {appUpdate.available.version} and restart</button>
      {/if}
      <span class="muted small">
        You have Lotus {appUpdate.current ?? "…"}{appUpdate.message ? ` · ${appUpdate.message}` : ""}
      </span>
    </div>
  </section>

  <section class="tool">
    <div class="head">
      <h2>Debug logging</h2>
      <label class="switch" title={settings?.debugLogging ? "Turn off" : "Turn on"}>
        <input
          type="checkbox"
          checked={settings?.debugLogging ?? false}
          disabled={!settings}
          onchange={(e) => setDebugLogging(e.currentTarget.checked)}
        />
        <span class="track"><span class="knob"></span></span>
      </label>
    </div>
    <p>
      For reporting a bug with the overlays. While it's on, the app writes what the overlays saw and did to
      <b>overlay.log</b>, and keeps the last screen image it read for each card area in <b>overlay-debug</b>. Attach
      those to a bug report. Turning it off deletes both.
    </p>
    <ul class="notes">
      <li>Off by default. The images only show the riven card or reward names the overlay reads, nothing else on screen.</li>
      <li>Riven cards that don't fit the stat data are always noted in <b>riven_misfits.jsonl</b> (card text only): that's how the app knows to fetch newer riven data.</li>
    </ul>
    <div class="actions">
      <button onclick={openDebugFolder} disabled={!debugFolder}>Open folder</button>
      {#if debugFolder}<span class="muted small mono">{debugFolder}</span>{/if}
    </div>
    {#if debugError}<p class="error">{debugError}</p>{/if}
  </section>

  <section class="tool">
    <div class="head">
      <h2>Home checklists</h2>
    </div>
    <p>The Daily and Weekly checklists on the Home page. Hidden ones keep their ticks.</p>
    {#each CHECKLISTS as c (c.kind)}
      <div class="toggle-row">
        <span>{c.label}</span>
        <label class="switch" title={checklistHidden[c.kind] ? "Show it again" : "Hide it"}>
          <input
            type="checkbox"
            checked={checklistHidden[c.kind]}
            onchange={(e) => setChecklistHidden(c.kind, e.currentTarget.checked)}
          />
          <span class="track"><span class="knob"></span></span>
        </label>
      </div>
    {/each}
  </section>
</div>

<style>
  .page {
    max-width: 900px;
  }

  h1 {
    margin: 0 0 4px;
  }

  .muted {
    color: var(--muted);
    margin: 0 0 20px;
  }

  .link {
    all: unset;
    color: var(--accent);
    cursor: pointer;
  }

  .link:hover {
    text-decoration: underline;
  }

  .mono {
    font-family: Consolas, "Cascadia Mono", monospace;
    word-break: break-all;
  }

  .small {
    font-size: 13px;
    margin: 0;
  }

  .tool + .tool {
    margin-top: 16px;
  }

  .tool {
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 16px 18px;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .toggle-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 0;
    border-top: 1px solid var(--line);
  }

  h2 {
    margin: 0;
    font-size: 18px;
  }

  .tool p {
    margin: 8px 0 0;
    color: var(--muted);
    line-height: 1.5;
  }

  .notes {
    margin: 10px 0 0;
    padding-left: 18px;
    color: var(--muted);
    font-size: 14px;
    line-height: 1.6;
  }

  .notes b {
    color: var(--text);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 14px;
  }

  .actions button {
    font: inherit;
    color: var(--text);
    background: var(--surface-2);
    border: 1px solid var(--line-strong);
    border-radius: 6px;
    padding: 6px 14px;
    cursor: pointer;
  }

  .actions button:hover:not(:disabled) {
    background: var(--surface-hover);
  }

  .actions button:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .error {
    color: var(--bad) !important;
  }

  /* On/off switch */
  .switch {
    margin-left: auto;
    position: relative;
    cursor: pointer;
  }

  .switch input {
    position: absolute;
    opacity: 0;
  }

  .track {
    display: block;
    width: 42px;
    height: 24px;
    border-radius: 999px;
    background: var(--surface-2);
    border: 1px solid var(--line-strong);
    transition: background 0.15s;
  }

  .knob {
    display: block;
    width: 18px;
    height: 18px;
    margin: 2px;
    border-radius: 50%;
    background: var(--muted);
    transition: transform 0.15s, background 0.15s;
  }

  .switch input:checked + .track {
    background: var(--accent);
    border-color: var(--accent);
  }

  .switch input:checked + .track .knob {
    transform: translateX(18px);
    background: white;
  }

  .switch input:disabled + .track {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .switch input:focus-visible + .track {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
