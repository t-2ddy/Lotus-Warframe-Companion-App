<script lang="ts" module>
  export type BestStat = { name: string; has: boolean };
  export type BestBuild = { required: BestStat[]; optional: BestStat[] };
  /** `always`: best on every weapon besides its builds (Blast, Corrosive, Viral, weak point). */
  export type Best = { builds: BestBuild[]; always: BestStat[]; harmless: BestStat[]; matched: number };
</script>

<script lang="ts">
  /**
   * A weapon's ideal riven stats from the community grading sheet, in two columns: positives and
   * negatives, each on the same light panel. The best in slot get a "Best" tag: in the
   * positives, the stats a build needs (the build this riven is closest to first); in the
   * negatives, the sheet's first pick. A ✓ marks
   * what this riven has. Sizes are in em, so it scales with its parent (overlay or popup).
   */
  let { best }: { best: Best } = $props();

  type Row = { name: string; has: boolean; bis: boolean };

  const positives = $derived.by(() => {
    const order = [best.builds[best.matched], ...best.builds.filter((_, i) => i !== best.matched)].filter(Boolean);
    const rows: Row[] = [];
    const add = (s: BestStat, bis: boolean) => {
      if (!rows.some((r) => r.name === s.name)) rows.push({ ...s, bis });
    };
    for (const b of order) b.required.forEach((s) => add(s, true));
    // Best on every weapon, after the sheet's own picks.
    (best.always ?? []).forEach((s) => add(s, true));
    for (const b of order) b.optional.forEach((s) => add(s, false));
    return rows;
  });

  const negatives = $derived<Row[]>(best.harmless.map((s, i) => ({ ...s, bis: i === 0 })));
</script>

<div class="best">
  <div class="col">
    <div class="head pos">Ideal positives</div>
    <ul>
      {#each positives as r}
        <li class:bis={r.bis}>
          <span class="name">{r.name}</span>
          {#if r.bis}<span class="tag">Best</span>{/if}
          {#if r.has}<span class="has" title="On this riven">✓</span>{/if}
        </li>
      {/each}
    </ul>
  </div>
  <div class="col">
    <div class="head neg">Ideal negatives</div>
    <ul>
      {#each negatives as r}
        <li class:bis={r.bis}>
          <span class="name">{r.name}</span>
          {#if r.bis}<span class="tag">Best</span>{/if}
          {#if r.has}<span class="has" title="This riven's negative">✓</span>{/if}
        </li>
      {:else}
        <li class="none">None listed</li>
      {/each}
    </ul>
  </div>
</div>

<style>
  .best {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.6em;
    color: #fff;
  }

  /* Both columns get the same light panel, equal in size. */
  .col {
    padding: 0.4em 0.5em 0.45em;
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.12);
  }

  .head {
    padding-bottom: 0.25em;
    margin-bottom: 0.3em;
    border-bottom: 2px solid;
    font-size: 0.78em;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .head.pos {
    border-color: var(--good, #34d399);
  }

  .head.neg {
    border-color: var(--bad, #f87171);
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.2em;
  }

  li {
    display: flex;
    align-items: center;
    gap: 0.4em;
    padding: 0.15em 0;
    font-size: 0.92em;
  }

  .name {
    min-width: 0;
  }

  .tag {
    margin-left: auto;
    padding: 0.05em 0.4em;
    border-radius: 3px;
    border: 1px solid rgba(255, 255, 255, 0.5);
    font-size: 0.7em;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .has {
    font-weight: 700;
  }

  li:not(.bis) .has {
    margin-left: auto;
  }

  .none {
    opacity: 0.6;
  }
</style>
