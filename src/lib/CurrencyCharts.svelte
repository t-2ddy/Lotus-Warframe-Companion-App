<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import CurrencyChart, { type Day } from "$lib/CurrencyChart.svelte";

  type Series = {
    label: string;
    current: number;
    changes: { at: number; before: number; after: number }[];
    /** A count per day (relics opened): each change's `after - before` happened in that capture. */
    perDay: boolean;
  };
  type History = { since: number | null; series: Series[] };

  /** Days shown in each chart (fewer if the app hasn't been running that long). */
  const CHART_DAYS = 30;
  const ICONS: Record<string, string> = {
    Platinum: "/icons/platinum.png",
    Endo: "/icons/endo.png",
    Credits: "/icons/credits.png",
    "Relics opened": "/icons/relics/axi.png",
  };

  let history = $state<History | null>(null);

  const dayStart = (ms: number) => {
    const d = new Date(ms);
    return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  };
  const addDays = (ms: number, n: number) => {
    const d = new Date(ms);
    return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n).getTime();
  };

  /** The value just before `t`: the last change before it, else the first change's starting value. */
  function valueBefore(s: Series, t: number) {
    let v: number | null = null;
    for (const c of s.changes) {
      if (c.at < t) v = c.after;
      else break;
    }
    return v ?? s.changes[0]?.before ?? s.current;
  }

  /** End-of-day values and day-over-day changes, in local days, from the first capture to today. */
  function daily(s: Series, since: number): Day[] {
    const today = dayStart(Date.now());
    let day = Math.max(dayStart(since), addDays(today, -(CHART_DAYS - 1)));
    const out: Day[] = [];
    while (day <= today) {
      const next = addDays(day, 1);
      const value = valueBefore(s, next);
      out.push({ day, value, change: value - valueBefore(s, day) });
      day = next;
    }
    return out;
  }

  /** Per-day counts: the day's total as the value, and the change from the day before. */
  function dailyCounts(s: Series, since: number): Day[] {
    const today = dayStart(Date.now());
    let day = Math.max(dayStart(since), addDays(today, -(CHART_DAYS - 1)));
    const out: Day[] = [];
    let previous = 0;
    while (day <= today) {
      const next = addDays(day, 1);
      const value = s.changes.filter((c) => c.at >= day && c.at < next).reduce((a, c) => a + c.after - c.before, 0);
      out.push({ day, value, change: value - previous });
      previous = value;
      day = next;
    }
    return out;
  }

  const charts = $derived(
    history?.since != null
      ? history.series.map((s) => ({
          label: s.label,
          icon: ICONS[s.label] ?? "",
          days: s.perDay ? dailyCounts(s, history!.since!) : daily(s, history!.since!),
        }))
      : [],
  );

  const load = () =>
    invoke<History>("get_currency_history")
      .then((h) => (history = h))
      .catch(() => {});

  onMount(() => {
    load();
    const off = listen("inventory-updated", load);
    return () => {
      off.then((f) => f());
    };
  });
</script>

{#if charts.length}
  <section class="charts">
    <h2>Your personal stats</h2>
    <div class="grid">
      {#each charts as c (c.label)}
        <CurrencyChart label={c.label} icon={c.icon} days={c.days} />
      {/each}
    </div>
  </section>
{/if}

<style>
  .charts {
    margin-top: 28px;
    container-type: inline-size;
  }

  h2 {
    margin: 0 0 10px;
    font-size: 20px;
  }

  /* Four charts: one per row when narrow, two by two from the width where Arbitrations and Void
     Fissures go side by side, all four in a row on a very wide window. */
  .grid {
    display: grid;
    grid-template-columns: 1fr;
    gap: 24px; /* the same gap as Arbitrations and Void Fissures above */
  }

  @container (min-width: 900px) {
    .grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }

  @container (min-width: 1800px) {
    .grid {
      grid-template-columns: repeat(4, minmax(0, 1fr));
    }
  }
</style>
