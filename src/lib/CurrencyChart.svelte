<script lang="ts">
  /**
   * One currency (or count, like relics opened) over time: the end-of-day value as a line, and each day's change as a bar
   * (green up, red down) on its own scale around a zero line. Both can be toggled; the expand
   * button shows the chart full screen.
   */
  export type Day = { day: number; value: number; change: number };

  let { label, icon, days }: { label: string; icon: string; days: Day[] } = $props();

  let showValue = $state(true);
  let showChange = $state(true);
  let expanded = $state(false);
  let hover = $state<number | null>(null);

  const PAD = { top: 14, right: 16, bottom: 54, left: 54 };

  /** 25.0K, 1.2M, 950 */
  const short = (v: number) => {
    const a = Math.abs(v);
    if (a >= 1e6) return `${(v / 1e6).toFixed(1)}M`;
    if (a >= 1e3) return `${(v / 1e3).toFixed(1)}K`;
    return `${Math.round(v)}`;
  };
  const dayLabel = (ms: number) => new Date(ms).toLocaleDateString(undefined, { month: "short", day: "numeric" });
  const longDay = (ms: number) =>
    new Date(ms).toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
  const signed = (v: number) => (v > 0 ? "+" : v < 0 ? "−" : "±") + Math.abs(v).toLocaleString();

  /** A round axis maximum with 5 even steps. */
  function niceMax(v: number) {
    if (v <= 0) return 5;
    const step = v / 5;
    const mag = 10 ** Math.floor(Math.log10(step));
    // Small scales (counts like relics opened) step in whole numbers: no "2.5" shown as "3".
    if (step <= 10) return Math.ceil(step) * 5;
    const nice = [1, 2, 2.5, 5, 10].find((m) => m * mag >= step)! * mag;
    return nice * 5;
  }

  /** Geometry for a plot of the given size. */
  function layout(width: number, height: number) {
    const w = Math.max(0, width - PAD.left - PAD.right);
    const h = Math.max(0, height - PAD.top - PAD.bottom);
    const n = days.length;
    const step = n ? w / n : w;
    const x = (i: number) => PAD.left + step * (i + 0.5);

    const max = niceMax(Math.max(0, ...days.map((d) => d.value)) * 1.08);
    const y = (v: number) => PAD.top + h - (v / max) * h;
    const ticks = [0, 1, 2, 3, 4, 5].map((i) => (max / 5) * i);

    // Change bars: zero line at 40% up the plot; the biggest change reaches 35% of its height.
    const maxChange = Math.max(1, ...days.map((d) => Math.abs(d.change)));
    const zero = PAD.top + h * 0.6;
    const barY = (c: number) => zero - (c / maxChange) * h * 0.35;
    const barW = Math.max(3, Math.min(22, step * 0.6));

    // Keep x labels readable: about one per 46px.
    const every = Math.max(1, Math.ceil(46 / Math.max(step, 1)));

    const line = days.map((d, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(d.value).toFixed(1)}`).join(" ");
    return { w, h, step, x, y, ticks, zero, barY, barW, every, line };
  }

  let width = $state(0);
  let bigWidth = $state(0);
  let bigHeight = $state(0);

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") expanded = false;
  }
</script>

<svelte:window onkeydown={expanded ? onKey : undefined} />

{#snippet chart(width: number, height: number)}
  {@const g = layout(width, height)}
  {#if width > 0}
    <svg {width} {height} role="img" aria-label="{label} by day">
      <rect class="plot" x={PAD.left} y={PAD.top} width={g.w} height={g.h} />

      {#each g.ticks as t}
        <line class="grid" x1={PAD.left} x2={PAD.left + g.w} y1={g.y(t)} y2={g.y(t)} />
        {#if showValue}
          <text class="tick" x={PAD.left - 8} y={g.y(t) + 4} text-anchor="end">{short(t)}</text>
        {/if}
      {/each}

      {#if showChange}
        <line class="zero" x1={PAD.left} x2={PAD.left + g.w} y1={g.zero} y2={g.zero} />
        {#each days as d, i}
          {#if d.change !== 0}
            {@const top = Math.min(g.barY(d.change), g.zero)}
            <rect
              class="bar"
              class:up={d.change > 0}
              class:down={d.change < 0}
              x={g.x(i) - g.barW / 2}
              y={top}
              width={g.barW}
              height={Math.max(2, Math.abs(g.barY(d.change) - g.zero))}
            />
          {/if}
        {/each}
      {/if}

      {#if showValue && days.length}
        <path class="line" d={g.line} />
        {#each days as d, i}
          <circle class="dot" class:active={hover === i} cx={g.x(i)} cy={g.y(d.value)} r={hover === i ? 5 : 3.5} />
        {/each}
      {/if}

      {#each days as d, i}
        {#if i % g.every === 0}
          <text
            class="tick"
            text-anchor="end"
            transform="translate({g.x(i) + 4},{PAD.top + g.h + 14}) rotate(-45)">{dayLabel(d.day)}</text
          >
        {/if}
      {/each}

      <!-- Hover targets: one column per day. -->
      {#each days as _, i}
        <rect
          role="presentation"
          class="hit"
          x={PAD.left + g.step * i}
          y={PAD.top}
          width={g.step}
          height={g.h}
          onmouseenter={() => (hover = i)}
          onmouseleave={() => (hover = null)}
        />
      {/each}

      {#if hover !== null && days[hover]}
        {@const d = days[hover]}
        {@const tx = Math.min(g.x(hover) + 10, PAD.left + g.w - 150)}
        <g class="tip" transform="translate({tx},{PAD.top + 8})">
          <rect width="140" height="58" rx="6" />
          <text x="10" y="18" class="tip-day">{longDay(d.day)}</text>
          <text x="10" y="35">{d.value.toLocaleString()}</text>
          <text x="10" y="51" class:up={d.change > 0} class:down={d.change < 0}>{signed(d.change)}</text>
        </g>
      {/if}
    </svg>
  {/if}
{/snippet}

{#snippet header()}
  <div class="head">
    <h3>{label}<img src={icon} alt="" /></h3>
    <div class="controls">
      <label class="toggle">
        <input type="checkbox" bind:checked={showValue} />
        <span class="box value"></span>Value
      </label>
      <label class="toggle">
        <input type="checkbox" bind:checked={showChange} />
        <span class="box change"></span>Change
      </label>
      <button class="expand" title={expanded ? "Close" : "Full screen"} onclick={() => (expanded = !expanded)}>
        {#if expanded}
          <svg viewBox="0 0 16 16" width="16" height="16"><path d="M3 3l10 10M13 3L3 13" /></svg>
        {:else}
          <svg viewBox="0 0 16 16" width="16" height="16"
            ><path d="M2 6V2h4M10 2h4v4M14 10v4h-4M6 14H2v-4M2 2l4 4M14 2l-4 4M14 14l-4-4M2 14l4-4" /></svg
          >
        {/if}
      </button>
    </div>
  </div>
{/snippet}

<section class="card">
  {@render header()}
  <div class="area" bind:clientWidth={width}>
    {#if days.length}
      {@render chart(width, 260)}
    {:else}
      <p class="empty">No history yet.</p>
    {/if}
  </div>
</section>

{#if expanded}
  <div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && (expanded = false)}>
    <section class="card big">
      {@render header()}
      <div class="area fill" bind:clientWidth={bigWidth} bind:clientHeight={bigHeight}>
        {@render chart(bigWidth, bigHeight)}
      </div>
    </section>
  </div>
{/if}

<style>
  .card {
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 14px 16px 8px;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 6px;
  }

  h3 {
    margin: 0;
    font-size: 18px;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  h3 img {
    width: 24px;
    height: 24px;
    object-fit: contain;
  }

  .controls {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 14px;
    font-size: 14px;
  }

  /* Positioned, so the hidden checkbox inside is placed against it: otherwise it's placed against
     the whole window, at its spot down the scrolled page, and the window itself scrolls a page
     too far. */
  .toggle {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    user-select: none;
  }

  .toggle input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }

  .box {
    width: 13px;
    height: 13px;
    border: 1.5px solid #e4e9f2;
    border-radius: 2px;
  }

  .box.change {
    border-color: #e06c75;
  }

  .toggle input:checked + .box.value {
    background: #e4e9f2;
  }

  .toggle input:checked + .box.change {
    background: #7a2a35;
  }

  .toggle input:focus-visible + .box {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .expand {
    display: grid;
    place-items: center;
    padding: 4px;
    background: none;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    color: var(--text);
  }

  .expand:hover {
    background: var(--surface-hover);
  }

  .expand path {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
  }

  .empty {
    color: var(--muted);
    margin: 20px 0;
  }

  /* Chart */
  .plot {
    fill: var(--surface-2);
  }

  .grid {
    stroke: var(--line-strong);
    stroke-width: 1;
    opacity: 0.6;
  }

  .zero {
    stroke: var(--muted);
    stroke-width: 1;
    opacity: 0.5;
  }

  .tick {
    fill: var(--muted);
    font-size: 12px;
  }

  .bar.up {
    fill: #2f7d45;
    stroke: #8fe0a5;
    stroke-width: 1;
  }

  .bar.down {
    fill: #7a2a35;
    stroke: #e8a0a8;
    stroke-width: 1;
  }

  .line {
    fill: none;
    stroke: #e4e9f2;
    stroke-width: 1.8;
    stroke-linejoin: round;
  }

  .dot {
    fill: var(--surface-2);
    stroke: #e4e9f2;
    stroke-width: 1.5;
  }

  .dot.active {
    fill: #e4e9f2;
  }

  .hit {
    fill: transparent;
  }

  .tip {
    pointer-events: none;
  }

  .tip rect {
    fill: var(--bg);
    stroke: var(--line-strong);
  }

  .tip text {
    fill: var(--text);
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }

  .tip .tip-day {
    fill: var(--muted);
    font-size: 12px;
  }

  .tip text.up {
    fill: var(--good);
  }

  .tip text.down {
    fill: var(--bad);
  }

  /* Full screen */
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: rgba(0, 0, 0, 0.6);
    display: grid;
    padding: 32px;
  }

  .big {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .fill {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
</style>
