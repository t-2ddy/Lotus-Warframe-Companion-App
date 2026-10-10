<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { friendlyError } from "$lib/errors";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { dateTime } from "$lib/format";
  import Fissures from "$lib/Fissures.svelte";
  import CurrencyCharts from "$lib/CurrencyCharts.svelte";
  import Checklist from "$lib/Checklist.svelte";

  type Overview = {
    account: string | null;
    accountName: string | null;
    masteryRank: number | null;
    lastStoredAt: number | null;
    currencies: { label: string; value: number }[];
  };

  let overview = $state<Overview | null>(null);

  const load = () =>
    invoke<Overview>("get_overview")
      .then((o) => (overview = o))
      .catch(() => {});

  // --- Arbitrations: today plus the next 2 days, in this PC's local time ---

  type Arb = {
    startMs: number;
    endMs: number;
    tag: string;
    /** S, A, B, C, D or F (Arbitration Goons). */
    tier: string;
    node: string;
    planet: string;
    missionType: string;
    enemy: string;
  };

  const DAYS = 3;
  const TIERS = ["S", "A", "B", "C", "D", "F"];
  const TIER_KEY = "arbTiers";

  /** Tiers shown in the list; remembered on this PC. All tiers when nothing is saved. */
  let shownTiers = $state<string[]>(
    (() => {
      try {
        const saved = JSON.parse(localStorage.getItem(TIER_KEY) ?? "null");
        if (Array.isArray(saved)) return TIERS.filter((t) => saved.includes(t));
      } catch {}
      return [...TIERS];
    })(),
  );

  function toggleTier(t: string) {
    shownTiers = shownTiers.includes(t) ? shownTiers.filter((x) => x !== t) : TIERS.filter((x) => x === t || shownTiers.includes(x));
    try {
      localStorage.setItem(TIER_KEY, JSON.stringify(shownTiers));
    } catch {}
  }

  function allTiers() {
    shownTiers = shownTiers.length === TIERS.length ? [] : [...TIERS];
    try {
      localStorage.setItem(TIER_KEY, JSON.stringify(shownTiers));
    } catch {}
  }
  let arbs = $state<Arb[]>([]);
  let now = $state(Date.now());
  let listEl = $state<HTMLElement | null>(null);
  let scrolledToNow = false;

  /** Local midnight at the start of the day containing `ms`. */
  const dayStart = (ms: number) => {
    const d = new Date(ms);
    return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  };
  /** Local midnight `n` days after the day containing `ms` (handles daylight-saving changes). */
  const addDays = (ms: number, n: number) => {
    const d = new Date(ms);
    return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n).getTime();
  };

  let arbError = $state<string | null>(null);

  async function loadArbs() {
    const from = dayStart(Date.now());
    try {
      arbs = await invoke<Arb[]>("get_arbitrations", { fromMs: from, toMs: addDays(from, DAYS) });
      arbError = null;
    } catch (e) {
      arbs = [];
      arbError = friendlyError(e, "browse.wf");
    }
  }

  /** Arbitrations grouped by local day: Today, Tomorrow, then the weekday and date. */
  const days = $derived.by(() => {
    const today = dayStart(now);
    const groups: { label: string; date: string | null; items: Arb[] }[] = [];
    // Finished arbitrations are left out; `now` ticks every 30 s, so each drops off when it ends.
    // Tiers switched off in the filter are left out too.
    for (const a of arbs.filter((x) => x.endMs > now && shownTiers.includes(x.tier))) {
      const day = dayStart(a.startMs);
      const offset = Math.round((day - today) / 86_400_000);
      const label =
        offset === 0
          ? "Today"
          : offset === 1
            ? "Tomorrow"
            : offset === -1
              ? "Yesterday"
              : new Date(day).toLocaleDateString(undefined, { weekday: "long", month: "short", day: "numeric" });
      // Today / Tomorrow / Yesterday also show their date; other days already include it.
      const date = Math.abs(offset) <= 1
        ? new Date(day).toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" })
        : null;
      const last = groups.at(-1);
      if (last && last.label === label) last.items.push(a);
      else groups.push({ label, date, items: [a] });
    }
    return groups;
  });

  const hour = (ms: number) => new Date(ms).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  /** Without minutes ("12 AM"): arbitrations run on the hour; used when the list is narrow. */
  const hourShort = (ms: number) => new Date(ms).toLocaleTimeString(undefined, { hour: "numeric" });

  /** Scroll the list (not the page) so the arbitration running now is at the top. */
  $effect(() => {
    if (!listEl || scrolledToNow || !arbs.length) return;
    const current = listEl.querySelector<HTMLElement>(".arb.now");
    if (current) {
      listEl.scrollTop = current.offsetTop - listEl.offsetTop - 40;
      scrolledToNow = true;
    }
  });

  onMount(() => {
    load();
    loadArbs();
    const off = listen("inventory-updated", load);
    const offArbs = listen("arbitrations-updated", loadArbs);

    // Keep "now" current so the running arbitration stays highlighted.
    const tick = setInterval(() => (now = Date.now()), 30_000);

    // At the end of each local day, move the 3-day window forward.
    let midnight: ReturnType<typeof setTimeout>;
    const scheduleMidnight = () => {
      const wait = addDays(Date.now(), 1) - Date.now() + 1000;
      midnight = setTimeout(() => {
        now = Date.now();
        scrolledToNow = false;
        loadArbs();
        scheduleMidnight();
      }, wait);
    };
    scheduleMidnight();

    return () => {
      off.then((f) => f());
      offArbs.then((f) => f());
      clearInterval(tick);
      clearTimeout(midnight);
    };
  });

  /** The three currencies shown on Home, in order, with their icons. */
  const TILES = [
    { label: "Platinum", icon: "/icons/platinum.png" },
    { label: "Credits", icon: "/icons/credits.png" },
    { label: "Endo", icon: "/icons/endo.png" },
  ];

  /** Mastery rank as the game shows it: 0–30, then L1, L2, … for Legendary ranks. */
  const rankLabel = (r: number) => (r > 30 ? `MR L${r - 30}` : `MR ${r}`);

  const value = (label: string) => overview?.currencies.find((c) => c.label === label)?.value;
</script>

<div class="home">
  {#if !overview}
    <p class="muted">Loading…</p>
  {:else if !overview.account}
    <h1>Welcome</h1>
    <p class="muted">No inventory captured yet. Log in or finish a mission with the app open.</p>
  {:else}
    <header>
      <div class="top-row">
        <h1 class="name">
          {#if overview.masteryRank != null}
            {@const r = overview.masteryRank}
            <img
              class="rank-icon"
              src="/icons/ranks/rank{Math.min(Math.max(r, 0), 51)}.png"
              alt=""
              title="Mastery Rank {rankLabel(r)}"
            />
          {/if}
          {overview.accountName ?? "Your account"}
          {#if overview.masteryRank != null}
            <span class="rank-num" title="Mastery Rank">{rankLabel(overview.masteryRank)}</span>
          {/if}
        </h1>
        <ul class="currencies">
          {#each TILES as t}
            {@const v = value(t.label)}
            <li title={t.label}>
              <img src={t.icon} alt={t.label} />
              <span class="value">{v === undefined ? "–" : v.toLocaleString()}</span>
            </li>
          {/each}
        </ul>
      </div>
      {#if overview.lastStoredAt}
        <p class="muted">Updated {dateTime(overview.lastStoredAt)}</p>
      {/if}
      {#if !overview.accountName}
        <p class="muted small">Your in-game name appears after your next login with the app open.</p>
      {/if}
    </header>
  {/if}

  <Checklist />

  <div class="boards">
  <div class="boards-row">
  <section class="arbitrations">
    <div class="arb-head">
      <h2><img class="arb-icon" src="/icons/vitus-essence.png" alt="" />Arbitrations</h2>
      <span class="muted small">Today and the next 2 days · your local time</span>
      <div class="tier-filter" role="group" aria-label="Filter by tier">
        <button class="tier-all" class:on={shownTiers.length === TIERS.length} onclick={allTiers}>All</button>
        {#each TIERS as t}
          <button
            class="tier tier-{t.toLowerCase()}"
            class:off={!shownTiers.includes(t)}
            aria-pressed={shownTiers.includes(t)}
            title="{shownTiers.includes(t) ? 'Hide' : 'Show'} {t} tier"
            onclick={() => toggleTier(t)}>{t}</button
          >
        {/each}
      </div>
    </div>
    {#if !arbs.length}
      <p class="muted">{arbError ? `Couldn't load the arbitration schedule. ${arbError}` : "Loading the arbitration schedule…"}</p>
    {:else}
      <div class="arb-list" bind:this={listEl}>
        {#if !days.length}
          <p class="muted empty">No upcoming arbitrations in the selected tiers.</p>
        {/if}
        {#each days as day}
          <h3 class="day">{day.label}{#if day.date}<span class="day-date">{day.date}</span>{/if}</h3>
          <ul>
            {#each day.items as a (a.startMs)}
              {@const isNow = now >= a.startMs && now < a.endMs}
              <li class="arb" class:now={isNow}>
                <span class="tier tier-{a.tier.toLowerCase()}" title="{a.tier} tier">{a.tier}</span>
                <span class="time"><span class="full">{hour(a.startMs)} – {hour(a.endMs)}</span><span class="short">{hourShort(a.startMs)} – {hourShort(a.endMs)}</span></span>
                <span class="where">
                  <span class="node">{a.node}</span>
                  {#if a.planet}<span class="planet">{a.planet}</span>{/if}
                </span>
                <span class="mission">{a.missionType || "–"}</span>
                <span class="enemy">{a.enemy}</span>
                {#if isNow}<span class="now-chip">Now</span>{/if}
              </li>
            {/each}
          </ul>
        {/each}
      </div>
      <p class="credit muted">
        Schedule: <a href="https://browse.wf/arbys" target="_blank" rel="noreferrer">browse.wf</a>
        · Tiers: Arbitration Goons
      </p>
    {/if}
  </section>

  <Fissures />
  </div>
  </div>

  <CurrencyCharts />
</div>

<style>
  /* Uses the full width, so in full screen the two lists split the screen evenly. */
  .home {
    width: 100%;
  }

  /* Arbitrations and fissures: stacked, or side by side when there's room for both (full screen). */
  .boards {
    container-type: inline-size;
  }

  @container (min-width: 960px) {
    .boards-row {
      display: flex;
      align-items: flex-start;
      gap: 24px;
    }

    .boards-row > :global(section) {
      flex: 1 1 0;
      min-width: 0;
    }

    /* Columns scale with the half-width lists; text that doesn't fit ends in "…". */
    .boards-row :global(.arb) {
      grid-template-columns: 30px 150px minmax(0, 1.6fr) minmax(0, 1fr) minmax(0, 1fr) 44px;
    }

    .boards-row :global(.fissures .row) {
      grid-template-columns: minmax(0, 1.6fr) minmax(0, 1fr) minmax(0, 1fr) 70px;
    }

    /* Headings the same height, so the two lists start level. */
    .boards-row :global(.arb-head),
    .boards-row :global(.fissures .head) {
      height: 34px;
      align-items: center;
    }

    .boards-row :global(.tier-filter) {
      flex: none;
    }

    .boards-row :global(.arb-head .small) {
      min-width: 0;
      margin-top: 0;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }

    /* Same height so the two lists line up. */
    .boards-row :global(.arb-list),
    .boards-row :global(.list) {
      height: 560px;
      max-height: none;
    }
  }

  /* Side by side in a smaller window (1280–1350 wide): the subtitle gives way to the tier filter,
     and the time column tightens. */
  @container (min-width: 960px) and (max-width: 1250px) {
    .boards-row :global(.arb-head .small) {
      display: none;
    }

    .boards-row :global(.arb) {
      grid-template-columns: 30px 100px minmax(0, 1.9fr) minmax(0, 0.9fr) minmax(0, 0.75fr) 44px;
    }

    .boards-row :global(.arb .time .full) {
      display: none;
    }

    .boards-row :global(.arb .time .short) {
      display: inline;
    }
  }

  .arb .time .short {
    display: none;
  }

  header {
    margin-bottom: 24px;
  }

  h1 {
    font-size: 34px;
    margin: 0 0 6px;
  }

  .muted {
    color: var(--muted);
    margin: 0;
  }

  .small {
    font-size: 13px;
    margin-top: 4px;
  }

  /* Name and the three currencies on one line, no panels behind them. */
  /* Name and the three currencies always on one line (never wrap under the name). */
  .top-row {
    display: flex;
    flex-wrap: nowrap;
    align-items: center;
    gap: 28px;
    margin-bottom: 6px;
  }

  .top-row h1 {
    margin: 0;
    flex: none;
    white-space: nowrap;
  }

  .name {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .rank-icon {
    width: 44px;
    height: 44px;
    object-fit: contain;
  }

  .rank-num {
    font-size: 18px;
    font-weight: 700;
    color: var(--muted);
    border: 1px solid var(--line-strong);
    border-radius: 6px;
    padding: 2px 9px;
  }

  .currencies {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: nowrap;
    align-items: center;
    gap: 22px;
  }

  .currencies li {
    display: flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
  }

  .currencies img {
    width: 30px;
    height: 30px;
    object-fit: contain;
  }

  .value {
    font-size: 20px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }

  /* Arbitrations: a scrollable 3-day list */
  .arbitrations {
    margin-top: 28px;
  }

  .arb-head {
    display: flex;
    align-items: baseline;
    gap: 14px;
    margin-bottom: 10px;
  }

  .tier-filter {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 6px;
    align-self: center;
  }

  .tier-filter button {
    cursor: pointer;
    font: inherit;
    border: none;
  }

  /* Off tiers are dimmed but keep their colour. */
  .tier-filter .tier.off {
    opacity: 0.3;
  }

  .tier-all {
    height: 24px;
    padding: 0 10px;
    border-radius: 4px;
    font-size: 13px;
    font-weight: 700;
    color: var(--muted);
    background: transparent;
    outline: 1px solid var(--line);
  }

  .tier-all.on {
    color: var(--text, white);
    background: var(--surface-2);
  }

  .empty {
    padding: 16px;
  }

  .arb-head h2 {
    margin: 0;
    font-size: 20px;
  }

  .arb-head h2 {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .arb-icon {
    width: 28px;
    height: 28px;
    object-fit: contain;
  }

  .arb-list {
    max-height: 460px;
    overflow-y: auto;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 4px 0;
  }

  .day {
    position: sticky;
    top: -4px;
    z-index: 1;
    margin: 0;
    padding: 11px 16px;
    font-size: 16px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.8px;
    color: var(--text);
    background: var(--surface-2);
    border-top: 1px solid var(--line-strong);
    border-bottom: 1px solid var(--line-strong);
    box-shadow: inset 3px 0 0 var(--accent);
  }

  .day-date {
    margin-left: 10px;
    font-size: 14px;
    color: var(--muted);
    text-transform: none;
    letter-spacing: 0;
    font-weight: 500;
  }

  .arb-list ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .arb {
    display: grid;
    grid-template-columns: 30px 170px minmax(0, 1fr) 130px 150px 50px;
    align-items: center;
    gap: 12px;
    padding: 9px 16px;
    border-bottom: 1px solid var(--line);
    font-size: 15px;
  }

  .arb.now {
    background: var(--surface-2);
    box-shadow: inset 3px 0 0 var(--accent);
  }

  /* Tier badge: flat colour per tier, best to worst. */
  .tier {
    width: 26px;
    height: 24px;
    display: grid;
    place-items: center;
    border-radius: 4px;
    font-size: 13px;
    font-weight: 800;
    color: #0b0f17;
    background: var(--surface-2);
  }

  .tier-s { background: #f2c14e; }
  .tier-a { background: #6fcf7f; }
  .tier-b { background: #5aa9f0; }
  .tier-c { background: #a78bfa; }
  .tier-d { background: #e8875b; }
  /* F: solid grey, so it reads as a tier like the others (the filter buttons have no border, so an
     outline alone left it invisible there). */
  .tier-f { background: #4b5563; color: #e5e7eb; }

  .time {
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .where {
    display: flex;
    gap: 8px;
    align-items: baseline;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .node {
    font-weight: 600;
  }

  .planet,
  .enemy {
    color: var(--muted);
  }

  .mission,
  .enemy {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .now-chip {
    font-size: 12px;
    font-weight: 700;
    color: white;
    background: var(--accent);
    border-radius: 4px;
    padding: 1px 7px;
    justify-self: end;
  }

  .credit {
    font-size: 12px;
    margin-top: 8px;
  }

  .credit a {
    color: var(--muted);
  }
</style>
