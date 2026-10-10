<script lang="ts">
  import { onMount } from "svelte";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import X from "@lucide/svelte/icons/x";
  import Plus from "@lucide/svelte/icons/plus";
  import { checklistHidden } from "$lib/checklistPrefs.svelte";

  // Daily and weekly to-dos, from tipsycontent.com's Warframe checklist
  // (https://tipsycontent.com/warframe/checklist). Ticks clear themselves at the game's resets:
  // daily at 00:00 UTC, weekly on Monday at 00:00 UTC. Ticks and which drawers are open are kept
  // in this browser's storage.
  type Task = { id: string; title: string; description: string };
  type Kind = "daily" | "weekly";

  const DAILY: Task[] = [
    { id: "daily-login", title: "Log in", description: "Collect the daily login reward." },
    { id: "daily-crafting-forma", title: "Crafting (Forma)", description: "Start building a new Forma (and collect finished ones)." },
    { id: "daily-crafting-other", title: "Crafting (Other)", description: "Craft daily resources/items using reusable blueprints." },
    { id: "daily-syndicates-standing", title: "Syndicates (Orbiter)", description: "Gain daily standing cap with your pledged Syndicate(s)." },
    {
      id: "daily-openworld-syndicates",
      title: "World Syndicate Standing",
      description: "Gain standing for Cetus, Quills, Fortuna, Vox, Deimos, Cavia, Zariman, Hexis.",
    },
    { id: "daily-focus", title: "Focus", description: "Farm daily Focus cap (Sanctuary Onslaught is recommended)." },
    { id: "daily-steel-path-incursions", title: "Steel Path Incursions", description: "Complete daily Incursions for Steel Essence." },
    { id: "daily-sortie", title: "Sortie", description: "Complete today's Sortie missions for rewards." },
  ];

  const WEEKLY: Task[] = [
    { id: "weekly-nightwave", title: "Nightwave", description: "Complete weekly Nightwave missions." },
    { id: "weekly-clem", title: "Help Clem", description: "Complete Clem's weekly survival mission (Relay)." },
    { id: "weekly-circuit-normal", title: "Duviri Circuit (Normal)", description: "Run for weekly Warframe rewards." },
    { id: "weekly-circuit-steelpath", title: "Duviri Circuit (Steel Path)", description: "Run for weekly Incarnon Adapters." },
    { id: "weekly-acrithis", title: "Acrithis Wares", description: "Check Acrithis for weekly Riven mods, Catalysts, and useful items." },
    { id: "weekly-archon-hunt", title: "Archon Hunt (Archon Shards)", description: "Do the weekly Archon Hunt for an Archon Shard and rewards." },
    { id: "weekly-ayatan", title: "Ayatan Treasure Hunt", description: "Complete Maroo's weekly hunt for an Ayatan Sculpture." },
    {
      id: "weekly-teshin",
      title: "Teshin Steel Path Honors",
      description: "Purchase Umbra Forma, Rivens, and Forma bundles with Steel Essence.",
    },
    { id: "weekly-iron-wake", title: "Iron Wake Shop", description: "Visit Palladino for 2 weekly Rivens and Kuva trade-ins." },
    { id: "weekly-kahl-mission", title: "Kahl Break Narmer", description: "Complete Kahl's weekly mission for Stock (Veilbreaker)." },
    { id: "weekly-yonta", title: "Archimedian Yonta", description: "Buy 35,000 Kuva for weekly Voidplumes." },
    { id: "weekly-calendar", title: "1999 Calendar Tasks", description: "Complete weekly POM-2 terminal tasks for Shards, Arcanes, and Rivens." },
    { id: "weekly-bird3", title: "Bird 3 (Cavia)", description: "Buy the weekly Archon Shard (requires Rank 5 Cavia)." },
    {
      id: "weekly-descendia-normal",
      title: "The Descendia (Normal)",
      description: "Complete the full tower descent in The Descendia for multiple rewards.",
    },
    {
      id: "weekly-descendia-steelpath",
      title: "The Descendia (Steel Path)",
      description: "Complete the full tower descent in The Descendia for multiple rewards.",
    },
    { id: "weekly-netracells", title: "Netracells", description: "Complete 5 weekly Netracell missions for Shard drops." },
    { id: "weekly-deep-archimedea", title: "Elite Deep Archimedea", description: "High-end weekly challenge for powerful Archon Shard chances." },
    { id: "weekly-temporal-archimedea", title: "Elite Temporal Archimedea", description: "Weekly endgame challenge for Archon Shard rewards." },
    // Added by the user (not on TipsyContent's list).
    {
      id: "weekly-icebind",
      title: "The Icebind (Riven Splicers)",
      description: "Collect 3 keys or complete 3 missions for riven-related rewards.",
    },
  ];

  const DAY = 24 * 60 * 60 * 1000;

  /** The last daily reset (00:00 UTC) at or before `now`. */
  const lastDaily = (now: number) => Math.floor(now / DAY) * DAY;
  /** The last weekly reset (Monday 00:00 UTC) at or before `now`. */
  function lastWeekly(now: number) {
    const day = lastDaily(now);
    const weekday = new Date(day).getUTCDay(); // 0 = Sunday
    return day - ((weekday + 6) % 7) * DAY;
  }
  const lastReset = (kind: Kind, now: number) => (kind === "daily" ? lastDaily(now) : lastWeekly(now));
  const nextReset = (kind: Kind, now: number) => lastReset(kind, now) + (kind === "daily" ? DAY : 7 * DAY);

  function untilText(ms: number) {
    const mins = Math.max(0, Math.ceil(ms / 60000));
    const d = Math.floor(mins / 1440);
    const h = Math.floor((mins % 1440) / 60);
    const m = mins % 60;
    return d > 0 ? `${d}d ${h}h` : h > 0 ? `${h}h ${m}m` : `${m}m`;
  }

  const DONE_KEY = "checklist.done";
  const OPEN_KEY = "checklist.open";
  const REMOVED_KEY = "checklist.removed";

  function read<T>(key: string, fallback: T): T {
    try {
      const raw = localStorage.getItem(key);
      return raw ? { ...fallback, ...JSON.parse(raw) } : fallback;
    } catch {
      return fallback;
    }
  }
  function write(key: string, value: unknown) {
    try {
      localStorage.setItem(key, JSON.stringify($state.snapshot(value)));
    } catch {}
  }

  /** Task id → when it was ticked (ms). A tick from before the last reset no longer counts. */
  let done = $state<Record<string, number>>(read(DONE_KEY, {}));
  let open = $state<Record<Kind, boolean>>(read(OPEN_KEY, { daily: true, weekly: true }));
  /** Tasks taken off the list (id → true); they can be added back. */
  let removed = $state<Record<string, boolean>>(read(REMOVED_KEY, {}));
  let now = $state(Date.now());

  onMount(() => {
    const timer = setInterval(() => (now = Date.now()), 30_000);
    return () => clearInterval(timer);
  });

  const isDone = (kind: Kind, id: string) => (done[id] ?? 0) >= lastReset(kind, now);

  function toggle(kind: Kind, id: string) {
    if (isDone(kind, id)) delete done[id];
    else done[id] = Date.now();
    write(DONE_KEY, done);
  }

  function setRemoved(id: string, on: boolean) {
    if (on) removed[id] = true;
    else delete removed[id];
    write(REMOVED_KEY, removed);
  }

  function toggleOpen(kind: Kind) {
    open[kind] = !open[kind];
    write(OPEN_KEY, open);
  }

  const ALL_DRAWERS: { kind: Kind; title: string; tasks: Task[] }[] = [
    { kind: "daily", title: "Daily", tasks: DAILY },
    { kind: "weekly", title: "Weekly", tasks: WEEKLY },
  ];
  // Either can be hidden from the Tools page.
  const DRAWERS = $derived(ALL_DRAWERS.filter((d) => !checklistHidden[d.kind]));
</script>

{#if DRAWERS.length}
<div class="checklist-wrap">
<div class="checklist" class:single={DRAWERS.length === 1}>
  {#each DRAWERS as d (d.kind)}
    {@const shown = d.tasks.filter((t) => !removed[t.id])}
    {@const hiddenTasks = d.tasks.filter((t) => removed[t.id])}
    {@const count = shown.filter((t) => isDone(d.kind, t.id)).length}
    <section class="drawer" class:open={open[d.kind]}>
      <button class="drawer-head" onclick={() => toggleOpen(d.kind)} aria-expanded={open[d.kind]}>
        <ChevronRight class="caret" size={18} strokeWidth={2} aria-hidden="true" />
        <h2>{d.title}</h2>
        <span class="count" class:all={count === shown.length}>{count}/{shown.length}</span>
        <span class="reset muted">resets in {untilText(nextReset(d.kind, now) - now)}</span>
      </button>
      {#if open[d.kind]}
        <ul>
          {#each shown as t (t.id)}
            {@const checked = isDone(d.kind, t.id)}
            <li class:checked>
              <label>
                <input type="checkbox" {checked} onchange={() => toggle(d.kind, t.id)} />
                <span class="text">
                  <span class="title">{t.title}</span>
                  <span class="desc">{t.description}</span>
                </span>
              </label>
              <button class="remove" onclick={() => setRemoved(t.id, true)} title="Remove from the list" aria-label="Remove {t.title}">
                <X size={16} strokeWidth={2} aria-hidden="true" />
              </button>
            </li>
          {:else}
            <li class="empty muted">No tasks on the list. Add some from below.</li>
          {/each}
        </ul>
        {#if hiddenTasks.length}
          <div class="removed">
            <span class="removed-label">Additional {d.kind === "daily" ? "dailies" : "weeklies"}</span>
            {#each hiddenTasks as t (t.id)}
              <button class="add-back" onclick={() => setRemoved(t.id, false)} title="Add back to the list">
                <Plus size={14} strokeWidth={2.25} aria-hidden="true" />{t.title}
              </button>
            {/each}
          </div>
        {/if}
      {/if}
    </section>
  {/each}
</div>
</div>
{/if}

<style>
  /* Stacked, or side by side when there's room. */
  .checklist-wrap {
    container-type: inline-size;
    margin-bottom: 20px;
  }

  .checklist {
    display: grid;
    gap: 12px;
  }

  @container (min-width: 900px) {
    .checklist:not(.single) {
      grid-template-columns: 1fr 1fr;
      align-items: start;
    }
  }

  .drawer {
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .drawer-head {
    all: unset;
    box-sizing: border-box;
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px;
    cursor: pointer;
  }

  .drawer-head:hover {
    background: var(--surface-2);
  }

  .drawer-head:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  .drawer-head :global(.caret) {
    flex: none;
    color: var(--muted);
    transition: transform 0.15s;
  }

  .open .drawer-head :global(.caret) {
    transform: rotate(90deg);
  }

  h2 {
    margin: 0;
    font-size: 17px;
  }

  .count {
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }

  .count.all {
    color: var(--good);
  }

  .reset {
    margin-left: auto;
    font-size: 13px;
  }

  .muted {
    color: var(--muted);
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 4px 8px 10px;
    border-top: 1px solid var(--line);
    display: grid;
    gap: 2px;
  }

  li {
    position: relative;
    display: flex;
    align-items: flex-start;
  }

  li > label {
    flex: 1;
    min-width: 0;
  }

  /* Remove: shows on hover (and keyboard focus), at the row's right. */
  .remove {
    all: unset;
    flex: none;
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    margin: 6px 4px 0 0;
    border-radius: 6px;
    color: var(--muted);
    cursor: pointer;
    opacity: 0;
  }

  li:hover .remove,
  .remove:focus-visible {
    opacity: 1;
  }

  .remove:hover {
    background: var(--surface-hover);
    color: var(--bad);
  }

  .remove:focus-visible {
    outline: 2px solid var(--accent);
  }

  .empty {
    padding: 10px 8px;
    font-size: 13px;
  }

  /* Removed tasks, to add back. */
  .removed {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding: 10px 16px 14px;
    border-top: 1px solid var(--line);
  }

  .removed-label {
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--muted);
    margin-right: 4px;
  }

  .add-back {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font: inherit;
    font-size: 13px;
    color: var(--muted);
    background: var(--surface-2);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 4px 10px 4px 7px;
    cursor: pointer;
  }

  .add-back:hover {
    color: var(--text);
    border-color: var(--line-strong);
  }

  label {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 8px;
    border-radius: 6px;
    cursor: pointer;
  }

  label:hover {
    background: var(--surface-2);
  }

  input {
    flex: none;
    width: 17px;
    height: 17px;
    margin: 2px 0 0;
    accent-color: var(--accent);
    cursor: pointer;
  }

  .text {
    display: grid;
    gap: 2px;
  }

  .title {
    font-weight: 600;
  }

  .desc {
    font-size: 13px;
    color: var(--muted);
  }

  /* Done: faded and struck through. */
  .checked .text {
    opacity: 0.45;
  }

  .checked .title {
    text-decoration: line-through;
  }
</style>
