<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { friendlyError } from "$lib/errors";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { imageUrl, markMissing } from "$lib/format";
  import RivenPopup from "$lib/RivenPopup.svelte";
  import RivenDataStatus from "$lib/RivenDataStatus.svelte";

  type Stat = {
    tag: string;
    text: string;
    good: boolean;
    /** Roll quality 0–1 (for the negative, smaller is better). */
    quality: number;
    letter: string;
    worst: string;
    best: string;
    /** Positives: wanted by the grading sheet. Negative: harmless. Null if the weapon isn't listed. */
    wanted: boolean | null;
    /** Spliced trait whose range isn't known yet: shown, not graded. */
    pending?: boolean;
  };
  type Riven = {
    id: string;
    image: string | null;
    kind: string;
    compat: string;
    weapon: string;
    name: string;
    rank: number;
    masteryReq: number;
    rerolls: number;
    polarity: string;
    disposition: number;
    dots: number;
    stats: Stat[];
    grade: "Great" | "Good" | "OK" | "Bad" | null;
    /** Has a spliced trait: no grade yet ("collecting data"). */
    gradePending: boolean;
    perfectness: number;
  };
  type Unveiling = { kind: string; challenge: string; progress: number; required: number };
  type Veiled = { kind: string; count: number };
  type Page = { rivens: Riven[]; unveiling: Unveiling[]; veiled: Veiled[] };

  const MAX_RANK = 8;

  let data = $state<Page | null>(null);
  let error = $state<string | null>(null);
  let tab = $state("All");
  let search = $state("");
  let sort = $state<"weapon" | "grade" | "perfectness" | "rerolls" | "disposition">("weapon");
  let gradeFilter = $state("All");
  /** The riven whose popup is open. */
  let selected = $state<Riven | null>(null);

  const GRADES = ["Great", "Good", "OK", "Bad"] as const;
  const gradeRank = (g: Riven["grade"]) => (g ? GRADES.indexOf(g) : GRADES.length);

  const load = () =>
    invoke<Page>("get_rivens")
      .then((p) => {
        data = p;
        error = null;
      })
      .catch((e) => (error = friendlyError(e, "the app")));

  onMount(() => {
    load();
    const off = listen("inventory-updated", load);
    const offGrades = listen("riven-grades-updated", load);
    // New riven data (dispositions, stat values) changes the numbers: reload once it's in.
    const offData = listen<{ state: string }>("riven-data", (e) => e.payload.state === "ready" && load());
    return () => {
      off.then((f) => f());
      offGrades.then((f) => f());
      offData.then((f) => f());
    };
  });

  /** Tabs: All plus each riven type the account has, in a fixed order. */
  const ORDER = ["Rifle", "Shotgun", "Pistol", "Melee", "Zaw", "Kitgun", "Archgun", "Companion Weapon"];
  const tabs = $derived.by(() => {
    const counts: Record<string, number> = {};
    for (const r of data?.rivens ?? []) counts[r.kind] = (counts[r.kind] ?? 0) + 1;
    const pos = (k: string) => (ORDER.includes(k) ? ORDER.indexOf(k) : ORDER.length);
    const kinds = Object.keys(counts).sort((a, b) => pos(a) - pos(b) || a.localeCompare(b));
    return [{ id: "All", count: data?.rivens.length ?? 0 }, ...kinds.map((k) => ({ id: k, count: counts[k] }))];
  });

  const visible = $derived.by(() => {
    const q = search.trim().toLowerCase();
    const list = (data?.rivens ?? []).filter(
      (r) =>
        (tab === "All" || r.kind === tab) &&
        (gradeFilter === "All" || gradeLabel(r) === gradeFilter) &&
        (!q ||
          r.weapon.toLowerCase().includes(q) ||
          r.name.toLowerCase().includes(q) ||
          r.stats.some((s) => s.text.toLowerCase().includes(q))),
    );
    if (sort === "grade")
      return [...list].sort((a, b) => gradeRank(a.grade) - gradeRank(b.grade) || b.perfectness - a.perfectness);
    if (sort === "perfectness") return [...list].sort((a, b) => b.perfectness - a.perfectness);
    if (sort === "rerolls") return [...list].sort((a, b) => b.rerolls - a.rerolls);
    if (sort === "disposition") return [...list].sort((a, b) => b.disposition - a.disposition);
    return list;
  });

  const gradeLabel = (r: Riven) => r.grade ?? (r.gradePending ? "Collecting data" : "Not rated");

  function gradeHelp(r: Riven) {
    if (r.gradePending) return "Has a spliced trait: the grading sheet doesn't cover those yet (collecting data)";
    switch (r.grade) {
      case "Great":
        return "Great: every positive is wanted for this weapon and the negative is harmless";
      case "Good":
        return "Good: the stats a build needs, a harmless negative; one positive is dead weight";
      case "OK":
        return "OK: some wanted stats, but a required one is missing or the negative hurts";
      case "Bad":
        return "Bad: no wanted stats, or one wanted stat with a harmful negative";
      default:
        return "This weapon isn't in the grading sheet";
    }
  }

  function statHelp(s: Stat) {
    if (s.pending) return "Spliced trait: its roll range isn't known yet, so it isn't graded (collecting data)";
    const role = s.good
      ? s.wanted === true
        ? "Wanted for this weapon"
        : s.wanted === false
          ? "Not a wanted stat for this weapon"
          : ""
      : s.wanted === true
        ? "Harmless negative for this weapon"
        : s.wanted === false
          ? "This negative can hurt builds"
          : "";
    return [`Roll ${s.letter} (${Math.round(s.quality * 100)}%)`, `Range: ${s.worst} (worst) to ${s.best} (best)`, role]
      .filter(Boolean)
      .join("\n");
  }

  /** S / A / B / C / F colour band. */
  const letterClass = (l: string) => l[0].toLowerCase();
</script>

<div class="toolbar">
  <div class="tabs" role="tablist">
    {#each tabs as t}
      <button role="tab" aria-selected={tab === t.id} class:active={tab === t.id} onclick={() => (tab = t.id)}>
        {t.id}<span class="count">{t.count}</span>
      </button>
    {/each}
  </div>
  <div class="tools">
    <input type="search" placeholder="Weapon, name or stat" bind:value={search} aria-label="Search rivens" />
    <select bind:value={gradeFilter} aria-label="Grade">
      <option value="All">Grade: All</option>
      {#each GRADES as g}<option value={g}>Grade: {g}</option>{/each}
      <option value="Collecting data">Grade: Collecting data</option>
      <option value="Not rated">Grade: Not rated</option>
    </select>
    <select bind:value={sort} aria-label="Sort">
      <option value="weapon">Sort: Weapon</option>
      <option value="grade">Sort: Grade</option>
      <option value="perfectness">Sort: Stat perfectness</option>
      <option value="rerolls">Sort: Rerolls</option>
      <option value="disposition">Sort: Disposition</option>
    </select>
  </div>
</div>

{#if data && (data.veiled.length || data.unveiling.length)}
  <div class="veiled">
    <span class="veiled-label">Veiled</span>
    {#each data.veiled as v}
      <span class="chip">{v.kind}<b>×{v.count}</b></span>
    {/each}
    {#each data.unveiling as u}
      <span class="chip unveiling" title="Unveiling challenge">{u.kind}: {u.challenge}<b>{u.progress}/{u.required}</b></span>
    {/each}
  </div>
{/if}

<RivenDataStatus compact />

{#if !data}
  <p class="muted">{error ? `Couldn't load rivens. ${error}` : "Loading…"}</p>
{:else if !data.rivens.length}
  <p class="muted">No unveiled rivens in the last capture.</p>
{:else}
  <p class="muted note">
    Stats at max rank ({MAX_RANK}). Green is a positive, red the negative; bold stats are ones the grading sheet wants
    for that weapon. The letter is how well each stat rolled (S best, F worst); hover a stat for its range.
  </p>
  <div class="grid">
    {#each visible as r (r.id)}
      <div
        class="riven"
        role="button"
        tabindex="0"
        title="Click for stat ranges and similar rivens on warframe.market"
        onclick={() => (selected = r)}
        onkeydown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), (selected = r))}
      >
        <div class="top">
          <span class="grade grade-{r.gradePending ? 'pending' : (r.grade ?? 'none').toLowerCase()}" title={gradeHelp(r)}>
            {gradeLabel(r)}
          </span>
          <span class="perfect" title="Stat perfectness: average roll quality">{Math.round(r.perfectness)}%</span>
          <span class="mr" title="Mastery rank required">MR {r.masteryReq}</span>
          {#if r.polarity}<span class="pol">{r.polarity}</span>{/if}
          <span class="rerolls" title="Times rerolled">⟳ {r.rerolls}</span>
        </div>
        <div class="art">
          {#if r.image}
            <img src={imageUrl(r.image)} alt="" loading="lazy" onerror={markMissing} />
          {/if}
        </div>
        <h3 class="weapon">{r.weapon}</h3>
        <div class="name">{r.name}</div>
        <ul class="stats">
          {#each r.stats as s}
            <li
              class:good={s.good}
              class:bad={!s.good}
              class:wanted={s.good && s.wanted === true}
              class:dead={s.good && s.wanted === false && !s.pending}
              title={statHelp(s)}
            >
              <span class="stat-text">
                {s.text}
                {#if !s.good && s.wanted === true}<span class="harmless">harmless</span>{/if}
              </span>
              {#if s.pending}
                <span class="collecting">collecting data</span>
              {:else}
                <span class="letter letter-{letterClass(s.letter)}">{s.letter}</span>
              {/if}
            </li>
          {/each}
        </ul>
        <div class="bottom">
          <span class="dots" title="Disposition {r.disposition.toFixed(2)}">
            {#each Array(5) as _, i}<span class:on={i < r.dots}></span>{/each}
          </span>
          <span class="kind">{r.kind}</span>
          <span class="pips" title="Rank {r.rank} of {MAX_RANK}">
            {#each Array(MAX_RANK) as _, i}<span class:on={i < r.rank}></span>{/each}
          </span>
        </div>
      </div>
    {:else}
      <p class="muted">No rivens match.</p>
    {/each}
  </div>
  <p class="muted credit">
    Grades: "good riven rolls" sheet by 44bananas and Xennethkeisere (refreshed every 2 weeks) · Stats and roll
    letters: Warframe Public Export Plus and warframe-riven-info by calamity-inc
  </p>
{/if}

{#if selected}
  <RivenPopup riven={selected} onclose={() => (selected = null)} />
{/if}

<style>
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 14px;
  }

  .tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .tabs button,
  select {
    font: inherit;
    color: var(--text);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 6px 12px;
    cursor: pointer;
  }

  .tabs button:hover {
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
    padding: 6px 10px;
    width: 220px;
  }

  .muted {
    color: var(--muted);
  }

  .note {
    font-size: 13px;
    margin: 0 0 12px;
  }

  .credit {
    font-size: 12px;
    margin-top: 16px;
  }

  /* Veiled counts and unveiling challenges */
  .veiled {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-bottom: 14px;
  }

  .veiled-label {
    font-size: 13px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--muted);
    margin-right: 4px;
  }

  .chip {
    display: flex;
    gap: 6px;
    align-items: baseline;
    padding: 3px 10px;
    border-radius: 6px;
    background: var(--surface);
    border: 1px solid var(--line);
    font-size: 13px;
  }

  .chip.unveiling {
    border-color: #6d4a99;
  }

  /* Cards: up to 5 across, like Inventory */
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(max(250px, calc((100% - 4 * 14px) / 5)), 1fr));
    gap: 14px;
  }

  .riven {
    --purple: #a77be8;
    position: relative;
    cursor: pointer;
    transition:
      background-color 0.15s,
      border-color 0.15s,
      box-shadow 0.15s,
      transform 0.15s;
    background: #24123a;
    border: 2px solid var(--purple);
    border-radius: 10px;
    padding: 10px 14px 12px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    text-align: center;
  }

  /* Hover: lift, lighter purple, bright border. */
  .riven:hover,
  .riven:focus-visible {
    background: #331a55;
    border-color: #e2ccff;
    box-shadow: 0 12px 24px rgba(0, 0, 0, 0.55);
    transform: translateY(-5px) scale(1.02);
    outline: none;
    z-index: 1;
  }

  .riven:active {
    transform: translateY(-2px) scale(1.01);
  }

  .top {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }

  .mr {
    font-weight: 700;
    padding: 1px 7px;
    border-radius: 4px;
    background: #3a2160;
  }

  .pol {
    color: #c9b3ec;
  }

  .rerolls {
    margin-left: auto;
    color: #c9b3ec;
    font-variant-numeric: tabular-nums;
  }

  .art {
    height: 96px;
    width: 100%;
    display: grid;
    place-items: center;
  }

  .art img {
    max-height: 96px;
    max-width: 100%;
    object-fit: contain;
  }

  .weapon {
    margin: 0;
    font-size: 19px;
    font-weight: 700;
    color: #f1e8ff;
  }

  .name {
    margin-top: -4px;
    font-size: 15px;
    font-weight: 600;
    color: var(--purple);
  }

  .stats {
    list-style: none;
    margin: 4px 0 2px;
    padding: 0;
    font-size: 14px;
    line-height: 1.45;
  }

  .stats {
    width: 100%;
    text-align: left;
  }

  .stats li {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    padding: 1px 0;
    cursor: default;
  }

  .stats .good {
    color: #6fdc8c;
  }

  .stats .bad {
    color: #ff7b86;
  }

  /* Stats the sheet wants are bold; positives it doesn't want are dimmed. */
  .stats .wanted .stat-text {
    font-weight: 700;
  }

  .stats .dead .stat-text {
    opacity: 0.6;
  }

  .harmless {
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

  /* Roll letter */
  .letter {
    flex: none;
    min-width: 26px;
    text-align: center;
    font-size: 12px;
    font-weight: 800;
    border-radius: 4px;
    padding: 0 4px;
    color: #0b0f17;
  }

  .letter-s { background: #f2c14e; }
  .letter-a { background: #6fcf7f; }
  .letter-b { background: #5aa9f0; }
  .letter-c { background: #e8875b; }
  .letter-f { background: #f87171; }

  /* Riven grade (from the sheet) */
  .grade {
    font-weight: 800;
    padding: 1px 8px;
    border-radius: 4px;
    color: #0b0f17;
  }

  .grade-great { background: #f2c14e; }
  .grade-good { background: #6fcf7f; }
  .grade-ok { background: #c9b3ec; }
  .grade-bad { background: #f87171; }
  .grade-none,
  .grade-pending {
    background: transparent;
    color: #c9b3ec;
    border: 1px solid #6d4a99;
  }

  .grade-pending {
    border-style: dashed;
  }

  /* A spliced trait without a known range. */
  .collecting {
    flex: none;
    font-size: 11px;
    font-style: italic;
    color: #c9b3ec;
    white-space: nowrap;
  }

  .perfect {
    color: #c9b3ec;
    font-variant-numeric: tabular-nums;
  }

  .bottom {
    margin-top: auto;
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding-top: 6px;
    border-top: 1px solid #3d2563;
    font-size: 11px;
  }

  .kind {
    text-transform: uppercase;
    letter-spacing: 0.6px;
    font-weight: 700;
    color: #c9b3ec;
  }

  .dots,
  .pips {
    display: flex;
    gap: 3px;
  }

  .dots span,
  .pips span {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    border: 1px solid var(--purple);
  }

  .dots span.on,
  .pips span.on {
    background: var(--purple);
  }
</style>
