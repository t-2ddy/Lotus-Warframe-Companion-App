<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { friendlyError } from "$lib/errors";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { hideOnError, imageUrl, markMissing, time } from "$lib/format";
  import Check from "@lucide/svelte/icons/check";

  type Row = {
    path: string;
    name: string;
    image: string | null;
    /** Blueprints: the built item, drawn over the blueprint picture. */
    overlay: string | null;
    kind: "warframe" | "weapon" | "companion" | "other";
    /** Relic tier from the ducat value. */
    tier: "gold" | "silver" | "bronze";
    owned: number;
    /** Progress on the part's set, in parts; 0/0 if not in a known set. */
    setOwned: number;
    setNeeded: number;
    /** Copies beyond what the set needs: safe to sell. */
    spare: number;
    /** Builds an item you haven't mastered yet. */
    forUnmastered: boolean;
    ducats: number;
    median: number;
    waPrice: number;
    volume: number;
    perPlat: number;
    perPlatMedian: number;
    trendDay: number;
  };
  type Data = {
    rows: Row[];
    account: string | null;
    updatedAt: number | null;
    fetching: boolean;
    error: string | null;
  };

  /** Fewer trades than this in the day: the price may be a fluke, so the row is flagged. */
  const LOW_VOLUME = 5;

  let data = $state<Data | null>(null);
  let loadError = $state<string | null>(null);
  let view = $state<"owned" | "all">("owned");
  let search = $state("");

  // Filters. "Show" chips list a group of parts (all on by default); "Keep for building" rules
  // leave out parts you may still build with. Saved as what's hidden, as before.
  type Hide = "warframe" | "weapon" | "companion" | "notMastered" | "gold" | "silver" | "bronze";
  type Chip = { key: Hide; label: string; title: string };
  const PART_TYPES: Chip[] = [
    { key: "warframe", label: "Warframe", title: "Warframe blueprints and parts. Click to show or hide" },
    { key: "weapon", label: "Weapon", title: "Weapon blueprints and parts. Click to show or hide" },
    { key: "companion", label: "Companion", title: "Sentinel blueprints and parts, and the Kavasa Kubrow collar. Click to show or hide" },
  ];
  const RARITIES: (Chip & { ducats: string })[] = [
    { key: "bronze", label: "Bronze", ducats: "15–25", title: "Common relic parts (15 or 25 ducats). Click to show or hide" },
    { key: "silver", label: "Silver", ducats: "45–65", title: "Uncommon relic parts (45 or 65 ducats). Click to show or hide" },
    { key: "gold", label: "Gold", ducats: "100", title: "Rare relic parts (100 ducats). Click to show or hide" },
  ];
  /** Leave out parts of sets you own at least this share of (one choice, or none). */
  const SET_SHARES = [100, 75, 50, 25];
  const HIDE_KEY = "ducanator.hide";
  /** `keepDupes` ("Still show spare copies"): the "may still need" filters keep parts you have
   *  more copies of than the set needs. */
  type HideState = Record<Hide, boolean> & { setShare: number | null; keepDupes: boolean };
  const DEFAULT_HIDE: HideState = {
    warframe: false,
    weapon: false,
    companion: false,
    notMastered: false,
    gold: false,
    silver: false,
    bronze: false,
    setShare: null,
    keepDupes: true,
  };
  function loadHide(): HideState {
    try {
      const saved = JSON.parse(localStorage.getItem(HIDE_KEY) ?? "{}");
      return { ...DEFAULT_HIDE, ...saved, setShare: SET_SHARES.includes(saved.setShare) ? saved.setShare : null };
    } catch {
      return { ...DEFAULT_HIDE };
    }
  }
  let hide = $state(loadHide());
  function saveHide() {
    try {
      localStorage.setItem(HIDE_KEY, JSON.stringify(hide));
    } catch {}
  }
  function toggleHide(key: Hide) {
    hide[key] = !hide[key];
    saveHide();
  }
  function toggleKeepDupes() {
    hide.keepDupes = !hide.keepDupes;
    saveHide();
  }
  /** The threshold the set rule uses when it's turned back on. */
  let lastShare = hide.setShare ?? 100;
  function toggleSetRule() {
    hide.setShare = hide.setShare === null ? lastShare : null;
    saveHide();
  }
  /** Picking a threshold also turns the set rule on. */
  function chooseSetShare(share: number) {
    hide.setShare = lastShare = share;
    saveHide();
  }
  /** A "keep" rule is on, so "still list spare copies" matters. */
  const keepRules = $derived(hide.notMastered || hide.setShare !== null);
  function clearFilters() {
    Object.assign(hide, DEFAULT_HIDE);
    saveHide();
  }
  const filtersSet = $derived(
    (Object.keys(DEFAULT_HIDE) as (keyof HideState)[]).some((k) => hide[k] !== DEFAULT_HIDE[k]),
  );
  const hidden = (r: Row) =>
    (hide.warframe && r.kind === "warframe") ||
    (hide.weapon && r.kind === "weapon") ||
    (hide.companion && r.kind === "companion") ||
    (hide.notMastered && r.forUnmastered && !(hide.keepDupes && r.spare > 0)) ||
    hide[r.tier] ||
    (hide.setShare !== null &&
      r.setNeeded > 0 &&
      r.setOwned * 100 >= hide.setShare * r.setNeeded &&
      !(hide.keepDupes && r.spare > 0));

  async function load() {
    try {
      data = await invoke<Data>("get_ducanator");
      loadError = null;
    } catch (e) {
      loadError = friendlyError(e, "the app");
    }
  }

  onMount(() => {
    load();
    const off = listen("ducanator-updated", load);
    return () => void off.then((f) => f());
  });

  // Column sorting, as on Inventory → Relics: high to low, then low to high, then back to the
  // default (ducats per plat, high to low).
  type Col = { key: keyof Row; label: string; title: string; optional?: boolean; ownedOnly?: boolean };
  const COLS: Col[] = [
    { key: "owned", label: "Owned", title: "How many you have", ownedOnly: true },
    { key: "ducats", label: "Ducats", title: "Ducats each at Baro's kiosks" },
    { key: "waPrice", label: "WA price", title: "Volume-weighted average trade price, previous day" },
    { key: "median", label: "Median", title: "Median trade price, previous day", optional: true },
    { key: "perPlat", label: "Ducats/plat", title: "Ducats ÷ WA price: the ranking warframe.market uses" },
    { key: "perPlatMedian", label: "D/p median", title: "Ducats ÷ median price", optional: true },
    { key: "volume", label: "Volume", title: "Trades in the previous day", optional: true },
    { key: "trendDay", label: "Trend", title: "Places moved in warframe.market's ranking over the day", optional: true },
  ];
  let sortKey = $state<keyof Row | null>(null);
  let sortDesc = $state(true);
  function sortBy(key: keyof Row) {
    if (sortKey !== key) [sortKey, sortDesc] = [key, true];
    else if (sortDesc) sortDesc = false;
    else sortKey = null;
  }

  const cols = $derived(COLS.filter((c) => view === "owned" || !c.ownedOnly));

  const rows = $derived.by(() => {
    if (!data) return [];
    const q = search.trim().toLowerCase();
    let list = data.rows.filter(
      (r) => (view === "all" || r.owned > 0) && !hidden(r) && (!q || r.name.toLowerCase().includes(q)),
    );
    const key = sortKey;
    if (key) {
      const dir = sortDesc ? -1 : 1;
      list = [...list].sort(
        (a, b) => dir * ((a[key] as number) - (b[key] as number)) || b.perPlat - a.perPlat || a.name.localeCompare(b.name),
      );
    }
    return list;
  });

  // Owned parts after the filters.
  /** Rows the view (Owned / All) and search allow, before the Hide filters: for "Hiding N of M". */
  const beforeFilters = $derived.by(() => {
    if (!data) return [];
    const q = search.trim().toLowerCase();
    return data.rows.filter((r) => (view === "all" || r.owned > 0) && (!q || r.name.toLowerCase().includes(q)));
  });
  const hiddenCount = $derived(beforeFilters.filter(hidden).length);

  const owned = $derived((data?.rows ?? []).filter((r) => r.owned > 0 && !hidden(r)));
  const summary = $derived({
    parts: owned.length,
    copies: owned.reduce((a, r) => a + r.owned, 0),
    ducats: owned.reduce((a, r) => a + r.owned * r.ducats, 0),
    plat: Math.round(owned.reduce((a, r) => a + r.owned * r.waPrice, 0)),
  });

  const fmt = (n: number, digits = 2) => (Number.isInteger(n) ? n.toLocaleString() : n.toFixed(digits));
</script>

{#snippet platinum()}
  <img class="plat-icon" src="/icons/platinum.png" alt="platinum" />
{/snippet}

{#snippet ducat()}
  <img class="plat-icon" src={imageUrl("Ducat.png")} alt="ducats" onerror={markMissing} />
{/snippet}

<div class="head">
  <div class="summary">
    {#if data?.account}
      <span><b>{summary.parts.toLocaleString()}</b> parts owned ({summary.copies.toLocaleString()} copies)</span>
      <span><b>{summary.ducats.toLocaleString()}</b> {@render ducat()}</span>
      <span>worth ~<b>{summary.plat.toLocaleString()}</b> {@render platinum()}</span>
    {:else if data}
      <span class="muted">No inventory captured yet, so nothing shows as owned.</span>
    {/if}
  </div>
  <div class="tools">
    <div class="chip-group" role="group" aria-label="Show">
      <button class:selected={view === "owned"} onclick={() => (view = "owned")}>Owned</button>
      <button class:selected={view === "all"} onclick={() => (view = "all")}>All</button>
    </div>
    <input type="search" placeholder="Search" bind:value={search} aria-label="Search" />
  </div>
</div>

{#snippet chip(on: boolean, title: string, onclick: () => void, label: string, note?: string)}
  <button class="chip" class:on aria-pressed={on} {title} {onclick}>
    <span class="tick" aria-hidden="true">{#if on}<Check size={14} strokeWidth={2.5} />{/if}</span>
    {label}{#if note}<span class="note">{note}</span>{/if}
  </button>
{/snippet}

<section class="filters" aria-label="Filters">
  <div class="group">
    <h2 class="filter-label">Show</h2>
    <div class="filter-row">
      <span class="row-label">Type</span>
      <div class="chip-group">
        {#each PART_TYPES as c (c.key)}
          {@render chip(!hide[c.key], c.title, () => toggleHide(c.key), c.label)}
        {/each}
      </div>
    </div>
    <div class="filter-row">
      <span class="row-label">Rarity</span>
      <div class="chip-group">
        {#each RARITIES as c (c.key)}
          {@render chip(!hide[c.key], c.title, () => toggleHide(c.key), c.label, c.ducats)}
        {/each}
      </div>
    </div>
  </div>

  <div class="group">
    <h2 class="filter-label">Keep for building <span class="hint">· left out of the list</span></h2>
    <label class="check" title="Parts of items you haven't mastered yet: keep them to build and level">
      <input type="checkbox" checked={hide.notMastered} onchange={() => toggleHide("notMastered")} />
      Parts for items I haven't mastered
    </label>
    <label class="check" title="Parts of sets you've already collected most of">
      <input type="checkbox" checked={hide.setShare !== null} onchange={toggleSetRule} />
      Parts of sets I've nearly finished
    </label>
    <div class="sub" class:off={hide.setShare === null}>
      <span class="inline-label">at least</span>
      <div class="segmented" role="radiogroup" aria-label="Set progress">
        {#each SET_SHARES as share}
          <button
            role="radio"
            aria-checked={hide.setShare === share}
            class:selected={hide.setShare === share}
            title={share === 100 ? "Sets you have every part of" : `Sets you have ${share}% or more of`}
            onclick={() => chooseSetShare(share)}
          >
            {share === 100 ? "Full" : `${share}%`}
          </button>
        {/each}
      </div>
    </div>
    <label
      class="check sub"
      class:off={!keepRules}
      title="Copies beyond what the set needs are safe to sell, so the rules above leave them listed"
    >
      <input type="checkbox" checked={hide.keepDupes} onchange={toggleKeepDupes} disabled={!keepRules} />
      …but still list spare copies <span class="muted">(beyond what the set needs)</span>
    </label>
  </div>

  <p class="hiding muted">
    Showing {(beforeFilters.length - hiddenCount).toLocaleString()} of {beforeFilters.length.toLocaleString()}
    {view === "owned" ? "owned parts" : "parts"}
    {#if filtersSet}· <button class="link" onclick={clearFilters}>Reset filters</button>{/if}
  </p>
</section>

<p class="source muted">
  Prices from
  <a href="https://warframe.market/tools/ducats" target="_blank" rel="noreferrer">warframe.market</a>, previous day
  {#if data?.updatedAt}· updated {time(data.updatedAt)}{/if}
  {#if data?.fetching}· updating…{/if}
  {#if data?.error}<span class="error">· couldn't update, showing the last saved prices. {friendlyError(data.error, "warframe.market")}</span>{/if}
</p>

{#if loadError}
  <p class="error">{loadError}</p>
{:else if !data || (data.fetching && !data.rows.length)}
  <p class="muted">Loading ducat prices…</p>
{:else if !data.rows.length}
  <p class="muted">No ducat prices yet.</p>
{:else}
  <div class="table-wrap">
    <table>
      <thead>
        <tr>
          <th></th>
          <th class="left">Item</th>
          {#each cols as c}
            <th class:opt={c.optional} aria-sort={sortKey === c.key ? (sortDesc ? "descending" : "ascending") : "none"}>
              <button class="col-sort" class:active={sortKey === c.key || (!sortKey && c.key === "perPlat")} onclick={() => sortBy(c.key)} title={c.title}>
                {c.label}<span class="arrow">{sortKey === c.key ? (sortDesc ? "▼" : "▲") : !sortKey && c.key === "perPlat" ? "▼" : ""}</span>
              </button>
            </th>
          {/each}
          {#if view === "owned"}<th>Total</th>{/if}
        </tr>
      </thead>
      <tbody>
        {#each rows as r (r.path)}
          <tr>
            <td class="icon">
              <span class="stack">
                {#if r.image}<img src={imageUrl(r.image)} alt="" loading="lazy" onerror={markMissing} />{:else}<div class="placeholder"></div>{/if}
                {#if r.overlay}<img class="overlay" src={imageUrl(r.overlay)} alt="" loading="lazy" onerror={hideOnError} />{/if}
              </span>
            </td>
            <td class="left name">
              {r.name}
              {#if r.volume < LOW_VOLUME}<span class="tag low" title="Only {r.volume} trade{r.volume === 1 ? '' : 's'} in the day">Low volume</span>{/if}
              {#if r.setNeeded > 0}
                <span class="set-progress" class:half={r.setOwned * 2 >= r.setNeeded} title="Parts of this set you have">
                  Set {r.setOwned}/{r.setNeeded}{#if r.spare > 0}<span class="dupe">&nbsp;· {r.spare} spare</span>{/if}
                </span>
              {/if}
            </td>
            {#if view === "owned"}<td class="num">{r.owned.toLocaleString()}</td>{/if}
            <td class="num ducats {r.tier}" title="{r.tier[0].toUpperCase()}{r.tier.slice(1)} tier">{r.ducats}</td>
            <td class="num">{fmt(r.waPrice)}</td>
            <td class="num opt">{fmt(r.median, 1)}</td>
            <td class="num ratio">{r.perPlat.toFixed(2)}</td>
            <td class="num opt muted">{r.perPlatMedian.toFixed(2)}</td>
            <td class="num opt">{r.volume.toLocaleString()}</td>
            <td class="num opt trend" class:up={r.trendDay > 0} class:down={r.trendDay < 0}>
              {r.trendDay > 0 ? `▲ ${r.trendDay}` : r.trendDay < 0 ? `▼ ${-r.trendDay}` : "–"}
            </td>
            {#if view === "owned"}<td class="num total">{(r.owned * r.ducats).toLocaleString()}</td>{/if}
          </tr>
        {:else}
          <tr><td colspan="12" class="muted empty">{view === "owned" ? "No owned prime parts match." : "Nothing matches."}</td></tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

<style>
  .head {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
  }

  .summary {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 22px;
    font-size: 19px;
  }

  .summary span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .tools,
  .chip-group {
    display: flex;
    gap: 6px;
    align-items: center;
  }

  .tools {
    gap: 12px;
  }

  button,
  input[type="search"] {
    font: inherit;
    color: var(--text);
    font-size: 15px;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 6px 12px;
  }

  .chip-group button {
    cursor: pointer;
    padding: 5px 12px;
    font-size: 15px;
    background: var(--surface-2);
  }

  .chip-group button:hover {
    background: var(--surface-hover);
    border-color: var(--line-strong);
  }

  .chip-group button.selected {
    background: var(--accent);
    border-color: var(--accent);
    color: white;
    font-weight: 600;
  }

  /* Filters: "Show" with one row of chips per category, "Keep for building" from the middle;
     they stack when the window is too narrow for both. */
  .filters {
    margin: 4px 0 10px;
    padding: 12px 16px;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    display: flex;
    flex-wrap: wrap;
    gap: 12px 48px;
  }

  .group {
    display: grid;
    align-content: start;
    gap: 8px;
  }

  /* Show fills the left half (never narrower than its chips), so Keep starts around the middle. */
  .group:first-child {
    flex: 0 0 calc(50% - 24px);
    min-width: max-content;
  }

  .group:first-child .chip-group {
    flex-wrap: nowrap;
  }

  .filter-label {
    margin: 0 0 2px;
    font-size: 13px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--muted);
  }

  .filter-label .hint {
    font-weight: 400;
    text-transform: none;
    letter-spacing: 0;
  }

  .filter-row {
    display: grid;
    grid-template-columns: 64px 1fr;
    align-items: center;
    gap: 10px;
  }

  .row-label {
    font-size: 14px;
    color: var(--muted);
  }

  .filters .chip-group {
    flex-wrap: wrap;
  }

  .inline-label {
    font-size: 14px;
    color: var(--muted);
  }

  /* Show chips: lit (tick, accent edge) when that group is listed, plain when it's left out. */
  .filters .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 5px 12px 5px 8px;
    color: var(--muted);
    transition:
      background-color 0.15s,
      border-color 0.15s,
      color 0.15s;
  }

  .filters .chip.on {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--text);
  }

  .tick {
    display: inline-grid;
    place-items: center;
    width: 14px;
    height: 14px;
  }

  .chip .note {
    font-size: 12px;
    color: var(--muted);
  }

  /* Keep rules: plain checkboxes; their options sit indented under them. */
  .check {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    width: fit-content;
    font-size: 15px;
    cursor: pointer;
  }

  .check input {
    width: 16px;
    height: 16px;
    margin: 0;
    accent-color: var(--accent);
    cursor: inherit;
  }

  .sub {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-left: 24px;
    font-size: 14px;
    transition: opacity 0.15s;
  }

  .sub.off {
    opacity: 0.45;
  }

  .check.sub.off {
    cursor: default;
  }

  .segmented {
    display: inline-flex;
  }

  .segmented button {
    padding: 3px 11px;
    font-size: 14px;
    background: var(--surface-2);
    border-radius: 0;
    cursor: pointer;
  }

  .segmented button + button {
    margin-left: -1px;
  }

  .segmented button:first-child {
    border-radius: 6px 0 0 6px;
  }

  .segmented button:last-child {
    border-radius: 0 6px 6px 0;
  }

  .segmented button:hover {
    background: var(--surface-hover);
  }

  .segmented button.selected {
    position: relative;
    background: var(--accent);
    border-color: var(--accent);
    color: white;
    font-weight: 600;
  }

  .hiding {
    flex-basis: 100%;
    margin: 0;
    font-size: 13px;
  }

  .link {
    all: unset;
    color: var(--accent);
    cursor: pointer;
  }

  .link:hover {
    text-decoration: underline;
  }

  .set-progress .dupe {
    color: var(--gold);
  }

  /* Tier colours, as relic reward rarities in Inventory. */
  .gold {
    --tier: var(--gold);
  }

  .silver {
    --tier: #c3ccd8;
  }

  .bronze {
    --tier: #c98b5a;
  }

  .ducats.gold,
  .ducats.silver,
  .ducats.bronze {
    color: var(--tier);
    font-weight: 600;
  }

  .set-progress {
    display: block;
    margin-top: 2px;
    font-size: 12px;
    font-weight: 400;
    color: var(--muted);
  }

  .set-progress.half {
    color: var(--good);
  }

  .source {
    margin: 0 0 14px;
    font-size: 14px;
  }

  .source a {
    color: var(--accent);
  }

  .muted {
    color: var(--muted);
  }

  .error {
    color: var(--bad);
  }

  .plat-icon {
    width: 1.1em;
    height: 1.1em;
    object-fit: contain;
  }

  /* Less important columns hide first on a narrow window. */
  .table-wrap {
    container-type: inline-size;
  }

  @container (max-width: 760px) {
    .opt {
      display: none;
    }
  }

  table {
    width: 100%;
    border-collapse: collapse;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    overflow: hidden;
    font-size: 15px;
  }

  th {
    font-size: 13px;
    font-weight: 600;
    color: var(--muted);
    text-align: right;
    padding: 12px 14px;
    border-bottom: 1px solid var(--line);
    white-space: nowrap;
  }

  td {
    padding: 8px 14px;
    border-bottom: 1px solid var(--line);
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  tbody tr:hover {
    background: var(--surface-2);
  }

  .left {
    text-align: left;
  }

  .col-sort {
    all: unset;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .col-sort:hover,
  .col-sort.active {
    color: var(--text);
  }

  .col-sort:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
    border-radius: 3px;
  }

  .col-sort .arrow {
    min-width: 0.8em;
    font-size: 10px;
    color: var(--accent);
  }

  .icon {
    width: 56px;
    padding-right: 0;
  }

  .stack {
    position: relative;
    display: block;
    width: 40px;
    height: 40px;
  }

  .stack img,
  .stack .placeholder {
    width: 100%;
    height: 100%;
    object-fit: contain;
    display: block;
  }

  /* The built item over a blueprint, as in Inventory. */
  .stack img.overlay {
    position: absolute;
    inset: 18%;
    width: 64%;
    height: 64%;
  }

  .name {
    font-weight: 600;
  }

  .tag {
    margin-left: 8px;
    font-size: 11px;
    font-weight: 600;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--surface-2);
    color: var(--muted);
  }

  .tag.low {
    color: var(--gold);
  }

  .ratio {
    font-weight: 700;
    color: var(--accent);
  }

  .trend.up {
    color: var(--good);
  }

  .trend.down {
    color: var(--bad);
  }

  .total {
    font-weight: 600;
  }

  .empty {
    text-align: center;
    padding: 24px;
  }
</style>
