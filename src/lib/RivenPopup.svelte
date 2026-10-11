<script lang="ts" module>
  import type { Best } from "$lib/BestStats.svelte";

  export type RivenStat = {
    tag: string;
    text: string;
    good: boolean;
    quality: number;
    letter: string;
    worst: string;
    best: string;
    wanted: boolean | null;
    /** Spliced trait whose range isn't known yet: shown, not graded. */
    pending?: boolean;
    /** A stat this riven type can't roll any more (DE removed it): shown, not graded. */
    legacy?: boolean;
    /** Its number couldn't be read off the card: shown with the range it's in, not graded. */
    unread?: boolean;
  };

  export type RivenInfo = {
    id: string;
    compat: string;
    weapon: string;
    name: string;
    kind: string;
    rank: number;
    masteryReq: number;
    rerolls: number;
    polarity: string;
    disposition: number;
    dots: number;
    stats: RivenStat[];
    grade: "Great" | "Good" | "OK" | "Bad" | null;
    /** Has a spliced trait: no grade yet ("collecting data"). */
    gradePending?: boolean;
    perfectness: number;
    /** The weapon's best stats from the grading sheet; null if the weapon isn't listed. */
    best?: Best | null;
  };
</script>

<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { friendlyError } from "$lib/errors";
  import { onMount } from "svelte";
  import BestStats from "$lib/BestStats.svelte";

  type ListingStat = { text: string; positive: boolean; shared: boolean };
  type Listing = {
    id: string;
    name: string;
    price: number;
    direct: boolean;
    topBid: number | null;
    stats: ListingStat[];
    rerolls: number;
    rank: number;
    mastery: number;
    polarity: string;
    seller: string;
    status: "ingame" | "online" | "offline" | string;
    updated: string;
    url: string;
  };
  type Similar = {
    level: string;
    total: number;
    lowest: number | null;
    median: number | null;
    listings: Listing[];
    webUrl: string;
  };

  let { riven, onclose }: { riven: RivenInfo; onclose: () => void } = $props();

  let similar = $state<Similar | null>(null);
  let error = $state<string | null>(null);
  let onlineOnly = $state(false);

  const shown = $derived((similar?.listings ?? []).filter((l) => !onlineOnly || l.status !== "offline"));

  onMount(() => {
    const positives = riven.stats.filter((s) => s.good).map((s) => s.tag);
    const negative = riven.stats.find((s) => !s.good)?.tag ?? null;
    const wanted = riven.stats.filter((s) => s.good && s.wanted).map((s) => s.tag);
    invoke<Similar>("similar_rivens", { weapon: riven.compat, weaponName: riven.weapon, positives, negative, wanted })
      .then((r) => (similar = r))
      .catch((e) => (error = friendlyError(e, "warframe.market")));
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }

  const statusLabel = (s: string) => (s === "ingame" ? "In game" : s === "online" ? "Online" : "Offline");
  const open = (url: string) => openUrl(url).catch(() => {});
</script>

<svelte:window onkeydown={onKey} />

<div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="popup" role="dialog" aria-modal="true" aria-label="{riven.weapon} {riven.name}">
    <header>
      <div>
        <h2>{riven.weapon} <span class="rname">{riven.name}</span></h2>
        <div class="meta">
          {#if riven.grade}<span class="grade grade-{riven.grade.toLowerCase()}">{riven.grade}</span>
          {:else if riven.gradePending}<span title="Has a spliced trait: the grading sheet doesn't cover those yet">Grade: collecting data</span>{/if}
          <span>Stat perfectness {Math.round(riven.perfectness)}%</span>
          <span>{riven.kind}</span>
          <span>Disposition {riven.disposition.toFixed(2)} ({riven.dots}/5)</span>
          <span>Rank {riven.rank}/8</span>
          <span>MR {riven.masteryReq}</span>
          <span>{riven.rerolls} rerolls</span>
        </div>
      </div>
      <button class="close" onclick={onclose} title="Close (Esc)" aria-label="Close">
        <svg viewBox="0 0 16 16" width="16" height="16"><path d="M3 3l10 10M13 3L3 13" /></svg>
      </button>
    </header>

    {#if riven.best}
      <section>
        <h3>Best stats for {riven.weapon} <span class="hint">from the community riven sheet</span></h3>
        <BestStats best={riven.best} />
      </section>
    {/if}

    <section>
      <h3>Stat ranges <span class="hint">at max rank, on this weapon</span></h3>
      <table class="ranges">
        <thead>
          <tr><th>Stat</th><th>Worst roll</th><th class="bar-col">Your roll</th><th>Best roll</th><th>Grade</th></tr>
        </thead>
        <tbody>
          {#each riven.stats as s}
            <tr class:bad={!s.good}>
              <td class="stat">
                {s.text}
                {#if s.good && s.wanted === true}<span class="tag want">wanted</span>{/if}
                {#if !s.good && s.wanted === true}<span class="tag">harmless</span>{/if}
              </td>
              {#if s.pending}
                <td colspan="3" class="pending">
                  {s.unread
                    ? `Number not read: somewhere from ${s.worst} to ${s.best}`
                    : s.legacy
                      ? "No longer rolls: DE removed it from this riven type"
                      : "Collecting data: spliced or new trait, range not known yet"}
                </td>
                <td><span class="letter letter-p">?</span></td>
              {:else}
              <td class="end">{s.worst}</td>
              <td class="bar-col">
                <div class="bar" title="{Math.round(s.quality * 100)}% of the way from worst to best">
                  <div class="fill" style="width: {s.quality * 100}%"></div>
                  <div class="mark" style="left: {s.quality * 100}%"></div>
                </div>
              </td>
              <td class="end">{s.best}</td>
              <td><span class="letter letter-{s.letter[0].toLowerCase()}">{s.letter}</span></td>
              {/if}
            </tr>
          {/each}
        </tbody>
      </table>
      <p class="hint">Each stat rolls between 90% and 110% of its base on this riven. For the negative, the best roll is the smallest.</p>
    </section>

    <section>
      <div class="market-head">
        <h3>Similar rivens on warframe.market</h3>
        <label class="toggle"><input type="checkbox" bind:checked={onlineOnly} /> Online sellers only</label>
      </div>
      {#if error}
        <p class="hint">Couldn't load listings. {error}</p>
      {:else if !similar}
        <p class="hint">Searching warframe.market…</p>
      {:else if !similar.total}
        <p class="hint">No listings for this weapon right now.</p>
      {:else}
        <div class="summary">
          <span class="level">{similar.level}</span>
          <span>{similar.total} listed</span>
          {#if similar.lowest !== null}
            <span>Lowest <b>{similar.lowest.toLocaleString()}</b><img src="/icons/platinum.png" alt="platinum" /></span>
          {/if}
          {#if similar.median !== null}
            <span>Median <b>{similar.median.toLocaleString()}</b><img src="/icons/platinum.png" alt="platinum" /></span>
          {/if}
          <button class="link" onclick={() => open(similar!.webUrl)}>Open search on warframe.market ↗</button>
        </div>
        <ul class="listings">
          {#each shown as l (l.id)}
            <li>
              <div class="price">
                <b>{l.price.toLocaleString()}</b><img src="/icons/platinum.png" alt="platinum" />
                {#if !l.direct}
                  <span class="bid" title="Auction: starting price{l.topBid ? `, top bid ${l.topBid}` : ''}">
                    Bid{l.topBid ? ` · ${l.topBid}` : ""}
                  </span>
                {/if}
              </div>
              <div class="body">
                <div class="lname">{l.name}</div>
                <div class="lstats">
                  {#each l.stats as s}
                    <span class:pos={s.positive} class:neg={!s.positive} class:shared={s.shared}>{s.text}</span>
                  {/each}
                </div>
                <div class="lmeta">
                  Rank {l.rank} · {l.rerolls} rerolls · MR {l.mastery}{l.polarity ? ` · ${l.polarity}` : ""}
                </div>
              </div>
              <div class="seller">
                <span class="status status-{l.status}">{statusLabel(l.status)}</span>
                <span class="sname">{l.seller}</span>
                <button class="link" onclick={() => open(l.url)}>View ↗</button>
              </div>
            </li>
          {:else}
            <li class="hint">No online sellers in these results.</li>
          {/each}
        </ul>
        <p class="hint">
          Stats on your riven are in bold. Direct sales first, cheapest first; bid auctions after. Listings are cached for
          10 minutes.
        </p>
      {/if}
    </section>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: rgba(0, 0, 0, 0.65);
    display: grid;
    place-items: center;
    padding: 24px;
  }

  .popup {
    width: min(980px, 100%);
    max-height: 100%;
    overflow-y: auto;
    background: var(--surface);
    border: 2px solid #a77be8;
    border-radius: var(--radius);
    padding: 18px 22px 20px;
  }

  header {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    margin-bottom: 8px;
  }

  h2 {
    margin: 0;
    font-size: 24px;
  }

  .rname {
    color: #a77be8;
  }

  .meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 14px;
    margin-top: 6px;
    font-size: 13px;
    color: var(--muted);
  }

  .close {
    margin-left: auto;
    display: grid;
    place-items: center;
    padding: 6px;
    background: none;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    color: var(--text);
  }

  .close:hover {
    background: var(--surface-hover);
  }

  .close path {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
  }

  section {
    margin-top: 18px;
  }

  h3 {
    margin: 0 0 8px;
    font-size: 16px;
  }

  .hint {
    font-size: 12px;
    font-weight: 400;
    color: var(--muted);
    margin: 6px 0 0;
  }

  /* Stat ranges */
  .ranges {
    width: 100%;
    border-collapse: collapse;
    font-size: 14px;
  }

  .ranges th {
    text-align: left;
    font-size: 12px;
    font-weight: 600;
    color: var(--muted);
    padding: 4px 8px;
    border-bottom: 1px solid var(--line);
  }

  .ranges td {
    padding: 8px;
    border-bottom: 1px solid var(--line);
  }

  .ranges .stat {
    color: #6fdc8c;
    font-weight: 600;
  }

  .ranges tr.bad .stat {
    color: #ff7b86;
  }

  .ranges .end {
    color: var(--muted);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }

  .bar-col {
    width: 26%;
  }

  .bar {
    position: relative;
    height: 8px;
    border-radius: 4px;
    background: var(--surface-2);
    border: 1px solid var(--line);
  }

  .fill {
    height: 100%;
    border-radius: 4px;
    background: #6d4a99;
  }

  .mark {
    position: absolute;
    top: -4px;
    width: 3px;
    height: 14px;
    margin-left: -1.5px;
    border-radius: 2px;
    background: #f1e8ff;
  }

  .tag {
    margin-left: 6px;
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: #c9b3ec;
    border: 1px solid #6d4a99;
    border-radius: 3px;
    padding: 0 4px;
    vertical-align: 1px;
  }

  .tag.want {
    color: #6fdc8c;
    border-color: #2f7d45;
  }

  .letter,
  .grade {
    display: inline-block;
    min-width: 26px;
    text-align: center;
    font-size: 12px;
    font-weight: 800;
    border-radius: 4px;
    padding: 1px 6px;
    color: #0b0f17;
  }

  .letter-s { background: #f2c14e; }
  .letter-a { background: #6fcf7f; }
  .letter-b { background: #5aa9f0; }
  .letter-c { background: #e8875b; }
  .letter-f { background: #f87171; }
  .letter-p { background: #8b8fa3; }
  .pending { font-style: italic; opacity: 0.75; text-align: center; }

  .grade-great { background: #f2c14e; }
  .grade-good { background: #6fcf7f; }
  .grade-ok { background: #c9b3ec; }
  .grade-bad { background: #f87171; }

  /* Market */
  .market-head {
    display: flex;
    align-items: baseline;
    gap: 12px;
  }

  .toggle {
    margin-left: auto;
    font-size: 13px;
    display: flex;
    gap: 6px;
    align-items: center;
    cursor: pointer;
  }

  .summary {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 16px;
    font-size: 14px;
    margin-bottom: 10px;
  }

  .summary img,
  .price img {
    width: 16px;
    height: 16px;
    vertical-align: -3px;
    margin-left: 3px;
  }

  .level {
    font-weight: 700;
    color: #c9b3ec;
  }

  .link {
    font: inherit;
    font-size: 13px;
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
    cursor: pointer;
  }

  .link:hover {
    text-decoration: underline;
  }

  .listings {
    list-style: none;
    margin: 0;
    padding: 0;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    max-height: 420px;
    overflow-y: auto;
  }

  .listings li {
    display: grid;
    grid-template-columns: 110px minmax(0, 1fr) 150px;
    gap: 12px;
    align-items: center;
    padding: 10px 14px;
    border-bottom: 1px solid var(--line);
  }

  .listings li.hint {
    display: block;
  }

  .price {
    font-size: 18px;
    font-variant-numeric: tabular-nums;
  }

  .bid {
    display: block;
    font-size: 11px;
    font-weight: 700;
    color: var(--muted);
  }

  .lname {
    font-weight: 700;
  }

  .lstats {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 12px;
    font-size: 13px;
  }

  .lstats .pos {
    color: #6fdc8c;
  }

  .lstats .neg {
    color: #ff7b86;
  }

  .lstats .shared {
    font-weight: 700;
  }

  .lstats span:not(.shared) {
    opacity: 0.75;
  }

  .lmeta {
    font-size: 12px;
    color: var(--muted);
  }

  .seller {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 2px;
    font-size: 13px;
    min-width: 0;
  }

  .sname {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .status {
    font-size: 11px;
    font-weight: 700;
    border-radius: 4px;
    padding: 0 6px;
  }

  .status-ingame {
    background: #2f7d45;
    color: white;
  }

  .status-online {
    background: var(--accent);
    color: white;
  }

  .status-offline {
    color: var(--muted);
    border: 1px solid var(--line-strong);
  }
</style>
