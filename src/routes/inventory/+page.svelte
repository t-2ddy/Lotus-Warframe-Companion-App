<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { compact, hideOnError, imageUrl, markMissing } from "$lib/format";

  type Tile = {
    path: string;
    name: string;
    image: string | null;
    /** For blueprints: what it builds, drawn over the blueprint picture. */
    overlay: string | null;
    underlay?: string | null;
    count: number;
    kind: string;
    prime: boolean;
    builds: "warframe" | "weapon" | "companion" | "other";
  };

  type ModTile = {
    path: string;
    name: string;
    image: string | null;
    count: number;
    ranks: { rank: number; count: number }[];
    kind: string;
    rarity: string | null;
    polarity: string | null;
    drain: number | null;
    maxRank: number | null;
    compat: string | null;
    /** Wiki card image (image-cache name, e.g. wiki__SerrationMod.png), shown at max rank. */
    wikiImage: string | null;
    /** Highest owned rank and its stat lines (used by the app-drawn fallback card). */
    topRank: number;
    stats: string[];
  };

  /** Mods whose wiki card failed to load; they show the app-drawn card instead. */
  let wikiFailed = $state<Record<string, boolean>>({});

  // --- Mods tab: Prices view (warframe.market) ---

  type PriceView = {
    path: string;
    rank: number;
    wts: number | null;
    wtb: number | null;
    status: "fresh" | "stale" | "loading" | "untradable";
    fetchedAt: number | null;
  };

  let modView = $state<"prices" | "cards">("prices");
  /** Prices by "<path>#<rank>". */
  let prices = $state<Record<string, PriceView>>({});

  const priceKey = (path: string, rank: number) => `${path}#${rank}`;

  // Rows ask for their price when they scroll into view, batched every 120 ms. Each row asks
  // once per page load; the backend caches prices for 2 weeks and queues the rest.
  const requested = new Set<string>();
  const pending = new Set<string>();
  let flushTimer: ReturnType<typeof setTimeout> | undefined;
  let observer: IntersectionObserver | undefined;

  async function flushPrices() {
    flushTimer = undefined;
    const keys = [...pending].map((k) => {
      const i = k.lastIndexOf("#");
      return { path: k.slice(0, i), rank: Number(k.slice(i + 1)) };
    });
    pending.clear();
    if (!keys.length) return;
    try {
      const result = await invoke<PriceView[]>("request_prices", { keys });
      for (const p of result) prices[priceKey(p.path, p.rank)] = p;
    } catch {
      // Leave those rows showing "…"; they'll be asked again after a reload.
    }
  }

  /** Svelte action: request this row's price once it's (nearly) on screen. */
  function watchPrice(node: HTMLElement, key: string) {
    observer ??= new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          const k = (e.target as HTMLElement).dataset.priceKey;
          if (e.isIntersecting && k && !requested.has(k)) {
            requested.add(k);
            pending.add(k);
          }
        }
        if (pending.size && !flushTimer) flushTimer = setTimeout(flushPrices, 120);
      },
      { rootMargin: "200px" },
    );
    node.dataset.priceKey = key;
    observer.observe(node);
    return {
      update(k: string) {
        node.dataset.priceKey = k;
      },
      destroy() {
        observer?.unobserve(node);
      },
    };
  }

  function priceText(p: PriceView | undefined, side: "wts" | "wtb") {
    if (!p || (p.status === "loading" && p[side] === null)) return "…";
    const v = p[side];
    return v === null ? "–" : v.toLocaleString();
  }

  type Reward = {
    path: string;
    name: string;
    image: string | null;
    overlay: string | null;
    underlay?: string | null;
    rarity: string;
    owned: number;
  };

  type Relic = {
    name: string;
    era: string;
    image: string | null;
    vaulted: boolean;
    counts: [number, number, number, number];
    total: number;
    rewards: Reward[];
  };

  /** One part of a prime set as it's traded (a warframe part's blueprint, a weapon part). */
  type SetPart = {
    path: string;
    name: string;
    image: string | null;
    overlay: string | null;
    needed: number;
    /** Tradable copies held. */
    owned: number;
  };

  type PrimeSet = {
    /** The built item, which is also the set on warframe.market. */
    path: string;
    name: string;
    /** The main blueprint's picture, with the item drawn over it. */
    image: string | null;
    overlay: string | null;
    vaulted: boolean;
    /** Complete sets held. */
    count: number;
    parts: SetPart[];
  };

  type InventoryData = { parts: Tile[]; sets: PrimeSet[]; relics: Relic[]; mods: ModTile[]; arcanes: ModTile[]; misc: Tile[] };

  type TabId = "parts" | "sets" | "relics" | "mods" | "arcanes" | "misc";
  const TABS: { id: TabId; label: string }[] = [
    { id: "parts", label: "Parts" },
    { id: "sets", label: "Prime Sets" },
    { id: "relics", label: "Relics" },
    { id: "mods", label: "Mods" },
    { id: "arcanes", label: "Arcanes" },
    { id: "misc", label: "Misc" },
  ];
  const REFINEMENTS = ["Intact", "Exceptional", "Flawless", "Radiant"];
  const ERAS = ["Lith", "Meso", "Neo", "Axi", "Requiem"];
  const RARITIES = ["Common", "Uncommon", "Rare", "Legendary"];

  let data = $state<InventoryData | null>(null);
  let tab = $state<TabId>("parts");
  let search = $state("");
  let sort = $state<"name" | "amount">("name");

  // Per-tab filters. "" means no filter.
  let partKind = $state("");
  let partPrime = $state("");
  let partBuilds = $state("");
  let setVaulted = $state("");
  let relicEra = $state("");
  let relicVaulted = $state("");
  let modType = $state("");
  let modRarity = $state("");
  let arcaneType = $state("");
  let arcaneRarity = $state("");
  let miscType = $state("");

  let expanded = $state<Record<string, boolean>>({});

  const load = () =>
    invoke<InventoryData>("get_inventory")
      .then((d) => (data = d))
      .catch(() => {});

  onMount(() => {
    load();
    const off = listen("inventory-updated", load);
    const offPrice = listen<PriceView>("market-price", (e) => {
      prices[priceKey(e.payload.path, e.payload.rank)] = e.payload;
    });
    return () => {
      off.then((f) => f());
      offPrice.then((f) => f());
      observer?.disconnect();
    };
  });

  const matchesSearch = (name: string) => !search || name.toLowerCase().includes(search.trim().toLowerCase());

  function sorted<T extends { count?: number; total?: number }>(list: T[]): T[] {
    if (sort === "name") return list; // already in name order from the backend
    return [...list].sort((a, b) => (b.count ?? b.total ?? 0) - (a.count ?? a.total ?? 0));
  }

  const parts = $derived(
    sorted(
      (data?.parts ?? []).filter(
        (p) =>
          matchesSearch(p.name) &&
          (!partKind || p.kind === partKind) &&
          (!partPrime || p.prime === (partPrime === "Prime")) &&
          (!partBuilds || p.builds === partBuilds),
      ),
    ),
  );
  /** A set's lowest sell price, or null while it's loading or not listed. */
  const setPrice = (s: PrimeSet) => prices[priceKey(s.path, 0)]?.wts ?? null;

  const sets = $derived.by(() => {
    const list = (data?.sets ?? []).filter(
      (s) =>
        (matchesSearch(s.name) || s.parts.some((p) => matchesSearch(p.name))) &&
        (!setVaulted || s.vaulted === (setVaulted === "Vaulted")),
    );
    const col = setCol;
    if (col === null) return sorted(list);
    const dir = setDesc ? -1 : 1;
    const value = (s: PrimeSet) => (col === "count" ? s.count : setPrice(s));
    // Sets without a price yet (or not listed) go last either way, by name.
    return [...list].sort((a, b) => {
      const [va, vb] = [value(a), value(b)];
      if (va === null || vb === null) return (va === null ? 1 : 0) - (vb === null ? 1 : 0) || a.name.localeCompare(b.name);
      return dir * (va - vb) || a.name.localeCompare(b.name);
    });
  });

  // Sorting by price needs every set's price, not just the rows on screen: ask for them all
  // (cached ones answer at once; the rest queue at warframe.market's polite pace).
  $effect(() => {
    if (tab !== "sets" || setCol !== "price" || !data) return;
    for (const s of data.sets) {
      const k = priceKey(s.path, 0);
      if (!requested.has(k)) {
        requested.add(k);
        pending.add(k);
      }
    }
    if (pending.size && !flushTimer) flushTimer = setTimeout(flushPrices, 0);
  });
  /** Sets tab: a clicked column (Sets owned or Set price) and its direction, as on Relics. */
  let setCol = $state<"count" | "price" | null>(null);
  let setDesc = $state(true);

  /** Click a column: highest first; again: lowest first; a third time: back to the Sort menu's order. */
  function sortSets(col: "count" | "price") {
    if (setCol !== col) [setCol, setDesc] = [col, true];
    else if (setDesc) setDesc = false;
    else setCol = null;
  }

  /** Sets tab: expanded rows, by set path (relics use their name, which can't clash). */
  let openSet = $state<Record<string, boolean>>({});

  /** Relics tab: a clicked count column (0–3 = Intact…Radiant, 4 = Total) and its direction. */
  let relicCol = $state<number | null>(null);
  let relicDesc = $state(true);
  const relicValue = (r: Relic, col: number) => (col === 4 ? r.total : r.counts[col]);

  /** Click a column: most first; again: fewest first; a third time: back to the Sort menu's order. */
  function sortRelics(col: number) {
    if (relicCol !== col) [relicCol, relicDesc] = [col, true];
    else if (relicDesc) relicDesc = false;
    else relicCol = null;
  }

  const relics = $derived.by(() => {
    const list = (data?.relics ?? []).filter(
      (r) =>
        (matchesSearch(r.name) || r.rewards.some((w) => matchesSearch(w.name))) &&
        (!relicEra || r.era === relicEra) &&
        (!relicVaulted || r.vaulted === (relicVaulted === "Vaulted")),
    );
    const col = relicCol;
    if (col === null) return sorted(list);
    const dir = relicDesc ? -1 : 1;
    return [...list].sort((a, b) => dir * (relicValue(a, col) - relicValue(b, col)) || a.name.localeCompare(b.name));
  });
  const mods = $derived(
    sorted(
      (data?.mods ?? []).filter(
        (m) => matchesSearch(m.name) && (!modType || m.kind === modType) && (!modRarity || m.rarity === modRarity),
      ),
    ),
  );
  /** Mods tab, Prices view: one row per owned rank of each mod, like warframe.market lists them. */
  const modRows = $derived(
    mods.flatMap((m) => m.ranks.map((r) => ({ m, rank: r.rank, count: r.count, key: priceKey(m.path, r.rank) }))),
  );

  const arcanes = $derived(
    sorted(
      (data?.arcanes ?? []).filter(
        (m) =>
          matchesSearch(m.name) && (!arcaneType || m.kind === arcaneType) && (!arcaneRarity || m.rarity === arcaneRarity),
      ),
    ),
  );
  const misc = $derived(
    sorted((data?.misc ?? []).filter((m) => matchesSearch(m.name) && (!miscType || m.kind === miscType))),
  );

  const counts = $derived({ parts: parts.length, sets: sets.length, relics: relics.length, mods: mods.length, arcanes: arcanes.length, misc: misc.length });

  const kinds = (list: { kind: string }[] = []) => [...new Set(list.map((i) => i.kind))].sort();
  const modTypes = $derived(kinds(data?.mods));
  const arcaneTypes = $derived(kinds(data?.arcanes));
  const miscTypes = $derived(kinds(data?.misc));

  const ranksText = (m: ModTile) => m.ranks.map((r) => `R${r.rank} ×${r.count}`).join(" · ");
  const capitalize = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
</script>

{#snippet chips(label: string, options: string[], value: string, set: (v: string) => void, labels?: Record<string, string>)}
  <div class="filter-group" role="group" aria-label={label}>
    <span class="filter-label">{label}</span>
    <div class="chip-group">
      <button class:selected={value === ""} onclick={() => set("")}>All</button>
      {#each options as o}
        <button class:selected={value === o} onclick={() => set(value === o ? "" : o)}>{labels?.[o] ?? o}</button>
      {/each}
    </div>
  </div>
{/snippet}

{#snippet select(label: string, options: string[], value: string, set: (v: string) => void)}
  <label class="filter-group">
    <span class="filter-label">{label}</span>
    <select {value} onchange={(e) => set((e.currentTarget as HTMLSelectElement).value)}>
      <option value="">All</option>
      {#each options as o}<option value={o}>{o}</option>{/each}
    </select>
  </label>
{/snippet}

{#snippet modViewSwitch()}
  <div class="view-switch filter-group" role="group" aria-label="Mods view">
    <span class="filter-label">View</span>
    <div class="chip-group">
      <button class:selected={modView === "prices"} onclick={() => (modView = "prices")}>Prices</button>
      <button class:selected={modView === "cards"} onclick={() => (modView = "cards")}>Cards</button>
    </div>
    <span class="muted source">Rank 0 prices from warframe.market · refreshed every 2 weeks</span>
  </div>
{/snippet}

{#snippet platinum()}
  <img class="plat-icon" src="/icons/platinum.png" alt="platinum" />
{/snippet}

{#snippet modCard(m: ModTile)}
  <div class="mod-card {(m.rarity ?? 'common').toLowerCase()}">
    <div class="mod-top">
      {#if m.drain !== null}<span class="mod-drain">{m.drain + m.topRank}</span>{/if}
      {#if m.polarity}<span class="mod-polarity">{capitalize(m.polarity)}</span>{/if}
      <span class="mod-count">×{m.count}</span>
    </div>
    <div class="mod-art">{@render image(m.image)}</div>
    <div class="mod-name">{m.name}</div>
    <ul class="mod-stats">
      {#each m.stats as line}<li>{line}</li>{:else}<li class="muted">No stats listed</li>{/each}
    </ul>
    {#if m.compat}<div class="mod-compat">{m.compat}</div>{/if}
    {#if m.maxRank !== null}
      <div class="mod-pips" title="Rank {m.topRank} of {m.maxRank}">
        {#each Array(m.maxRank) as _, r}<span class:on={r < m.topRank}></span>{/each}
      </div>
    {/if}
  </div>
{/snippet}

{#snippet image(src: string | null, alt = "", overlay: string | null = null, underlay: string | null = null)}
  <span class="stack">
    {#if underlay}
      <img class="underlay" src={imageUrl(underlay)} alt="" loading="lazy" onerror={hideOnError} />
    {/if}
    {#if src}
      <img src={imageUrl(src)} {alt} loading="lazy" onerror={markMissing} />
    {:else}
      <div class="placeholder"></div>
    {/if}
    {#if overlay}
      <img class="overlay" src={imageUrl(overlay)} alt="" loading="lazy" onerror={hideOnError} />
    {/if}
  </span>
{/snippet}

<div class="toolbar">
  <div class="tabs" role="tablist">
    {#each TABS as t}
      <button role="tab" aria-selected={tab === t.id} class:active={tab === t.id} onclick={() => (tab = t.id)}>
        {t.label}<span class="count">{counts[t.id]}</span>
      </button>
    {/each}
  </div>
  <div class="tools">
    <input type="search" placeholder="Search" bind:value={search} aria-label="Search" />
    <select bind:value={sort} aria-label="Sort" onchange={() => ([relicCol, setCol] = [null, null])}>
      <option value="name">Sort: Name</option>
      <option value="amount">Sort: Amount</option>
    </select>
  </div>
</div>

<div class="filters">
  {#if tab === "parts"}
    {@render chips("Kind", ["Blueprint", "Part"], partKind, (v) => (partKind = v), { Blueprint: "Blueprints", Part: "Parts" })}
    {@render chips("Prime", ["Prime", "Normal"], partPrime, (v) => (partPrime = v))}
    {@render chips("Builds", ["warframe", "weapon", "companion", "other"], partBuilds, (v) => (partBuilds = v), {
      warframe: "Warframe",
      weapon: "Weapon",
      companion: "Companion",
      other: "Other",
    })}
  {:else if tab === "sets"}
    {@render chips("Vaulted", ["Vaulted", "Unvaulted"], setVaulted, (v) => (setVaulted = v))}
    <span class="muted source">Prices from warframe.market · refreshed every 2 weeks</span>
  {:else if tab === "relics"}
    {@render chips("Era", ERAS, relicEra, (v) => (relicEra = v))}
    {@render chips("Vaulted", ["Vaulted", "Unvaulted"], relicVaulted, (v) => (relicVaulted = v))}
  {:else if tab === "mods"}
    {@render select("Mod type", modTypes, modType, (v) => (modType = v))}
    {@render chips("Rarity", RARITIES, modRarity, (v) => (modRarity = v))}
  {:else if tab === "arcanes"}
    {@render select("Arcane type", arcaneTypes, arcaneType, (v) => (arcaneType = v))}
    {@render chips("Rarity", RARITIES, arcaneRarity, (v) => (arcaneRarity = v))}
  {:else}
    {@render select("Type", miscTypes, miscType, (v) => (miscType = v))}
  {/if}
</div>

{#snippet sortIcon(active: boolean, desc: boolean)}
  <!-- Both arrows always show (faint), so the column reads as sortable; the active direction lights up. -->
  <svg class="sort-icon" viewBox="0 0 8 12" aria-hidden="true">
    <path class:on={active && !desc} d="M4 0.5 7.5 4.5H0.5z" />
    <path class:on={active && desc} d="M4 11.5 0.5 7.5H7.5z" />
  </svg>
{/snippet}

{#snippet pricePair(p: PriceView | undefined, title: string)}
  {#if p?.status === "untradable"}
    <span class="muted">Not listed</span>
  {:else}
    <span class="set-prices" class:stale={p?.status === "stale"} {title}>
      <span class="set-price wts"><span class="side">WTS</span>{priceText(p, "wts")}{@render platinum()}</span>
      <span class="set-price wtb"><span class="side">WTB</span>{priceText(p, "wtb")}{@render platinum()}</span>
    </span>
  {/if}
{/snippet}

{#if !data}
  <p class="empty">Loading…</p>
{:else if tab === "sets"}
  <table class="relics sets">
    <thead>
      <tr>
        <th></th>
        <th class="left">Set</th>
        {#each [["count", "Sets owned"], ["price", "Set price"]] as [col, label]}
          <th class="mid" aria-sort={setCol === col ? (setDesc ? "descending" : "ascending") : "none"}>
            <button class="col-sort" class:active={setCol === col} onclick={() => sortSets(col as "count" | "price")} title="Sort by {label.toLowerCase()}">
              {label}{@render sortIcon(setCol === col, setDesc)}
            </button>
          </th>
        {/each}
        <th></th>
        <th></th>
      </tr>
    </thead>
    <tbody>
      {#each sets as s (s.path)}
        {@const p = prices[priceKey(s.path, 0)]}
        <tr
          class="relic-row"
          class:open={openSet[s.path]}
          onclick={() => (openSet[s.path] = !openSet[s.path])}
          use:watchPrice={priceKey(s.path, 0)}
        >
          <td class="set-icon-cell"><span class="set-icon">{@render image(s.image, "", s.overlay)}</span></td>
          <td class="left name">{s.name}</td>
          <td class="num total mid" class:zero={s.count === 0}>{s.count || "–"}</td>
          <td class="mid">{@render pricePair(p, "Lowest sell and highest buy for the whole set on warframe.market")}</td>
          <td>{#if s.vaulted}<span class="tag">Vaulted</span>{/if}</td>
          <td class="caret" aria-hidden="true">{openSet[s.path] ? "▾" : "▸"}</td>
        </tr>
        {#if openSet[s.path]}
          <tr class="rewards-row">
            <td></td>
            <td colspan="5">
              <ul class="rewards set-parts">
                {#each s.parts as part (part.path)}
                  <li use:watchPrice={priceKey(part.path, 0)}>
                    <span class="reward-icon">{@render image(part.image, "", part.overlay)}</span>
                    <span class="reward-name">
                      {part.name}
                      <span class="owned" class:none={part.owned === 0}>
                        Owned {part.owned.toLocaleString()}{#if part.needed > 1}<span class="muted"> · {part.needed} per set</span>{/if}
                      </span>
                    </span>
                    {@render pricePair(prices[priceKey(part.path, 0)], "Lowest sell and highest buy on warframe.market")}
                  </li>
                {/each}
              </ul>
            </td>
          </tr>
        {/if}
      {:else}
        <tr><td colspan="6" class="empty">No prime sets match.</td></tr>
      {/each}
    </tbody>
  </table>
{:else if tab === "relics"}
  <table class="relics">
    <thead>
      <tr>
        <th></th>
        <th class="left">Relic</th>
        {#each [...REFINEMENTS, "Total"] as label, i}
          <th aria-sort={relicCol === i ? (relicDesc ? "descending" : "ascending") : "none"}>
            <button class="col-sort" class:active={relicCol === i} onclick={() => sortRelics(i)} title="Sort by {label} count">
              {label}{@render sortIcon(relicCol === i, relicDesc)}
            </button>
          </th>
        {/each}
        <th></th>
        <th></th>
      </tr>
    </thead>
    <tbody>
      {#each relics as r (r.name)}
        <tr class="relic-row" class:open={expanded[r.name]} onclick={() => (expanded[r.name] = !expanded[r.name])}>
          <td class="icon">{@render image(r.image, r.era)}</td>
          <td class="left name">{r.name}</td>
          {#each r.counts as c}<td class="num" class:zero={c === 0}>{c || "–"}</td>{/each}
          <td class="num total">{r.total}</td>
          <td>{#if r.vaulted}<span class="tag">Vaulted</span>{/if}</td>
          <td class="caret" aria-hidden="true">{expanded[r.name] ? "▾" : "▸"}</td>
        </tr>
        {#if expanded[r.name]}
          <tr class="rewards-row">
            <td></td>
            <td colspan="8">
              <ul class="rewards">
                {#each r.rewards as w}
                  <li>
                    <span class="reward-icon">{@render image(w.image, "", w.overlay)}</span>
                    <span class="reward-name rarity {w.rarity.toLowerCase()}">{w.name}</span>
                    <span class="rarity {w.rarity.toLowerCase()}">{w.rarity}</span>
                    <span class="owned" class:none={w.owned === 0}>Owned {w.owned.toLocaleString()}</span>
                  </li>
                {:else}
                  <li class="muted">No reward data for this relic.</li>
                {/each}
              </ul>
            </td>
          </tr>
        {/if}
      {:else}
        <tr><td colspan="9" class="empty">No relics match.</td></tr>
      {/each}
    </tbody>
  </table>
{:else if tab === "mods" && modView === "prices"}
  {@render modViewSwitch()}
  <div class="price-grid">
    {#each modRows as row (row.key)}
      {@const m = row.m}
      <!-- Every row shows the unranked (rank 0) price, whatever rank the copies are. -->
      {@const p = prices[priceKey(m.path, 0)]}
      <article
        class="price-row {(m.rarity ?? 'common').toLowerCase()}"
        class:untradable={p?.status === "untradable"}
        use:watchPrice={priceKey(m.path, 0)}
      >
        <div class="price-thumb">
          {#if m.wikiImage && !wikiFailed[m.path]}
            <img src={imageUrl(m.wikiImage)} alt="" loading="lazy" onerror={() => (wikiFailed[m.path] = true)} />
          {:else}
            {@render image(m.image)}
          {/if}
        </div>
        <div class="price-body">
          <h3 title={m.compat ? `${m.name} (${m.compat})` : m.name}>{m.name}</h3>
          <div class="price-line">
            <span class="qty">×{row.count.toLocaleString()}</span>
            <span class="lvl">Lvl: <b>{row.rank}</b>/{m.maxRank ?? "?"}</span>
          </div>
          {#if p?.status === "untradable"}
            <div class="not-tradable">Not on warframe.market</div>
          {:else}
            <div class="pills" class:stale={p?.status === "stale"} title={p?.fetchedAt ? `Rank 0 price from ${new Date(p.fetchedAt).toLocaleTimeString()}` : "Loading price"}>
              <span class="pill wts">
                <span class="side">WTS</span>
                <span class="plat">{priceText(p, "wts")}{@render platinum()}</span>
              </span>
              <span class="pill wtb">
                <span class="side">WTB</span>
                <span class="plat">{priceText(p, "wtb")}{@render platinum()}</span>
              </span>
            </div>
          {/if}
        </div>
      </article>
    {:else}
      <p class="empty">Nothing matches.</p>
    {/each}
  </div>
{:else if tab === "mods"}
  {@render modViewSwitch()}
  <div class="grid">
    {#each mods as m (m.path)}
      <article class="wiki-tile" title={m.compat ? `${m.name} (${m.compat})` : m.name}>
        {#if m.wikiImage && !wikiFailed[m.path]}
          <img
            class="wiki-card"
            src={imageUrl(m.wikiImage)}
            alt={m.name}
            loading="lazy"
            onerror={() => (wikiFailed[m.path] = true)}
          />
        {:else}
          {@render modCard(m)}
        {/if}
        <div class="wiki-name">{m.name}</div>
        <div class="wiki-total">{m.count.toLocaleString()} owned</div>
        <ul class="rank-chips" aria-label="Copies per rank">
          {#each m.ranks as r}
            <li class:maxed={m.maxRank !== null && r.rank >= m.maxRank}>
              <span class="rank-label">Rank {r.rank}</span>
              <span class="rank-count">×{r.count.toLocaleString()}</span>
            </li>
          {/each}
        </ul>
      </article>
    {:else}
      <p class="empty">Nothing matches.</p>
    {/each}
  </div>
{:else if tab === "arcanes"}
  <div class="grid small">
    {#each arcanes as m (m.path)}
      <article class="tile" title={m.compat ? `${m.name} (${m.compat})` : m.name}>
        <span class="amount">×{compact(m.count)}</span>
        <div class="tile-image">{@render image(m.image)}</div>
        <h3>{m.name}</h3>
        <div class="meta">
          {#if m.rarity}<span class="rarity {m.rarity.toLowerCase()}">{m.rarity}</span>{/if}
          {#if m.polarity}<span>{capitalize(m.polarity)}</span>{/if}
          {#if m.drain !== null}<span>Drain {m.drain}</span>{/if}
        </div>
        <div class="ranks">{ranksText(m)}{#if m.maxRank !== null}<span class="muted"> / max R{m.maxRank}</span>{/if}</div>
      </article>
    {:else}
      <p class="empty">Nothing matches.</p>
    {/each}
  </div>
{:else}
  <div class="grid small">
    {#each tab === "parts" ? parts : misc as p (p.path)}
      <article class="tile" title={p.name}>
        <span class="amount">×{compact(p.count)}</span>
        <div class="tile-image">{@render image(p.image, "", p.overlay, p.underlay ?? null)}</div>
        <h3>{p.name}</h3>
        <div class="meta">
          <span>{p.kind}</span>
          {#if tab === "parts" && p.prime}<span class="prime">Prime</span>{/if}
        </div>
      </article>
    {:else}
      <p class="empty">Nothing matches.</p>
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
    margin-bottom: 10px;
  }

  .tabs,
  .tools,
  .filters,
  .chip-group {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
  }

  /* Each filter is its own labelled panel, spaced well apart from the next. */
  .filters {
    gap: 16px 28px;
    margin: 4px 0 20px;
    padding-bottom: 18px;
    border-bottom: 1px solid var(--line);
  }

  .filter-group {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }

  .filter-label {
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--muted);
  }

  .chip-group {
    gap: 6px;
  }

  .filter-group select {
    min-width: 180px;
  }

  button,
  input[type="search"],
  select {
    font: inherit;
    color: var(--text);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 6px 12px;
  }

  button {
    cursor: pointer;
  }

  button:hover {
    background: var(--surface-hover);
    border-color: var(--line-strong);
  }

  .tabs button.active,
  .chip-group button.selected {
    background: var(--accent);
    border-color: var(--accent);
    color: white;
    font-weight: 600;
  }

  .chip-group button {
    padding: 5px 12px;
    font-size: 13px;
    background: var(--surface-2);
  }

  .count {
    margin-left: 6px;
    font-size: 12px;
    opacity: 0.7;
  }

  input[type="search"] {
    width: 220px;
    outline: none;
  }

  input[type="search"]:focus,
  select:focus {
    border-color: var(--accent);
    outline: none;
  }

  .empty,
  .muted {
    color: var(--muted);
  }

  /* Tiles */
  /* At most 5 tiles per row: each column is at least a fifth of the width (or 260px on
     narrow windows, which then shows fewer per row). */
  .grid {
    --gap: 14px;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(max(260px, calc((100% - 4 * var(--gap)) / 5)), 1fr));
    gap: var(--gap);
  }

  /* Parts, Arcanes, Misc: slightly smaller tiles, at most 6 per row. */
  .grid.small {
    --gap: 12px;
    grid-template-columns: repeat(auto-fill, minmax(max(220px, calc((100% - 5 * var(--gap)) / 6)), 1fr));
  }

  .grid.small .tile {
    padding: 14px 12px 12px;
  }

  .grid.small .tile-image {
    width: 108px;
    height: 108px;
  }

  .tile {
    position: relative;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 18px 16px 16px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    text-align: center;
  }

  .tile:hover {
    border-color: var(--line-strong);
  }

  .amount {
    position: absolute;
    top: 10px;
    right: 10px;
    font-size: 14px;
    font-weight: 700;
    padding: 2px 9px;
    border-radius: 5px;
    background: var(--surface-2);
  }

  .tile-image {
    width: 128px;
    height: 128px;
  }

  .tile-image img,
  .tile-image .placeholder {
    width: 100%;
    height: 100%;
    object-fit: contain;
    border-radius: 6px;
  }

  /* An image with an optional overlay: the built item drawn over its blueprint. */
  .stack {
    position: relative;
    display: block;
    width: 100%;
    height: 100%;
  }

  /* Drawn first, at full size, so the main image (a Tauforged shard's glow) sits on top. */
  .stack img.underlay {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
  }

  .stack img:not(.underlay):not(.overlay) {
    position: relative;
  }

  .stack img.overlay {
    position: absolute;
    top: 14%;
    left: 14%;
    width: 72%;
    height: 72%;
    object-fit: contain;
    border-radius: 0;
    background: none;
  }

  .tile h3 {
    font-size: 18px;
    font-weight: 600;
    margin: 0;
    line-height: 1.3;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .meta {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 4px 10px;
    font-size: 14px;
    color: var(--muted);
  }

  .ranks {
    font-size: 14px;
  }

  .prime {
    color: var(--gold);
    font-weight: 600;
  }

  /* Rarity colours (mods, arcanes, relic rewards) */
  .rarity {
    font-weight: 600;
  }

  .rarity.common {
    color: #c98b5a;
  }

  .rarity.uncommon {
    color: #c3ccd8;
  }

  .rarity.rare {
    color: var(--gold);
  }

  .rarity.legendary {
    color: #d9e4f5;
  }

  .placeholder,
  img:global(.missing) {
    background: var(--line);
    color: transparent;
  }

  /* B / C: app-drawn mod card. Original styling; border colour = rarity. */
  /* Mods tab: one wiki card per mod, with copies and ranks underneath. Cards are shown at most
     at their natural width (312px) so they stay sharp. */
  .wiki-tile {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }

  /* The aspect ratio reserves the card's space before the image loads. Without it every card
     starts 0px tall, the browser treats them all as on screen, and lazy loading downloads all
     of them at once. 312×441 is the wiki's card size. */
  .wiki-card {
    width: 100%;
    max-width: 312px;
    aspect-ratio: 312 / 441;
    height: auto;
    object-fit: contain;
    display: block;
  }

  .wiki-name {
    font-size: 17px;
    font-weight: 700;
    text-align: center;
  }

  .wiki-total {
    font-size: 13px;
    color: var(--muted);
    margin-top: -4px;
  }

  /* One chip per rank level owned; a maxed rank is highlighted. */
  .rank-chips {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 6px;
  }

  .rank-chips li {
    display: flex;
    gap: 6px;
    align-items: baseline;
    padding: 3px 10px;
    border-radius: 6px;
    background: var(--surface-2);
    border: 1px solid var(--line);
    font-size: 13px;
  }

  .rank-chips li.maxed {
    border-color: var(--accent);
  }

  .rank-label {
    color: var(--muted);
  }

  .rank-count {
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }

  .mod-card {
    --rarity: #c98b5a;
    position: relative;
    width: 100%;
    max-width: 312px;
    background: #0b111c;
    border: 2px solid var(--rarity);
    border-radius: 10px;
    padding: 10px 12px 12px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    text-align: center;
  }

  .mod-card.uncommon {
    --rarity: #c3ccd8;
  }

  .mod-card.rare {
    --rarity: var(--gold);
  }

  .mod-card.legendary {
    --rarity: #d9e4f5;
  }

  .mod-top {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
  }

  .mod-drain {
    font-weight: 700;
    font-size: 14px;
    padding: 0 7px;
    border-radius: 4px;
    background: var(--surface-2);
  }

  .mod-polarity {
    color: var(--muted);
  }

  .mod-count {
    margin-left: auto;
    font-weight: 700;
  }

  .mod-art {
    width: 100%;
    height: 170px;
    border-radius: 6px;
    overflow: hidden;
    background: var(--surface-2);
  }

  .mod-art img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .mod-name {
    font-size: 16px;
    font-weight: 700;
    color: var(--rarity);
  }

  .mod-stats {
    list-style: none;
    margin: 0;
    padding: 0;
    font-size: 13px;
    line-height: 1.35;
    min-height: 2.7em;
  }

  .mod-compat {
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.6px;
    padding: 2px 10px;
    border: 1px solid var(--line-strong);
    border-radius: 999px;
    color: var(--muted);
  }

  .mod-pips {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 3px;
  }

  .mod-pips span {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    border: 1px solid var(--rarity);
  }

  .mod-pips span.on {
    background: var(--rarity);
  }

  /* Mods: view switch */
  .view-switch {
    margin-bottom: 16px;
    width: fit-content;
  }

  .view-switch .source,
  .filters .source {
    font-size: 12px;
  }

  .filters .source {
    align-self: center;
  }

  /* Mods: Prices view. One row per owned rank, rarity colour on the left edge. */
  .price-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(460px, 1fr));
    gap: 12px;
  }

  .price-row {
    --rarity: #c98b5a;
    display: flex;
    gap: 14px;
    align-items: center;
    background: var(--surface);
    border: 1px solid var(--line);
    border-left: 4px solid var(--rarity);
    border-radius: var(--radius);
    padding: 10px 14px 10px 10px;
    min-height: 132px;
  }

  .price-row.uncommon {
    --rarity: #c3ccd8;
  }

  .price-row.rare {
    --rarity: var(--gold);
  }

  .price-row.legendary {
    --rarity: #d9e4f5;
  }

  .price-thumb {
    flex: none;
    width: 80px;
    aspect-ratio: 312 / 441;
  }

  .price-thumb img,
  .price-thumb .placeholder {
    width: 100%;
    height: 100%;
    object-fit: contain;
    display: block;
  }

  .price-body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .price-body h3 {
    margin: 0;
    font-size: 19px;
    font-weight: 700;
    text-align: center;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .price-line {
    display: flex;
    justify-content: space-around;
    font-size: 15px;
  }

  .qty {
    font-weight: 700;
  }

  .lvl {
    color: var(--muted);
  }

  .lvl b {
    color: var(--text);
  }

  .pills {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }

  .pills.stale {
    opacity: 0.6;
  }

  .pill {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 14px;
    border-radius: 999px;
    font-size: 15px;
  }

  .pill.wts {
    background: rgba(248, 113, 113, 0.16);
    border: 1px solid rgba(248, 113, 113, 0.35);
  }

  .pill.wtb {
    background: rgba(52, 211, 153, 0.13);
    border: 1px solid rgba(52, 211, 153, 0.32);
  }

  .side {
    font-weight: 600;
    color: var(--muted);
  }

  .plat {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 700;
    font-size: 17px;
    font-variant-numeric: tabular-nums;
  }

  .plat-icon {
    width: 22px;
    height: 22px;
    object-fit: contain;
  }

  .not-tradable {
    text-align: center;
    color: var(--muted);
    font-size: 14px;
    padding: 9px 0;
  }

  .price-row.untradable {
    opacity: 0.75;
  }

  /* Relics */
  .relics {
    width: 100%;
    border-collapse: collapse;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    overflow: hidden;
    font-size: 17px;
  }

  /* Column headers that sort (relic counts, set count and price). Plain text, no button look:
     the arrows and a brighter label on hover say they're clickable. */
  .col-sort {
    all: unset;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    transition: color 120ms ease-out;
  }

  /* The page's button:hover would paint a background; headers stay plain. */
  .col-sort:hover {
    background: none;
  }

  .col-sort:hover,
  .col-sort.active {
    color: var(--text);
  }

  .sort-icon {
    width: 7px;
    height: 11px;
    flex: none;
  }

  .sort-icon path {
    fill: currentColor;
    opacity: 0.3;
    transition: opacity 120ms ease-out, fill 120ms ease-out;
  }

  .col-sort:hover .sort-icon path {
    opacity: 0.6;
  }

  .sort-icon path.on {
    fill: var(--accent);
    opacity: 1;
  }

  .col-sort:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
    border-radius: 3px;
  }

  .relics th {
    font-size: 14px;
    font-weight: 600;
    color: var(--muted);
    text-align: right;
    padding: 14px 18px;
    border-bottom: 1px solid var(--line);
  }

  .relics td {
    padding: 12px 18px;
    border-bottom: 1px solid var(--line);
    text-align: right;
  }

  .relics .left {
    text-align: left;
  }

  .relic-row {
    cursor: pointer;
  }

  .relic-row:hover,
  .relic-row.open {
    background: var(--surface-2);
  }

  .relics .icon {
    width: 72px;
    padding-right: 0;
  }

  .relics .icon img,
  .relics .icon .placeholder {
    width: 52px;
    height: 52px;
    object-fit: contain;
    display: block;
  }

  .relics .name {
    font-weight: 600;
    font-size: 19px;
  }

  .num {
    font-variant-numeric: tabular-nums;
  }

  .num.zero {
    color: var(--muted);
  }

  .total {
    font-weight: 700;
  }

  .caret {
    width: 36px;
    font-size: 18px;
    color: var(--muted);
  }

  .tag {
    font-size: 14px;
    padding: 2px 10px;
    border-radius: 4px;
    background: var(--surface-2);
    color: var(--muted);
  }

  .rewards-row td {
    background: var(--surface-2);
    text-align: left;
  }

  .rewards {
    list-style: none;
    margin: 0;
    padding: 8px 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(380px, 1fr));
    gap: 12px 24px;
    font-size: 16px;
  }

  .rewards li {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .reward-icon {
    flex: none;
    width: 56px;
    height: 56px;
  }

  .reward-icon img,
  .reward-icon .placeholder {
    width: 100%;
    height: 100%;
    object-fit: contain;
    display: block;
  }

  .reward-name {
    flex: 1;
  }

  .owned {
    font-size: 14px;
    font-variant-numeric: tabular-nums;
    color: var(--good);
  }

  .owned.none {
    color: var(--muted);
  }

  /* Prime sets: the Relics table's look, with the set's price per row and each part's below. */
  .sets .mid {
    text-align: center;
  }

  .sets .set-icon-cell {
    width: 72px;
    padding-right: 0;
  }

  .set-icon {
    display: block;
    width: 52px;
    height: 52px;
  }

  .set-icon :global(img:not(.overlay)),
  .set-icon .placeholder {
    width: 100%;
    height: 100%;
    object-fit: contain;
    display: block;
  }

  .set-parts {
    grid-template-columns: repeat(auto-fill, minmax(460px, 1fr));
  }

  .set-parts .reward-name {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .set-prices {
    display: inline-flex;
    gap: 8px;
  }

  .set-prices.stale {
    opacity: 0.7;
  }

  .set-price {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 10px;
    border-radius: 999px;
    font-size: 15px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .set-price .side {
    font-size: 12px;
  }

  .set-price.wts {
    background: rgba(248, 113, 113, 0.16);
    border: 1px solid rgba(248, 113, 113, 0.35);
  }

  .set-price.wtb {
    background: rgba(52, 211, 153, 0.13);
    border: 1px solid rgba(52, 211, 153, 0.32);
  }

  .set-price .plat-icon {
    width: 16px;
    height: 16px;
  }
</style>
