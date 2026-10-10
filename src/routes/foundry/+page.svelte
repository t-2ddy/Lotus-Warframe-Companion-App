<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { compact, hideOnError, imageUrl, markMissing } from "$lib/format";

  type Tab = "warframe" | "primary" | "secondary" | "melee" | "modular" | "arch" | "companion";

  type Part = {
    path: string;
    name: string;
    image: string | null;
    /** For blueprints: what it builds, drawn over the blueprint picture. */
    overlay: string | null;
    needed: number;
    owned: number;
  };

  type Item = {
    path: string;
    name: string;
    tab: Tab;
    image: string | null;
    isPrime: boolean;
    vaulted: boolean;
    masteryReq: number;
    maxRank: number;
    xp: number;
    rank: number;
    mastered: boolean;
    owned: number;
    subsumed: boolean;
    archonShards: number;
    incarnon: boolean;
    usedForCrafting: boolean;
    favorite: boolean;
    readyToBuild: boolean;
    parts: Part[];
  };

  type FoundryData = { account: string | null; masteryRank: number | null; items: Item[] };

  const TABS: { id: Tab | "all"; label: string }[] = [
    { id: "all", label: "All" },
    { id: "warframe", label: "Warframe" },
    { id: "primary", label: "Primary" },
    { id: "secondary", label: "Secondary" },
    { id: "melee", label: "Melee" },
    { id: "modular", label: "Modular" },
    { id: "arch", label: "Arch" },
    { id: "companion", label: "Companion" },
  ];

  type Choice = "yes" | "no";
  type Filter = {
    key: string;
    label: string;
    yes?: string;
    no?: string;
    /** Set when the data for this filter isn't available yet. */
    unavailable?: string;
    test: (i: Item) => boolean;
  };

  let data = $state<FoundryData | null>(null);
  let tab = $state<Tab | "all">("all");
  let search = $state("");
  let filtersOpen = $state(false);
  let chosen = $state<Record<string, Choice | undefined>>({});

  const mr = $derived(data?.masteryRank ?? null);

  const FILTERS: Filter[] = [
    { key: "type", label: "Type", yes: "Prime", no: "Normal", test: (i) => i.isPrime },
    { key: "mastery", label: "Mastery", yes: "Mastered", no: "Unmastered", test: (i) => i.mastered },
    { key: "owned", label: "Owned", test: (i) => i.owned > 0 },
    { key: "vaulted", label: "Vaulted", test: (i) => i.vaulted },
    { key: "ready", label: "Ready to build", test: (i) => i.readyToBuild },
    { key: "enough", label: "Enough mastery", test: (i) => mr === null || i.masteryReq <= mr },
    {
      key: "resurgence",
      label: "Prime resurgence",
      unavailable: "Needs live game-world data. Coming in a later milestone.",
      test: () => false,
    },
    { key: "crafting", label: "Used for crafting", test: (i) => i.usedForCrafting },
    { key: "favorite", label: "Favorite", test: (i) => i.favorite },
    { key: "helminth", label: "Helminth done", test: (i) => i.subsumed },
    { key: "shards", label: "Contains archon shards", test: (i) => i.archonShards > 0 },
    { key: "incarnon", label: "Incarnon genesis", test: (i) => i.incarnon },
  ];

  const load = () =>
    invoke<FoundryData>("get_foundry")
      .then((d) => (data = d))
      .catch(() => {});

  onMount(() => {
    load();
    const off = listen("inventory-updated", load);
    return () => {
      off.then((f) => f());
    };
  });

  /** Everything except the tab: search text and filter choices. */
  function passes(i: Item) {
    if (search && !i.name.toLowerCase().includes(search.trim().toLowerCase())) return false;
    for (const f of FILTERS) {
      const c = chosen[f.key];
      if (c && f.test(i) !== (c === "yes")) return false;
    }
    return true;
  }

  const matching = $derived((data?.items ?? []).filter(passes));
  const visible = $derived(tab === "all" ? matching : matching.filter((i) => i.tab === tab));
  const tabCounts = $derived.by(() => {
    const counts: Record<string, number> = { all: matching.length };
    for (const i of matching) counts[i.tab] = (counts[i.tab] ?? 0) + 1;
    return counts;
  });
  const activeFilters = $derived(Object.values(chosen).filter(Boolean).length);
  const masteredHere = $derived(visible.filter((i) => i.mastered).length);

  function choose(key: string, c: Choice) {
    chosen[key] = chosen[key] === c ? undefined : c;
  }

  async function toggleFavorite(item: Item) {
    const on = !item.favorite;
    item.favorite = on;
    try {
      await invoke("set_favorite", { item: item.path, on });
    } catch {
      item.favorite = !on;
    }
  }

  function closeFiltersOnOutside(e: MouseEvent) {
    if (filtersOpen && !(e.target as HTMLElement).closest(".filters")) filtersOpen = false;
  }
</script>

<svelte:window onclick={closeFiltersOnOutside} />

<div class="toolbar">
  <div class="tabs" role="tablist">
    {#each TABS as t}
      <button role="tab" aria-selected={tab === t.id} class:active={tab === t.id} onclick={() => (tab = t.id)}>
        {t.label}<span class="count">{tabCounts[t.id] ?? 0}</span>
      </button>
    {/each}
  </div>
  <div class="tools">
    <input type="search" placeholder="Search items" bind:value={search} aria-label="Search items" />
    <div class="filters">
      <button class="filter-btn" class:on={activeFilters > 0} onclick={() => (filtersOpen = !filtersOpen)}>
        Filters{activeFilters ? ` (${activeFilters})` : ""}
      </button>
      {#if filtersOpen}
        <div class="filter-panel" role="dialog" aria-label="Filters">
          {#each FILTERS as f}
            <div class="group" class:unavailable={f.unavailable} title={f.unavailable ?? ""}>
              <div class="group-label">{f.label}</div>
              <div class="choices">
                {#each ["yes", "no"] as const as c}
                  <button
                    class:selected={chosen[f.key] === c}
                    disabled={!!f.unavailable}
                    onclick={() => choose(f.key, c)}
                  >
                    {c === "yes" ? (f.yes ?? "Yes") : (f.no ?? "No")}
                  </button>
                {/each}
              </div>
            </div>
          {/each}
          <button class="clear" onclick={() => (chosen = {})} disabled={!activeFilters}>Clear filters</button>
        </div>
      {/if}
    </div>
  </div>
</div>

{#if !data}
  <p class="empty">Loading…</p>
{:else if !data.items.length}
  <p class="empty">Loading item data. This takes a moment the first time the app runs.</p>
{:else}
  <p class="summary">
    {visible.length.toLocaleString()} items · {masteredHere.toLocaleString()} mastered
    {#if mr !== null}· Mastery rank {mr}{/if}
    {#if !data.account}· No inventory captured yet, so nothing shows as owned or mastered{/if}
  </p>

  <div class="grid">
    {#each visible as i (i.path)}
      <article class="item" class:owned={i.owned > 0}>
        <header>
          <button
            class="star"
            class:on={i.favorite}
            onclick={() => toggleFavorite(i)}
            aria-label={i.favorite ? `Unstar ${i.name}` : `Star ${i.name}`}
            aria-pressed={i.favorite}
          >
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="M12 2.5l2.9 6.1 6.6.8-4.9 4.6 1.3 6.6L12 17.3l-5.9 3.3 1.3-6.6-4.9-4.6 6.6-.8z" />
            </svg>
          </button>
          <h3 title={i.name}>{i.name}</h3>
          {#if i.mastered}
            <img class="mastered-icon" src="/icons/mastered.png" alt="Mastered" title="Mastered" />
          {/if}
        </header>

        <div class="body">
          <div class="portrait">
            {#if i.image}
              <img src={imageUrl(i.image)} alt="" loading="lazy" onerror={markMissing} />
            {:else}
              <div class="placeholder"></div>
            {/if}
          </div>
          {#if i.parts.length}
            <ul class="parts">
              {#each i.parts as p}
                <li class:enough={p.owned >= p.needed} title="{p.name}: {p.owned.toLocaleString()} owned, {p.needed} needed">
                  {#if p.image}
                    <img src={imageUrl(p.image)} alt={p.name} loading="lazy" onerror={markMissing} />
                  {:else}
                    <div class="placeholder"></div>
                  {/if}
                  {#if p.overlay}
                    <img class="overlay" src={imageUrl(p.overlay)} alt="" loading="lazy" onerror={hideOnError} />
                  {/if}
                  {#if p.owned > 0}<span class="badge">{compact(p.owned)}</span>{/if}
                </li>
              {/each}
            </ul>
          {/if}
        </div>

        <footer>
          {#if i.mastered}<span class="tag mastered">Mastered</span>{/if}
          {#if i.vaulted}<span class="tag">Vaulted</span>{/if}
          {#if !i.mastered && i.xp > 0}<span class="tag">Rank {i.rank}/{i.maxRank}</span>{/if}
          {#if i.mastered && i.maxRank > 30}<span class="tag">Rank {i.maxRank}</span>{/if}
          {#if i.incarnon}<span class="tag">Incarnon</span>{/if}
          {#if i.archonShards > 0}<span class="tag">{i.archonShards} shard{i.archonShards === 1 ? "" : "s"}</span>{/if}
          {#if mr !== null && i.masteryReq > mr}<span class="tag bad">MR {i.masteryReq}</span>{/if}
          {#if i.subsumed}<span class="tag helminth">Subsumed</span>{/if}
          {#if i.owned > 0}<span class="tag good">Owned{i.owned > 1 ? ` ×${i.owned}` : ""}</span>{/if}
        </footer>
      </article>
    {:else}
      <p class="empty">No items match.</p>
    {/each}
  </div>
{/if}

<style>
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 12px;
  }

  .tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .tabs button,
  .filter-btn,
  .choices button,
  .clear {
    font: inherit;
    color: var(--text);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 6px 12px;
    cursor: pointer;
  }

  .tabs button:hover,
  .filter-btn:hover,
  .choices button:hover:not(:disabled),
  .clear:hover:not(:disabled) {
    background: var(--surface-hover);
    border-color: var(--line-strong);
  }

  .tabs button.active {
    background: var(--accent);
    border-color: var(--accent);
    color: white;
    font-weight: 600;
  }

  .count {
    margin-left: 6px;
    font-size: 12px;
    opacity: 0.7;
  }

  .tools {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  input[type="search"] {
    font: inherit;
    color: var(--text);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 6px 12px;
    width: 240px;
    outline: none;
  }

  input[type="search"]::placeholder {
    color: var(--muted);
  }

  input[type="search"]:focus {
    border-color: var(--accent);
  }

  .filters {
    position: relative;
  }

  .filter-btn.on {
    border-color: var(--accent);
    color: #93b8f8;
    font-weight: 600;
  }

  .filter-panel {
    position: absolute;
    right: 0;
    top: calc(100% + 8px);
    width: 290px;
    max-height: 70vh;
    overflow-y: auto;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    padding: 14px;
    z-index: 5;
    box-shadow: var(--shadow);
  }

  .group {
    margin-bottom: 10px;
  }

  .group-label {
    font-weight: 600;
    text-align: center;
    margin-bottom: 4px;
  }

  .choices {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
  }

  .choices button.selected {
    background: var(--accent);
    border-color: var(--accent);
    color: white;
    font-weight: 600;
  }

  .group.unavailable {
    opacity: 0.45;
  }

  .choices button:disabled,
  .clear:disabled {
    cursor: default;
  }

  .clear {
    width: 100%;
    margin-top: 4px;
  }

  .summary,
  .empty {
    color: var(--muted);
    margin: 0 0 12px;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: 14px;
  }

  .item {
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .item:hover {
    border-color: var(--line-strong);
  }

  /* Owned: a green left edge, so owned items are easy to pick out. */
  .item.owned {
    box-shadow: inset 3px 0 0 var(--good);
  }

  header {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  /* The card title size. The name and the mastered icon are sized from it in em, so they scale together. */
  header {
    font-size: 22px;
  }

  h3 {
    font-size: 1em;
    font-weight: 600;
    margin: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .star {
    flex: none;
    width: 24px;
    height: 24px;
    padding: 2px;
    border: none;
    background: none;
    cursor: pointer;
  }

  .star svg {
    width: 100%;
    height: 100%;
    fill: none;
    stroke: var(--muted);
    stroke-width: 1.6;
  }

  .star.on svg {
    fill: var(--gold);
    stroke: var(--gold);
  }

  .mastered-icon {
    flex: none;
    width: 1.5em;
    height: 1.5em;
    object-fit: contain;
    background: none;
    border: none;
    border-radius: 0;
    box-shadow: none;
  }

  .body {
    display: flex;
    gap: 10px;
    align-items: center;
  }

  .portrait {
    flex: none;
    width: 104px;
    height: 104px;
  }

  .portrait {
    border-radius: 8px;
    background: var(--surface-2);
  }

  .portrait img,
  .portrait .placeholder {
    width: 100%;
    height: 100%;
    object-fit: contain;
    border-radius: 8px;
  }

  .parts {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .parts li {
    position: relative;
    width: 38px;
    height: 38px;
    border-radius: 50%;
    background: var(--surface-2);
    border: 2px solid var(--line-strong);
    display: grid;
    place-items: center;
  }

  .parts li.enough {
    border-color: var(--good);
  }

  .parts img,
  .parts .placeholder {
    width: 30px;
    height: 30px;
    object-fit: contain;
    border-radius: 50%;
  }

  /* The built item drawn over a blueprint, centred in the circle. */
  .parts img.overlay {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 24px;
    height: 24px;
    transform: translate(-50%, -50%);
    border-radius: 0;
    background: none;
  }

  .badge {
    position: absolute;
    right: -6px;
    bottom: -6px;
    min-width: 18px;
    padding: 0 5px;
    border-radius: 9px;
    background: #070a10;
    border: 1px solid var(--line-strong);
    color: var(--text);
    font-size: 10px;
    font-weight: 700;
    line-height: 15px;
    text-align: center;
  }

  .placeholder,
  img:global(.missing) {
    background: var(--line);
    color: transparent;
  }

  footer {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: auto;
  }

  .tag {
    font-size: 12px;
    padding: 1px 8px;
    border-radius: 4px;
    background: var(--surface-2);
    color: var(--muted);
  }

  .tag.good {
    color: var(--good);
    font-weight: 600;
  }

  /* Teal to match the mastered wreath icon. */
  .tag.mastered {
    color: #6fb8a8;
    font-weight: 600;
  }

  .tag.bad {
    color: var(--bad);
  }

  .tag.helminth {
    color: var(--helminth);
    font-weight: 600;
  }
</style>
