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
  const sum = (list: Progress[]) => ({
    done: list.reduce((a, p) => a + p.done, 0),
    total: list.reduce((a, p) => a + p.total, 0),
  });

  const progressInRank = $derived(data ? data.totalXp - data.rankStart : 0);
  const rankSpan = $derived(data ? data.nextRank - data.rankStart : 1);
  const rankPct = $derived(Math.min(100, Math.max(0, (100 * progressInRank) / rankSpan)));

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

{#snippet platinum()}
  <img class="plat-icon" src="/icons/platinum.png" alt="platinum" />
{/snippet}

{#snippet progressBox(title: string, rows: Progress[])}
  {@const all = sum(rows)}
  {@const segs = segments(rows, all.total)}
  <section class="box">
    <h2>{title}</h2>
    <div class="box-body">
      <!-- Progress ring, one coloured segment per row sized by its share of the box total.
           r = 15.9155 makes the circumference 100, so dash lengths are percents. -->
      <svg class="ring" viewBox="0 0 36 36" role="img" aria-label="{title}: {pct(all)}% done">
        <circle class="ring-track" cx="18" cy="18" r="15.9155" />
        {#each segs as s, i}
          {#if s.length > 0}
            <circle
              class="ring-seg"
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
        <text x="18" y="18">{pct(all)}%</text>
      </svg>
      <dl>
        {#each rows as r, i}
          <dt><span class="swatch" style="background: {segs[i].color}"></span>{r.label}</dt>
          <dd><span class="row-pct">{pct(r)}%</span> {r.done}/{r.total}</dd>
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
  <header class="rank-head">
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
        <span class="xp-pct">{Math.floor(rankPct)}%</span>
        <span>
          {progressInRank.toLocaleString()} / {rankSpan.toLocaleString()} XP
          {#if data.readyForTest}<span class="ready">· Rank test available</span>{/if}
        </span>
      </div>
      <div class="bar"><div class="fill" style="width: {rankPct}%"></div></div>
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
      <p class="muted note">Items you own or are building in the foundry that aren't mastered yet.</p>
    {/if}

    <div class="cards">
      {#each list as s (s.path)}
        {@const c = mode === "platinum" ? cost(s) : null}
        <article class="card">
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
                onclick={() => toggleFavorite(s)}
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
                {#if s.building}<span class="chip building">Building</span>{:else}<span class="chip owned">Owned</span>{/if}
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

            {#if mode !== "easy" && s.parts.length}
              <ul class="parts">
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
  .rank-head {
    display: flex;
    align-items: center;
    gap: 24px;
    margin-bottom: 20px;
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
  }

  .rank-icon {
    width: 64px;
    height: 64px;
    object-fit: contain;
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
    height: 14px;
    border-radius: 7px;
    background: var(--surface-2);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    background: var(--accent);
    border-radius: 7px;
  }

  .breakdown {
    margin-top: 6px;
    font-size: 12px;
    color: var(--muted);
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

  .ring {
    flex: none;
    width: 80px;
    height: 80px;
    transform: rotate(-90deg);
  }

  .ring circle {
    fill: none;
    stroke-width: 3.2;
  }

  .ring-track {
    stroke: var(--surface-2);
  }

  .ring-seg {
    stroke-linecap: butt;
  }

  .swatch {
    display: inline-block;
    width: 9px;
    height: 9px;
    border-radius: 2px;
    margin-right: 7px;
    vertical-align: 1px;
  }

  /* Counter-rotated so the percent reads upright. */
  .ring text {
    transform: rotate(90deg);
    transform-origin: 18px 18px;
    text-anchor: middle;
    dominant-baseline: central;
    font-size: 8px;
    font-weight: 700;
    fill: var(--text);
  }

  .box dl {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 6px 14px;
    margin: 0;
    font-size: 15px;
  }

  .box dt {
    text-align: right;
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
    background: var(--surface);
    border: 2px solid var(--line-strong);
    display: grid;
    place-items: center;
  }

  .parts li.enough {
    border-color: var(--good);
  }

  .parts img,
  .parts .placeholder {
    width: 26px;
    height: 26px;
    object-fit: contain;
    border-radius: 50%;
  }

  .parts img.overlay {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 20px;
    height: 20px;
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
