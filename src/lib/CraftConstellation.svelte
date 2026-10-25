<script lang="ts" module>
  export type WebNode = {
    path: string;
    name: string;
    image: string | null;
    mastered: boolean;
    owned: number;
    rank: number;
    maxRank: number;
  };
  export type WebLink = { from: string; to: string; count: number };
  export type CraftWeb = { nodes: WebNode[]; links: WebLink[] };
</script>

<script lang="ts">
  // Mastery Helper → Crafting trees: items built from other masterable items, drawn as a build
  // diagram. Ingredients sit on the left; the lines from every ingredient of an item meet in one
  // bracket just before it, the way the Foundry merges parts into one build. Each weapon is an item
  // tile (picture, name, mastered / rank / not owned), and hovering one keeps its chain lit.
  import { imageUrl, markMissing } from "$lib/format";

  let {
    web: fullWeb,
    onopen,
    focus = null,
  }: { web: CraftWeb; onopen: (path: string) => void; focus?: string | null } = $props();

  /** Everything linked to `path`: what goes into it, and what it goes into, all the way. */
  function chainOf(links: WebLink[], path: string) {
    const set = new Set([path]);
    const walk = (p: string, up: boolean) => {
      for (const l of links) {
        const [here, next] = up ? [l.to, l.from] : [l.from, l.to];
        if (here === p && !set.has(next)) {
          set.add(next);
          walk(next, up);
        }
      }
    };
    walk(path, true);
    walk(path, false);
    return set;
  }

  /** With `focus`, only that weapon's chain is drawn. */
  const web = $derived.by(() => {
    if (!focus) return fullWeb;
    const keep = chainOf(fullWeb.links, focus);
    return {
      nodes: fullWeb.nodes.filter((n) => keep.has(n.path)),
      links: fullWeb.links.filter((l) => keep.has(l.from) && keep.has(l.to)),
    };
  });

  /** Tile size, the gap between steps (room for the bracket), and between tiles in a step. */
  const W = 224;
  const H = 62;
  const GAP_X = 64;
  const GAP_Y = 14;
  /** Where a bracket's trunk stands: this far before the item it builds. */
  const TRUNK = 22;
  /** Corner radius of a bracket's bends. */
  const BEND = 7;

  type Placed = WebNode & { x: number; y: number };
  type Wire = WebLink & { a: Placed; b: Placed };
  type Tree = { id: number; nodes: Placed[]; wires: Wire[]; width: number; height: number; finals: string[] };

  const trees = $derived.by(() => layout(web));

  function layout(web: CraftWeb): Tree[] {
    const byPath = new Map(web.nodes.map((n) => [n.path, n]));
    const links = web.links.filter((l) => byPath.has(l.from) && byPath.has(l.to));
    const ins = new Map<string, string[]>(); // result → ingredients
    const outs = new Map<string, string[]>(); // ingredient → results
    for (const l of links) {
      ins.set(l.to, [...(ins.get(l.to) ?? []), l.from]);
      outs.set(l.from, [...(outs.get(l.from) ?? []), l.to]);
    }

    // Trees: weapons connected by any link, found by walking both ways.
    const groupOf = new Map<string, number>();
    const groups: string[][] = [];
    for (const n of web.nodes) {
      if (groupOf.has(n.path)) continue;
      const id = groups.length;
      const members: string[] = [];
      const stack = [n.path];
      while (stack.length) {
        const p = stack.pop()!;
        if (groupOf.has(p)) continue;
        groupOf.set(p, id);
        members.push(p);
        stack.push(...(ins.get(p) ?? []), ...(outs.get(p) ?? []));
      }
      groups.push(members);
    }

    // Step: the longest chain of ingredients below a weapon (base weapons first).
    const depth = new Map<string, number>();
    const depthOf = (p: string): number => {
      if (!depth.has(p)) {
        depth.set(p, 0); // guards against a loop in the data
        depth.set(p, Math.max(0, ...(ins.get(p) ?? []).map((i) => depthOf(i) + 1)));
      }
      return depth.get(p)!;
    };

    const out = groups.map((members, id) => {
      const maxDepth = Math.max(...members.map(depthOf));
      const columns: string[][] = Array.from({ length: maxDepth + 1 }, () => []);
      for (const p of members.sort((a, b) => byPath.get(a)!.name.localeCompare(byPath.get(b)!.name))) {
        columns[depthOf(p)].push(p);
      }
      // Fewer crossings: order each step by where its lines go, sweeping right then left.
      const pos = new Map<string, number>();
      const index = () => columns.forEach((c) => c.forEach((p, i) => pos.set(p, i)));
      const by = (neighbours: Map<string, string[]>) => (p: string) => {
        const list = neighbours.get(p) ?? [];
        return list.length ? list.reduce((s, q) => s + pos.get(q)!, 0) / list.length : pos.get(p)!;
      };
      index();
      for (let pass = 0; pass < 3; pass++) {
        for (let d = 1; d <= maxDepth; d++) {
          const key = by(ins);
          columns[d].sort((a, b) => key(a) - key(b));
          index();
        }
        for (let d = maxDepth - 1; d >= 0; d--) {
          const key = by(outs);
          columns[d].sort((a, b) => key(a) - key(b));
          index();
        }
      }
      const rows = Math.max(...columns.map((c) => c.length));
      const placed = new Map<string, Placed>();
      columns.forEach((col, d) =>
        col.forEach((p, i) => {
          placed.set(p, {
            ...byPath.get(p)!,
            x: d * (W + GAP_X),
            // Shorter steps sit centred against the tallest.
            y: (i + (rows - col.length) / 2) * (H + GAP_Y),
          });
        }),
      );
      const nodes = [...placed.values()];
      return {
        id,
        nodes,
        wires: links.filter((l) => placed.has(l.from)).map((l) => ({ ...l, a: placed.get(l.from)!, b: placed.get(l.to)! })),
        width: (maxDepth + 1) * W + maxDepth * GAP_X,
        height: rows * H + (rows - 1) * GAP_Y,
        finals: nodes.filter((n) => !outs.has(n.path)).map((n) => n.name),
      };
    });
    return out.sort((a, b) => b.nodes.length - a.nodes.length || a.finals[0].localeCompare(b.finals[0]));
  }

  /** An elbow from an ingredient's right edge to its result's left edge, bending at the result's
   *  trunk, so every ingredient of one item shares the same vertical line. */
  function wire(a: Placed, b: Placed) {
    const [x1, y1] = [a.x + W, a.y + H / 2];
    const [x2, y2] = [b.x, b.y + H / 2];
    const xm = x2 - TRUNK;
    if (Math.abs(y2 - y1) < 1) return `M${x1} ${y1}H${x2}`;
    const s = Math.sign(y2 - y1);
    const r = Math.min(BEND, Math.abs(y2 - y1) / 2);
    return `M${x1} ${y1}H${xm - r}Q${xm} ${y1} ${xm} ${y1 + s * r}V${y2 - s * r}Q${xm} ${y2} ${xm + r} ${y2}H${x2}`;
  }

  // Hover: the weapon, everything that goes into it, and everything it goes into.
  let hovered = $state<string | null>(null);
  const chain = $derived(hovered ? chainOf(web.links, hovered) : null);
  const lit = (p: string) => !chain || chain.has(p);

  const status = (n: WebNode) =>
    n.mastered ? "Mastered" : n.owned > 0 ? `Rank ${n.rank}/${n.maxRank}` : n.rank > 0 ? `Rank ${n.rank}/${n.maxRank}, not owned` : "Not owned";

  const total = $derived(web.nodes.length);
  const masteredTotal = $derived(web.nodes.filter((n) => n.mastered).length);
</script>

<p class="summary">
  <span><b>{masteredTotal}</b> of {total} mastered {focus ? "in this tree" : "across crafting trees"}</span>
  <span class="muted">Click a weapon for its recipe</span>
</p>

{#if !trees.length}
  <p class="muted">No crafting trees yet. Item data is still loading.</p>
{:else}
  <div class="trees" class:single={trees.length === 1}>
    {#each trees as t (t.id)}
      <figure class="tree-wrap">
        {#if trees.length > 1}<figcaption>{t.finals.join(" · ")}</figcaption>{/if}
        <div class="tree" style="width: {t.width}px; height: {t.height}px">
          <svg class="wires" viewBox="0 0 {t.width} {t.height}" width={t.width} height={t.height} aria-hidden="true">
            {#each t.wires as w}
              {@const on = lit(w.a.path) && lit(w.b.path)}
              <path class="wire" class:on={chain && on} class:dim={!on} d={wire(w.a, w.b)} />
              {#if w.count > 1}
                <text class="times" class:dim={!on} x={w.b.x - TRUNK - 6} y={w.a.y + H / 2 - 5}>×{w.count}</text>
              {/if}
            {/each}
          </svg>
          {#each t.nodes as n (n.path)}
            <button
              class="tile"
              class:mastered={n.mastered}
              class:missing={!n.mastered && n.owned === 0}
              class:focus={n.path === focus}
              class:dim={!lit(n.path)}
              style="left: {n.x}px; top: {n.y}px; width: {W}px; height: {H}px"
              title="{n.name}: {status(n)}"
              onpointerenter={() => (hovered = n.path)}
              onpointerleave={() => hovered === n.path && (hovered = null)}
              onfocus={() => (hovered = n.path)}
              onblur={() => hovered === n.path && (hovered = null)}
              onclick={() => onopen(n.path)}
            >
              <span class="pic">
                {#if n.image}<img src={imageUrl(n.image)} alt="" loading="lazy" onerror={markMissing} />{/if}
              </span>
              <span class="text">
                <span class="name-line">
                  <span class="name">{n.name}</span>
                  {#if n.mastered}<img class="mastered-icon" src="/icons/mastered.png" alt="Mastered" />{/if}
                </span>
                {#if !n.mastered}<span class="state">{status(n)}</span>{/if}
              </span>
              {#if !n.mastered && n.rank > 0}
                <span class="rank" style="width: {(100 * n.rank) / n.maxRank}%" aria-hidden="true"></span>
              {/if}
            </button>
          {/each}
        </div>
      </figure>
    {/each}
  </div>
{/if}

<style>
  .summary {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    gap: 4px 16px;
    margin: 0 0 14px;
    font-size: 13px;
    color: var(--muted);
  }

  .summary b {
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }

  .trees {
    display: flex;
    flex-wrap: wrap;
    gap: 28px 40px;
  }

  .trees.single {
    justify-content: center;
  }

  .tree-wrap {
    margin: 0;
    max-width: 100%;
    overflow-x: auto;
    /* Room for the focus ring and the tiles' lift. */
    padding: 4px;
  }

  figcaption {
    margin-bottom: 10px;
    font-size: 13px;
    font-weight: 600;
    color: var(--muted);
  }

  .tree {
    position: relative;
  }

  .wires {
    position: absolute;
    inset: 0;
    overflow: visible;
  }

  .wire {
    fill: none;
    stroke: var(--line-strong);
    stroke-width: 1.5;
    transition:
      opacity 0.15s,
      stroke 0.15s;
  }

  .wire.on {
    stroke: var(--muted);
  }

  .times {
    font-size: 11px;
    font-weight: 600;
    fill: var(--muted);
    text-anchor: end;
    font-variant-numeric: tabular-nums;
  }

  .dim {
    opacity: 0.28;
  }

  /* A weapon: the same tile as the Inventory and Foundry, picture then name and status. */
  .tile {
    position: absolute;
    box-sizing: border-box;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 12px 0 7px;
    overflow: hidden;
    font: inherit;
    text-align: left;
    color: var(--text);
    background: var(--surface-2);
    border: 1px solid var(--line);
    border-radius: 8px;
    cursor: pointer;
    transition:
      opacity 0.15s,
      border-color 0.15s,
      background-color 0.15s;
  }

  .tile:hover {
    border-color: var(--line-strong);
    background: var(--surface-hover);
  }

  .tile:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  /* The item whose card was opened. */
  .tile.focus {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .pic {
    flex: none;
    display: grid;
    place-items: center;
    width: 50px;
    height: 50px;
    border-radius: 6px;
    background: var(--surface);
  }

  .pic img {
    width: 46px;
    height: 46px;
    object-fit: contain;
  }

  .missing .pic img {
    opacity: 0.45;
  }

  .text {
    display: grid;
    gap: 2px;
    min-width: 0;
  }

  /* The name, then the mastered wreath right after it. */
  .name-line {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    font-size: 17px;
  }

  .name {
    overflow: hidden;
    font-weight: 600;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .mastered-icon {
    flex: none;
    width: 1.5em;
    height: 1.5em;
    object-fit: contain;
  }

  .missing .name {
    color: var(--muted);
  }

  /* The opened item stays readable even when it isn't owned. */
  .focus .name {
    color: var(--text);
  }

  .state {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  /* Rank so far, along the bottom edge of weapons still levelling. */
  .rank {
    position: absolute;
    left: 0;
    bottom: 0;
    height: 2px;
    background: var(--accent);
  }
</style>
