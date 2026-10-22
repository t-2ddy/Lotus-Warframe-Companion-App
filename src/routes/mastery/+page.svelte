<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { compact, hideOnError, imageUrl, markMissing } from "$lib/format";

  type Part = { path: string; name: string; image: string | null; overlay: string | null; needed: number; owned: number };
  type Progress = { label: string; done: number; total: number };
  type Suggestion = {
    path: string;
    name: string;
    image: string | null;
    tab: string;
    xpGain: number;
    rank: number;
    maxRank: number;
    owned: boolean;
    building: boolean;
    favorite: boolean;
    parts: Part[];
    /** From relics: relic parts owned / in the set, and owned relics that drop the missing ones. */
    relicPartsOwned: number;
    relicPartsTotal: number;
    relicSources: string[];
    ready: boolean;
    buy: { path: string; count: number }[];
    /** With platinum: no parts owned (resources aside), so the whole set is the thing to buy. */
    ownsNoParts: boolean;
    /** From relics: each missing part, the owned relics that drop it (Intact chance), for the popup. */
    relicNeeds: RelicNeed[];
    /** Easy: owned, building, ready (build it now) or set (every part held, some to craft). */
    stage: "owned" | "building" | "ready" | "set" | "";
  };
  type RelicNeed = {
    path: string;
    name: string;
    image: string | null;
    overlay: string | null;
    missing: number;
    relics: { name: string; owned: number; chance: number }[];
  };
  type MasteryData = {
    hasData: boolean;
    rank: number;
    totalXp: number;
    rankStart: number;
    nextRank: number;
    readyForTest: boolean;
    breakdown: { items: number; nodes: number; steelPathNodes: number; junctions: number; intrinsics: number };
    content: Progress[];
    starChart: Progress[];
    intrinsics: Progress[];
    easy: Suggestion[];
    relics: Suggestion[];
    platinum: Suggestion[];
  };
  type PriceView = {
    path: string;
    rank: number;
    wts: number | null;
    wtb: number | null;
    status: "fresh" | "stale" | "loading" | "untradable";
  };

  type Mode = "easy" | "relics" | "platinum";
  const MODES: { id: Mode; label: string }[] = [
    { id: "easy", label: "Easy" },
    { id: "relics", label: "From relics" },
    { id: "platinum", label: "With platinum" },
  ];

  let data = $state<MasteryData | null>(null);
  let mode = $state<Mode>("easy");
  /** Lowest sell prices (rank 0) by part path. */
  let prices = $state<Record<string, PriceView>>({});
  let pricesRequested = false;

  const load = () =>
    invoke<MasteryData>("get_mastery")
      .then((d) => (data = d))
      .catch(() => {});

  onMount(() => {
    load();
    const offUpdated = listen("inventory-updated", load);
    const offPrice = listen<PriceView>("market-price", (e) => {
      if (e.payload.rank === 0) prices[e.payload.path] = e.payload;
    });
    return () => {
      offUpdated.then((f) => f());
      offPrice.then((f) => f());
    };
  });

  /** Platinum mode needs every part's price to sort by cost, so it asks for all of them once.
   *  The backend fetches them at ~3 per second and keeps each for 2 weeks. */
  async function requestAllPrices() {
    if (pricesRequested || !data) return;
    pricesRequested = true;
    const paths = [...new Set(data.platinum.flatMap((s) => s.buy.map((b) => b.path)))];
    try {
      const result = await invoke<PriceView[]>("request_prices", { keys: paths.map((path) => ({ path, rank: 0 })) });
      for (const p of result) prices[p.path] = p;
    } catch {
      pricesRequested = false;
    }
  }

  $effect(() => {
    if (mode === "platinum" && data) requestAllPrices();
  });

  const pct = (p: { done: number; total: number }) => (p.total ? Math.round((100 * p.done) / p.total) : 0);
  /** Segment colours for a box's rows, in order (a box has at most 4 rows). */
  const SEGMENT_COLORS = ["#3b82f6", "#2dd4bf", "#fbbf24", "#f472b6", "#a78bfa"];
  /** Each row's share of the box total as a ring segment: start and length out of 100. */
  function segments(rows: Progress[], total: number) {
    let start = 0;
    return rows.map((r, i) => {
      const length = total ? (100 * r.done) / total : 0;
      const seg = { start, length, color: SEGMENT_COLORS[i % SEGMENT_COLORS.length] };
      start += length;
      return seg;
    });
  }
  /** The progress-box row under the pointer (box title and row index); its ring segment grows. */
  let hoveredRow = $state<{ box: string; i: number } | null>(null);
  const isHovered = (box: string, i: number) => hoveredRow?.box === box && hoveredRow.i === i;
  const hoverRow = (box: string, i: number) => () => (hoveredRow = { box, i });
  const leaveRow = (box: string, i: number) => () => isHovered(box, i) && (hoveredRow = null);

  const sum = (list: Progress[]) => ({
    done: list.reduce((a, p) => a + p.done, 0),
    total: list.reduce((a, p) => a + p.total, 0),
  });

  const progressInRank = $derived(data ? data.totalXp - data.rankStart : 0);
  const rankSpan = $derived(data ? data.nextRank - data.rankStart : 1);
  const rankPct = $derived(Math.min(100, Math.max(0, (100 * progressInRank) / rankSpan)));

  /** The Easy card under the pointer: its XP shows as a lighter stretch on the rank bar. */
  let hovered = $state<Suggestion | null>(null);
  const previewGain = $derived(mode === "easy" && hovered ? hovered.xpGain : 0);
  const previewPct = $derived(Math.min(100, Math.max(0, (100 * (progressInRank + previewGain)) / rankSpan)));
  const previewRanksUp = $derived(previewGain > 0 && progressInRank + previewGain >= rankSpan);

  /** The rank header sticks to the top of the page and shrinks once the page scrolls down.
   *  Pages scroll inside the layout's .content. The gap between the two thresholds is bigger than
   *  the height the header loses, so shrinking it can't scroll the page back over the threshold. */
  let headEl = $state<HTMLElement | null>(null);
  let compactHead = $state(false);
  $effect(() => {
    const scroller = headEl?.closest<HTMLElement>(".content");
    if (!scroller) return;
    const onScroll = () => {
      if (!compactHead && scroller.scrollTop > 90) compactHead = true;
      else if (compactHead && scroller.scrollTop < 10) compactHead = false;
    };
    onScroll();
    scroller.addEventListener("scroll", onScroll, { passive: true });
    return () => scroller.removeEventListener("scroll", onScroll);
  });

  /** Platinum cost of a suggestion: null while any part is unpriced, undefined if a part can't be bought. */
  function cost(s: Suggestion): number | null | undefined {
    let total = 0;
    for (const b of s.buy) {
      const p = prices[b.path];
      if (!p || (p.status === "loading" && p.wts === null)) return null;
      if (p.status === "untradable" || p.wts === null) return undefined;
      total += p.wts * b.count;
    }
    return total;
  }

  const priced = $derived(
    (data?.platinum ?? [])
      .map((s) => ({ s, cost: cost(s) }))
      .filter((x) => x.cost !== undefined)
      .sort((a, b) => (a.cost ?? Infinity) - (b.cost ?? Infinity) || b.s.xpGain - a.s.xpGain),
  );
  const pricedCount = $derived(Object.values(prices).filter((p) => p.status !== "loading").length);
  const partCount = $derived(new Set((data?.platinum ?? []).flatMap((s) => s.buy.map((b) => b.path))).size);
  const platinumTotal = $derived(priced.reduce((a, x) => a + (x.cost ?? 0), 0));

  const list = $derived(
    mode === "easy" ? (data?.easy ?? []) : mode === "relics" ? (data?.relics ?? []) : priced.map((x) => x.s),
  );

  /** The "From relics" or "With platinum" card whose popup is open, and which tab opened it. */
  let detail = $state<Suggestion | null>(null);
  let detailMode = $state<Mode>("relics");

  /** Opens a set's popup and asks for the prices it shows (cached by the backend): the missing
   *  parts from relics; the set and every part with platinum (a set's market path is its own). */
  async function openDetail(s: Suggestion) {
    detail = s;
    detailMode = mode;
    const paths = mode === "platinum" ? [s.path, ...s.parts.map((p) => p.path)] : s.relicNeeds.map((n) => n.path);
    const keys = [...new Set(paths)].filter((path) => !prices[path]).map((path) => ({ path, rank: 0 }));
    if (!keys.length) return;
    try {
      const result = await invoke<PriceView[]>("request_prices", { keys });
      for (const p of result) prices[p.path] = p;
    } catch {}
  }

  function onCardKey(e: KeyboardEvent, s: Suggestion) {
    if (e.target !== e.currentTarget) return;
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      openDetail(s);
    }
  }

  /** Buying every missing part of the open set: null while any is unpriced, undefined if one can't be bought. */
  const detailCost = $derived.by(() => {
    if (!detail) return null;
    if (detailMode === "platinum") return cost(detail);
    let total = 0;
    for (const n of detail.relicNeeds) {
      const p = prices[n.path];
      if (!p || (p.status === "loading" && p.wts === null)) return null;
      if (p.status === "untradable" || p.wts === null) return undefined;
      total += p.wts * n.missing;
    }
    return total;
  });

  async function toggleFavorite(s: Suggestion) {
    const on = !s.favorite;
    s.favorite = on;
    try {
      await invoke("set_favorite", { item: s.path, on });
    } catch {
      s.favorite = !on;
    }
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && detail && (detail = null)} />

{#snippet platinum()}
  <img class="plat-icon" src="/icons/platinum.png" alt="platinum" />
{/snippet}

{#snippet priceText(price: PriceView | undefined, count: number, verb: string)}
  {#if !price || (price.status === "loading" && price.wts === null)}
    <span class="muted">…</span>
  {:else if price.wts === null}
    <span class="muted">Not traded</span>
  {:else}
    {verb} {(price.wts * count).toLocaleString()} {@render platinum()}
  {/if}
{/snippet}

{#snippet progressBox(title: string, rows: Progress[])}
  {@const all = sum(rows)}
  {@const segs = segments(rows, all.total)}
  <section class="box">
    <h2>{title}</h2>
    <div class="box-body">
      <!-- Progress ring, one coloured segment per row sized by its share of the box total.
           r = 15.9155 makes the circumference 100, so dash lengths are percents. -->
      <div class="ring-wrap">
      <svg class="ring" viewBox="0 0 36 36" role="img" aria-label="{title}: {pct(all)}% done">
        <circle class="ring-track" cx="18" cy="18" r="15.9155" />
        {#each segs as s, i}
          {#if s.length > 0}
            <!-- svelte-ignore a11y_no_static_element_interactions (pointer-only highlight; the rows carry the same data) -->
            <circle
              class="ring-seg"
              class:active={isHovered(title, i)}
              class:dim={hoveredRow?.box === title && !isHovered(title, i)}
              onpointerenter={hoverRow(title, i)}
              onpointerleave={leaveRow(title, i)}
              cx="18"
              cy="18"
              r="15.9155"
              stroke={s.color}
              stroke-dasharray="{s.length} {100 - s.length}"
              stroke-dashoffset={-s.start}
            >
              <title>{rows[i].label}: {rows[i].done}/{rows[i].total}</title>
            </circle>
          {/if}
        {/each}
      </svg>
      <!-- HTML over the ring, not SVG text: redrawing the rotated SVG as segments animate made
           counter-rotated text shake by a sub-pixel. -->
      <span class="ring-pct" aria-hidden="true">{pct(all)}%</span>
      </div>
      <dl class:hovering={hoveredRow?.box === title}>
        {#each rows as r, i}
          <dt class:active={isHovered(title, i)} onpointerenter={hoverRow(title, i)} onpointerleave={leaveRow(title, i)}>
            <span class="swatch" style="background: {segs[i].color}"></span>{r.label}
          </dt>
          <dd class:active={isHovered(title, i)} onpointerenter={hoverRow(title, i)} onpointerleave={leaveRow(title, i)}>
            <span class="row-pct">{pct(r)}%</span> {r.done}/{r.total}
          </dd>
        {/each}
      </dl>
    </div>
  </section>
{/snippet}

{#if !data}
  <p class="muted">Loading…</p>
{:else if !data.hasData}
  <p class="muted">No inventory captured yet. Log in or finish a mission with the app open.</p>
{:else}
  <header class="rank-head" class:compact={compactHead} bind:this={headEl}>
    <div class="rank">
      <span class="rank-number">{data.rank > 30 ? `L${data.rank - 30}` : data.rank}</span>
      <img
        class="rank-icon"
        src="/icons/ranks/rank{Math.min(Math.max(data.rank, 0), 51)}.png"
        alt=""
        title="Mastery Rank {data.rank > 30 ? `Legendary ${data.rank - 30}` : data.rank}"
      />
    </div>
    <div class="xp">
      <div class="xp-text">
        <span class="xp-pct">
          {Math.floor(rankPct)}%
          {#if previewGain > 0}
            <span class="gain">+{previewGain.toLocaleString()} XP → {previewRanksUp ? "rank up" : `${Math.floor(previewPct)}%`}</span>
          {/if}
        </span>
        <span>
          {progressInRank.toLocaleString()} / {rankSpan.toLocaleString()} XP
          {#if data.readyForTest}<span class="ready">· Rank test available</span>{/if}
        </span>
      </div>
      <div class="bar">
        <div class="preview" style="width: {previewGain > 0 ? previewPct : rankPct}%"></div>
        <div class="fill" style="width: {rankPct}%"></div>
      </div>
      <div class="breakdown">
        Total {data.totalXp.toLocaleString()} XP: items {data.breakdown.items.toLocaleString()} ·
        star chart {data.breakdown.nodes.toLocaleString()} · Steel Path {data.breakdown.steelPathNodes.toLocaleString()} ·
        junctions {data.breakdown.junctions.toLocaleString()} · intrinsics {data.breakdown.intrinsics.toLocaleString()}
      </div>
    </div>
  </header>

  <div class="boxes">
    {@render progressBox("Game content", data.content)}
    {@render progressBox("Star chart", data.starChart)}
    {@render progressBox("Intrinsics", data.intrinsics)}
  </div>

  <section class="ways">
    <div class="ways-head">
      <h2>Best ways to level up mastery</h2>
      <div class="modes" role="tablist">
        {#each MODES as m}
          <button role="tab" aria-selected={mode === m.id} class:active={mode === m.id} onclick={() => (mode = m.id)}>
            {m.label}
          </button>
        {/each}
      </div>
    </div>

    {#if mode === "platinum"}
      <p class="muted note">
        Cheapest way to get each item: the lowest warframe.market sell price for only the parts you're missing.
        {#if pricedCount < partCount}Pricing parts: {pricedCount} / {partCount}…{/if}
      </p>
    {:else if mode === "relics"}
      <p class="muted note">
        Prime sets closest to being built, where every missing part drops from a relic you own. Fewest missing parts first.
      </p>
    {:else}
      <p class="muted note">
        Items that aren't mastered yet: ones you own or are building, then sets ready to build, then full sets where you
        have every part (some still blueprints to craft).
      </p>
    {/if}

    <div class="cards">
      {#each list as s (s.path)}
        {@const c = mode === "platinum" ? cost(s) : null}
        {@const opens = mode !== "easy"}
        <!-- svelte-ignore a11y_no_noninteractive_tabindex (the card is role="button" whenever it's focusable) -->
        <article
          class="card lift"
          class:clickable={opens}
          role={opens ? "button" : undefined}
          tabindex={opens ? 0 : undefined}
          aria-label={mode === "relics"
            ? `${s.name}: relics and prices for the missing parts`
            : opens
              ? `${s.name}: set and part prices`
              : undefined}
          onclick={opens ? () => openDetail(s) : undefined}
          onkeydown={opens ? (e) => onCardKey(e, s) : undefined}
          onpointerenter={() => (hovered = s)}
          onpointerleave={() => hovered === s && (hovered = null)}
        >
          <div class="pic">
            {#if s.image}
              <img src={imageUrl(s.image)} alt="" loading="lazy" onerror={markMissing} />
            {:else}
              <div class="placeholder"></div>
            {/if}
            <span class="xp-badge">+{s.xpGain.toLocaleString()} XP</span>
          </div>
          <div class="info">
            <div class="name-row">
              <button
                class="star"
                class:on={s.favorite}
                onclick={(e) => {
                  e.stopPropagation();
                  toggleFavorite(s);
                }}
                aria-label={s.favorite ? `Unstar ${s.name}` : `Star ${s.name}`}
                aria-pressed={s.favorite}
              >
                <svg viewBox="0 0 24 24" aria-hidden="true">
                  <path d="M12 2.5l2.9 6.1 6.6.8-4.9 4.6 1.3 6.6L12 17.3l-5.9 3.3 1.3-6.6-4.9-4.6 6.6-.8z" />
                </svg>
              </button>
              <h3 title={s.name}>{s.name}</h3>
            </div>

            {#if mode === "easy"}
              <div class="detail">
                <span>Level <b>{s.rank}</b>/{s.maxRank}</span>
                {#if s.stage === "building"}<span class="chip building">Foundry</span>
                {:else if s.stage === "ready"}<span class="chip owned">Ready to build</span>
                {:else if s.stage === "set"}<span class="chip building" title="You have every part; some are still blueprints to craft, or resources are short">Full set</span>
                {:else}<span class="chip owned">Inventory</span>{/if}
              </div>
            {:else if mode === "relics"}
              <div class="detail">
                <span class="set-count"><b>{s.relicPartsOwned}</b>/{s.relicPartsTotal} relic parts</span>
                <span class="set-bar" aria-hidden="true">
                  <span style="width: {(100 * s.relicPartsOwned) / Math.max(1, s.relicPartsTotal)}%"></span>
                </span>
                {#if s.ready}<span class="chip owned">Ready to build</span>{/if}
              </div>
              {#if s.relicSources.length}
                <div class="sources" title={s.relicSources.join(", ")}>From: {s.relicSources.join(" · ")}</div>
              {/if}
            {:else}
              <div class="detail">
                <span class="price">
                  {#if c === null}…{:else}{c?.toLocaleString()}{/if}
                  {@render platinum()}
                </span>
              </div>
            {/if}

            {#if (mode !== "easy" || s.stage === "ready" || s.stage === "set") && s.parts.length}
              <ul class="parts" class:big={mode === "relics"}>
                {#each s.parts as p}
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
        </article>
      {:else}
        <p class="muted">Nothing here right now.</p>
      {/each}
    </div>

    {#if detail}
      {@const d = detail}
      <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
      <div class="backdrop" onclick={(e) => e.target === e.currentTarget && (detail = null)}>
        <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="relic-detail-title">
          <header class="dialog-head">
            {#if d.image}<img class="dialog-pic" src={imageUrl(d.image)} alt="" onerror={markMissing} />{/if}
            <div>
              <h2 id="relic-detail-title">{d.name}</h2>
              <p class="muted">
                {#if detailMode === "platinum"}
                  {d.buy.length} part{d.buy.length === 1 ? "" : "s"} to buy · +{d.xpGain.toLocaleString()} XP
                {:else}
                  {d.relicPartsOwned}/{d.relicPartsTotal} relic parts · +{d.xpGain.toLocaleString()} XP
                {/if}
              </p>
            </div>
            <!-- svelte-ignore a11y_autofocus -->
            <button class="close" onclick={() => (detail = null)} aria-label="Close" autofocus>
              <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3 3l6 6M9 3l-6 6" /></svg>
            </button>
          </header>

          {#if detailMode === "platinum"}
            {@const setPrice = prices[d.path]}
            <!-- Highlighted: what to buy. The missing parts, or the whole set when no part is owned. -->
            <ul class="needs">
              <li class:pick={d.ownsNoParts}>
                <div class="need-head">
                  <span class="need-icon">
                    {#if d.image}<img src={imageUrl(d.image)} alt="" onerror={markMissing} />{/if}
                  </span>
                  <span class="need-name">
                    {d.name} Set
                    {#if d.ownsNoParts}<span class="pick-tag">Buy this</span>{/if}
                  </span>
                  <span class="need-price" title="Lowest warframe.market sell price">
                    {@render priceText(setPrice, 1, d.ownsNoParts ? "Buy" : "")}
                  </span>
                </div>
              </li>
            </ul>
            <h3 class="dialog-sub">Parts</h3>
            <ul class="needs">
              {#each d.parts as p}
                {@const need = d.buy.find((b) => b.path === p.path)}
                {@const price = prices[p.path]}
                <li class:pick={need && !d.ownsNoParts}>
                  <div class="need-head">
                    <span class="need-icon">
                      {#if p.image}<img src={imageUrl(p.image)} alt="" onerror={markMissing} />{/if}
                      {#if p.overlay}<img class="overlay" src={imageUrl(p.overlay)} alt="" onerror={hideOnError} />{/if}
                    </span>
                    <span class="need-name">
                      {p.name}
                      <span class="muted part-count">{compact(p.owned)}/{p.needed} owned</span>
                    </span>
                    <span class="need-price" class:muted={!need} title="Lowest warframe.market sell price">
                      {#if need}
                        {@render priceText(price, need.count, need.count > 1 ? `Buy ${need.count} ·` : "Buy")}
                      {:else if p.owned >= p.needed}
                        <span class="have">Owned</span>
                      {:else}
                        <span>Resource</span>
                      {/if}
                    </span>
                  </div>
                </li>
              {/each}
            </ul>
            <footer class="dialog-foot">
              <span class="muted">Prices: lowest warframe.market sell order.</span>
              {#if detailCost !== undefined}
                <span>
                  {d.ownsNoParts ? "Parts separately:" : "Buy missing parts:"}
                  {#if detailCost === null}…{:else}<b>{detailCost.toLocaleString()}</b>{/if}
                  {@render platinum()}
                </span>
              {/if}
            </footer>
          {:else if !d.relicNeeds.length}
            <p class="muted">You have every relic part. Build it in the foundry.</p>
          {:else}
            <ul class="needs">
              {#each d.relicNeeds as n (n.path)}
                {@const price = prices[n.path]}
                <li>
                  <div class="need-head">
                    <span class="need-icon">
                      {#if n.image}<img src={imageUrl(n.image)} alt="" onerror={markMissing} />{/if}
                      {#if n.overlay}<img class="overlay" src={imageUrl(n.overlay)} alt="" onerror={hideOnError} />{/if}
                    </span>
                    <span class="need-name">{n.name}{#if n.missing > 1}<span class="muted"> ×{n.missing}</span>{/if}</span>
                    <span class="need-price" title="Lowest warframe.market sell price">
                      {#if !price || (price.status === "loading" && price.wts === null)}
                        <span class="muted">…</span>
                      {:else if price.wts === null}
                        <span class="muted">Not traded</span>
                      {:else}
                        Buy {(price.wts * n.missing).toLocaleString()} {@render platinum()}
                      {/if}
                    </span>
                  </div>
                  <ul class="drops">
                    {#each n.relics as r (r.name)}
                      <li>
                        <span class="relic">{r.name}</span>
                        <span class="muted">×{r.owned}</span>
                        <span class="chance">{r.chance.toLocaleString(undefined, { maximumFractionDigits: 2 })}%</span>
                      </li>
                    {/each}
                  </ul>
                </li>
              {/each}
            </ul>
            <footer class="dialog-foot">
              <span class="muted">Drop chances at Intact. Prices: lowest warframe.market sell order.</span>
              {#if detailCost !== undefined}
                <span>
                  Buy all missing:
                  {#if detailCost === null}…{:else}<b>{detailCost.toLocaleString()}</b>{/if}
                  {@render platinum()}
                </span>
              {/if}
            </footer>
          {/if}
        </div>
      </div>
    {/if}

    {#if mode === "platinum" && priced.length}
      <div class="total">Total: {platinumTotal.toLocaleString()} {@render platinum()}</div>
    {/if}
  </section>

  <p class="credit muted">
    Star chart mastery values: <a href="https://wiki.warframe.com/w/Module:Missions/data" target="_blank" rel="noreferrer">Warframe wiki</a>
    (CC BY-SA 3.0). Prices: warframe.market.
  </p>
{/if}

<style>
  .muted {
    color: var(--muted);
  }

  /* Header: rank and XP bar */
  /* Sticks to the top of .content. The negative margins and top cover .content's 20px padding,
     so cards scroll under a solid header instead of showing through the gap above it. */
  .rank-head {
    position: sticky;
    top: -20px;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 24px;
    margin: -20px -20px 0;
    padding: 20px;
    background: var(--bg);
    border-bottom: 1px solid transparent;
    transition:
      padding 0.2s ease-out,
      gap 0.2s ease-out,
      border-color 0.2s,
      box-shadow 0.2s;
  }

  .rank-head.compact {
    gap: 14px;
    padding: 16px 20px 8px;
    border-bottom-color: var(--line);
    box-shadow: 0 6px 14px rgba(0, 0, 0, 0.35);
  }

  .rank-head.compact .rank-number {
    font-size: 28px;
  }

  .rank-head.compact .rank-icon {
    width: 34px;
    height: 34px;
  }

  .rank-head.compact .xp-text {
    font-size: 13px;
    margin-bottom: 4px;
  }

  .rank-head.compact .bar {
    height: 10px;
  }

  .rank-head.compact .breakdown {
    max-height: 0;
    margin-top: 0;
    opacity: 0;
  }

  .rank {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .rank-number {
    font-size: 56px;
    font-weight: 700;
    line-height: 1;
    transition: font-size 0.2s ease-out;
  }

  .rank-icon {
    width: 64px;
    height: 64px;
    object-fit: contain;
    transition:
      width 0.2s ease-out,
      height 0.2s ease-out;
  }

  .xp {
    flex: 1;
    min-width: 0;
  }

  .xp-text {
    display: flex;
    justify-content: space-between;
    font-size: 16px;
    margin-bottom: 8px;
  }

  .xp-pct {
    font-weight: 700;
  }

  .ready {
    color: var(--good);
    font-weight: 600;
  }

  .bar {
    position: relative;
    height: 14px;
    border-radius: 7px;
    background: var(--surface-2);
    overflow: hidden;
    transition: height 0.2s ease-out;
  }

  .fill {
    position: relative;
    height: 100%;
    background: var(--accent);
    border-radius: 7px;
  }

  /* Hovered card's XP: a lighter bar under the fill, so only the gained stretch shows. */
  .preview {
    position: absolute;
    inset: 0 auto 0 0;
    background: rgba(96, 165, 250, 0.45);
    border-radius: 7px;
    transition: width 160ms ease-out;
  }

  .gain {
    margin-left: 8px;
    font-weight: 600;
    color: #93c5fd;
  }

  .breakdown {
    margin-top: 6px;
    max-height: 40px;
    overflow: hidden;
    font-size: 12px;
    color: var(--muted);
    transition:
      max-height 0.2s ease-out,
      margin-top 0.2s ease-out,
      opacity 0.15s;
  }

  /* Summary boxes */
  .boxes {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: 16px;
    margin-bottom: 20px;
  }

  .box {
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 14px 18px;
  }

  .box h2 {
    margin: 0 0 12px;
    font-size: 17px;
    text-align: center;
  }

  /* The ring goes above the rows when the box is too narrow for both side by side. */
  .box-body {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: center;
    gap: 12px 16px;
  }

  .ring-wrap {
    position: relative;
    flex: none;
    width: 80px;
    height: 80px;
  }

  .ring-pct {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    font-size: 17.8px;
    font-weight: 700;
    pointer-events: none;
  }

  .ring {
    display: block;
    width: 100%;
    height: 100%;
    transform: rotate(-90deg);
    /* A grown segment reaches past the viewBox edge. */
    overflow: visible;
  }

  .ring circle {
    fill: none;
    stroke-width: 3.2;
  }

  .ring-track {
    stroke: var(--surface-2);
  }

  /* Hovering a row (or the segment) grows its segment out from the centre and dims the rest. */
  .ring-seg {
    stroke-linecap: butt;
    transform-origin: 18px 18px;
    transition:
      transform 0.15s ease-out,
      opacity 0.15s;
  }

  .ring-seg.active {
    transform: scale(1.12);
  }

  .ring-seg.dim {
    opacity: 0.4;
  }

  .swatch {
    display: inline-block;
    width: 9px;
    height: 9px;
    border-radius: 2px;
    margin-right: 7px;
    vertical-align: 1px;
  }

  .box dl {
    display: grid;
    grid-template-columns: 1fr auto;
    margin: 0;
    font-size: 15px;
  }

  /* The gaps between rows and columns are padding on dt and dd instead of grid gap, so the pointer
     always sits on a row and moves straight from one to the next. */
  .box dt,
  .box dd {
    padding: 3px 0;
  }

  .box dt {
    text-align: right;
    padding-right: 14px;
  }

  /* Other rows fade by colour, not opacity: an opacity transition moves the text onto its own
     layer and back, which shifts it by a sub-pixel and makes the numbers shake. */
  .box dt,
  .box dd,
  .box .row-pct,
  .box .swatch {
    transition:
      color 0.15s,
      opacity 0.15s;
  }

  .box dl.hovering dt:not(.active),
  .box dl.hovering dd:not(.active),
  .box dl.hovering dd:not(.active) .row-pct {
    color: #4b5468;
  }

  .box dl.hovering dt:not(.active) .swatch {
    opacity: 0.4;
  }

  .box dd {
    margin: 0;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }

  .row-pct {
    display: inline-block;
    min-width: 42px;
    color: var(--muted);
  }

  /* Best ways list */
  .ways {
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 16px 18px 18px;
    position: relative;
  }

  .ways-head {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
  }

  .ways-head h2 {
    margin: 0;
    font-size: 18px;
  }

  .modes {
    display: flex;
    gap: 6px;
  }

  .modes button {
    font: inherit;
    color: var(--text);
    background: var(--surface-2);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 6px 14px;
    cursor: pointer;
  }

  .modes button.active {
    background: var(--accent);
    border-color: var(--accent);
    color: white;
    font-weight: 600;
  }

  .note {
    margin: 10px 0 14px;
    font-size: 13px;
  }

  /* Always at least 2 per row (a window a third of the screen wide still shows two); 380px
     columns once there's room. */
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(380px, calc((100% - 12px) / 2)), 1fr));
    gap: 12px;
  }

  /* On a narrow card the details wrap under the picture. */
  .card {
    container-type: inline-size;
    display: flex;
    flex-wrap: wrap;
    gap: 8px 16px;
    align-items: center;
    background: var(--surface-2);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 8px 12px;
  }

  .pic {
    position: relative;
    flex: none;
    width: 96px;
    height: 78px;
  }

  .pic img,
  .pic .placeholder {
    width: 100%;
    height: 100%;
    object-fit: contain;
  }

  .xp-badge {
    position: absolute;
    left: 0;
    bottom: 0;
    font-size: 12px;
    font-weight: 700;
    padding: 1px 6px;
    border-radius: 4px;
    background: #070a10;
    border: 1px solid var(--line-strong);
  }

  .info {
    flex: 1 1 170px;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .name-row {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .name-row h3 {
    margin: 0;
    font-size: 18px;
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

  .detail {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 14px;
    font-size: 15px;
  }

  @container (max-width: 290px) {
    .pic {
      margin: 0 auto;
    }

    .name-row h3 {
      font-size: 16px;
    }

    .detail {
      font-size: 14px;
    }
  }

  .chip {
    font-size: 13px;
    font-weight: 600;
    padding: 2px 9px;
    border-radius: 4px;
    background: var(--surface);
  }

  .chip.owned {
    color: var(--good);
  }

  .chip.building {
    color: var(--accent);
  }

  .set-count b {
    font-size: 17px;
  }

  .set-bar {
    width: 90px;
    height: 8px;
    border-radius: 4px;
    background: var(--surface);
    overflow: hidden;
  }

  .set-bar span {
    display: block;
    height: 100%;
    background: var(--good);
  }

  .card.lift {
    position: relative;
    transition:
      border-color 0.15s,
      background-color 0.15s,
      box-shadow 0.15s,
      transform 0.15s;
  }

  .card.clickable {
    cursor: pointer;
  }

  /* Hover: a lift a little smaller than the Rivens page's. Easy cards lift too, while their XP
     shows on the rank bar. */
  .card.lift:hover,
  .card.clickable:focus-visible {
    border-color: var(--line-strong);
    background: var(--surface-hover);
    box-shadow: 0 8px 18px rgba(0, 0, 0, 0.45);
    transform: translateY(-3px) scale(1.015);
    z-index: 1;
  }

  .card.clickable:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .card.clickable:active {
    transform: translateY(-1px) scale(1.005);
  }

  /* The set's popup: relics to open and prices to buy, per missing part. */
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: grid;
    place-items: center;
    padding: 24px;
    background: rgba(0, 0, 0, 0.6);
  }

  .dialog {
    width: min(560px, 100%);
    max-height: 100%;
    overflow-y: auto;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    padding: 18px 20px;
    box-shadow: var(--shadow);
    animation: dialog-in 180ms cubic-bezier(0.23, 1, 0.32, 1);
  }

  @keyframes dialog-in {
    from {
      opacity: 0;
      transform: scale(0.97);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .dialog {
      animation: none;
    }
  }

  .dialog-head {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 14px;
  }

  .dialog-head h2 {
    margin: 0;
    font-size: 18px;
  }

  .dialog-head p {
    margin: 2px 0 0;
    font-size: 13px;
  }

  .dialog-pic {
    width: 64px;
    height: 52px;
    object-fit: contain;
  }

  .close {
    margin-left: auto;
    align-self: flex-start;
    width: 28px;
    height: 28px;
    display: grid;
    place-items: center;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--muted);
    cursor: pointer;
  }

  .close:hover {
    background: var(--surface-2);
    color: var(--text);
  }

  .close svg {
    width: 12px;
    height: 12px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
  }

  .needs {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 10px;
  }

  .needs > li {
    background: var(--surface-2);
    border: 1px solid var(--line);
    border-radius: 8px;
    padding: 10px 12px;
  }

  .need-head {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .need-icon {
    position: relative;
    flex: none;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    background: var(--surface);
    border: 2px solid var(--line-strong);
    display: grid;
    place-items: center;
  }

  .need-icon img {
    width: 30px;
    height: 30px;
    object-fit: contain;
  }

  .need-icon img.overlay {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 24px;
    height: 24px;
    transform: translate(-50%, -50%);
  }

  .need-name {
    flex: 1;
    min-width: 0;
    font-weight: 600;
  }

  /* With platinum: what to buy, the missing parts or the whole set when none is owned. */
  .needs > li.pick {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .pick-tag {
    margin-left: 8px;
    padding: 1px 7px;
    border-radius: 999px;
    background: var(--accent);
    color: white;
    font-size: 11px;
    font-weight: 600;
    vertical-align: 1px;
  }

  .part-count {
    display: block;
    font-size: 12px;
    font-weight: 400;
  }

  .have {
    color: var(--good);
  }

  .dialog-sub {
    margin: 16px 0 8px;
    font-size: 13px;
    font-weight: 600;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .need-price {
    display: flex;
    align-items: center;
    gap: 4px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .drops {
    list-style: none;
    margin: 8px 0 0 50px;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .drops li {
    display: flex;
    gap: 6px;
    align-items: baseline;
    padding: 3px 10px;
    border-radius: 999px;
    background: var(--surface);
    border: 1px solid var(--line);
    font-size: 13px;
  }

  .relic {
    font-weight: 600;
  }

  .chance {
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }

  .dialog-foot {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    margin-top: 14px;
    font-size: 13px;
  }

  .dialog-foot > span:last-child {
    display: flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
    font-size: 14px;
  }

  .sources {
    font-size: 13px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .price {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 700;
    font-size: 17px;
  }

  .plat-icon {
    width: 20px;
    height: 20px;
    object-fit: contain;
  }

  /* One row: each circle shrinks below its size when needed so a whole set fits the card. */
  .parts {
    list-style: none;
    margin: 0;
    padding: 0 7px 6px 0;
    display: flex;
    flex-wrap: nowrap;
    gap: 6px;
  }

  .parts li {
    position: relative;
    flex: 0 1 38px;
    min-width: 0;
    aspect-ratio: 1;
    border-radius: 50%;
    background: var(--surface);
    border: 2px solid var(--line-strong);
    display: grid;
    place-items: center;
  }

  /* From relics: bigger part icons. */
  .parts.big li {
    flex-basis: 48px;
  }

  .parts li.enough {
    border-color: var(--good);
  }

  .parts img,
  .parts .placeholder {
    width: 72%;
    height: 72%;
    object-fit: contain;
    border-radius: 50%;
  }

  .parts img.overlay {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 56%;
    height: 56%;
    transform: translate(-50%, -50%);
    border-radius: 0;
  }

  .badge {
    position: absolute;
    right: -6px;
    bottom: -6px;
    min-width: 18px;
    padding: 0 4px;
    border-radius: 9px;
    background: #070a10;
    border: 1px solid var(--line-strong);
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

  .total {
    position: sticky;
    bottom: 0;
    margin: 14px 0 0 auto;
    width: fit-content;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 14px;
    border-radius: 8px;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    font-weight: 700;
    font-size: 16px;
  }

  .credit {
    font-size: 12px;
    margin-top: 14px;
  }

  .credit a {
    color: var(--muted);
  }
</style>
