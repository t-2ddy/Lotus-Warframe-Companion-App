<script lang="ts">
  // The Foundry's item popup: the full recipe, each part's own blueprint cost, and where to get
  // every part (mission, relic, bounty, enemy, vendor, Clan Dojo lab or Market).
  import { invoke } from "@tauri-apps/api/core";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import { compact, hideOnError, imageUrl, markMissing } from "$lib/format";

  type Source = {
    kind: "relic" | "mission" | "bounty" | "enemy" | "activity" | "vendor" | "research" | "market";
    place: string;
    detail: string;
    chance: number | null;
    cost: string | null;
    vaulted: boolean;
    link: string | null;
  };

  type Recipe = { credits: number; time: number; ingredients: Ingredient[] };

  type Ingredient = {
    path: string | null;
    name: string;
    image: string | null;
    overlay: string | null;
    needed: number;
    owned: number;
    kind: "blueprint" | "part" | "item" | "resource";
    recipe: Recipe | null;
    sources: Source[];
  };

  type Detail = {
    path: string;
    name: string;
    image: string | null;
    vaulted: boolean;
    recipe: Recipe | null;
    sources: Source[];
    wikiLoaded: boolean;
  };

  let { path, onclose }: { path: string; onclose: () => void } = $props();

  let detail = $state<Detail | null>(null);
  let error = $state<string | null>(null);
  /** Ingredients whose full source list is shown. */
  let showAll = $state<Record<string, boolean>>({});

  onMount(() => {
    invoke<Detail>("get_item_detail", { path })
      .then((d) => (detail = d))
      .catch((e) => (error = String(e)));
  });

  const KIND_LABEL: Record<Source["kind"], string> = {
    relic: "Relic",
    mission: "Mission",
    bounty: "Bounty",
    enemy: "Enemy",
    activity: "Activity",
    vendor: "Vendor",
    research: "Dojo lab",
    market: "Market",
  };

  const PART_LABEL: Record<Ingredient["kind"], string> = {
    blueprint: "Main blueprint",
    part: "Part",
    item: "Item",
    resource: "Resource",
  };

  /** Lines shown before "Show more". Relics are chips, so more of them fit. */
  const SHOWN = 4;
  const SHOWN_RELICS = 8;

  /** Seconds → "3 days", "12 h", "1 min". */
  function duration(s: number) {
    if (s >= 86400) return `${+(s / 86400).toFixed(1)} day${s === 86400 ? "" : "s"}`;
    if (s >= 3600) return `${+(s / 3600).toFixed(1)} h`;
    if (s >= 60) return `${Math.round(s / 60)} min`;
    return `${s} s`;
  }

  const chance = (c: number) => `${c.toLocaleString(undefined, { maximumFractionDigits: 2 })}%`;
  const wikiUrl = (page: string) => `https://wiki.warframe.com/w/${encodeURIComponent(page.replace(/ /g, "_"))}`;
  const openWiki = (page: string | null) => page && openUrl(wikiUrl(page)).catch(() => {});

  /** Parts first, in the game's order; resources after, as they matter least. */
  const parts = $derived(detail?.recipe?.ingredients.filter((i) => i.kind !== "resource") ?? []);
  const resources = $derived(detail?.recipe?.ingredients.filter((i) => i.kind === "resource") ?? []);
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
<div class="backdrop" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="item-detail-title">
    <header class="dialog-head">
      {#if detail?.image}<img class="dialog-pic" src={imageUrl(detail.image)} alt="" onerror={markMissing} />{/if}
      <div class="head-text">
        <h2 id="item-detail-title">{detail?.name ?? "…"}</h2>
        {#if detail?.recipe}
          <p class="muted">
            Build: {detail.recipe.credits.toLocaleString()} credits · {duration(detail.recipe.time)}
            {#if detail.vaulted}<span class="tag">Vaulted</span>{/if}
          </p>
        {:else if detail?.vaulted}
          <p><span class="tag">Vaulted</span></p>
        {/if}
      </div>
      {#if detail}
        <button class="wiki" onclick={() => openWiki(detail!.name)} title="Open {detail.name} on the Warframe wiki">
          Wiki
          <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M4.5 2.5h5v5M9.5 2.5 3 9" /></svg>
        </button>
      {/if}
      <!-- svelte-ignore a11y_autofocus -->
      <button class="close" onclick={onclose} aria-label="Close" autofocus>
        <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3 3l6 6M9 3l-6 6" /></svg>
      </button>
    </header>

    {#if error}
      <p class="muted">Couldn't load this item: {error}</p>
    {:else if !detail}
      <p class="muted">Loading…</p>
    {:else}
      {#if detail.sources.length}
        <h3 class="dialog-sub">{detail.recipe ? "Or get it whole" : "Where to get it"}</h3>
        {@render sourceList(detail.sources, "item", true)}
      {/if}

      {#if parts.length}
        <h3 class="dialog-sub">Blueprint and parts</h3>
        <ul class="needs">
          {#each parts as p, i}
            {@render ingredient(p, `p${i}`)}
          {/each}
        </ul>
      {/if}

      {#if resources.length}
        <h3 class="dialog-sub">Resources</h3>
        <ul class="needs">
          {#each resources as r, i}
            {@render ingredient(r, `r${i}`)}
          {/each}
        </ul>
      {/if}

      {#if !detail.recipe && !detail.sources.length}
        <p class="muted">No source is listed for this item. The wiki page has more.</p>
      {/if}

      <footer class="dialog-foot muted">
        Drop chances from DE's drop tables (relics at Intact). Vendors, Clan Dojo labs and part recipes
        from the <button class="place credit" onclick={() => openWiki("Module:Blueprints/data")}>Warframe wiki</button>
        (CC BY-SA 3.0){detail.wikiLoaded ? "" : ", still loading"}.
      </footer>
    {/if}
  </div>
</div>

{#snippet ingredient(p: Ingredient, key: string)}
  {@const enough = p.owned >= p.needed}
  <li class:enough>
    <div class="need-head">
      <span class="need-icon" class:enough>
        {#if p.image}<img src={imageUrl(p.image)} alt="" onerror={markMissing} />{/if}
        {#if p.overlay}<img class="overlay" src={imageUrl(p.overlay)} alt="" onerror={hideOnError} />{/if}
      </span>
      <span class="need-name">
        {p.name}{#if p.needed > 1}<span class="muted"> ×{p.needed.toLocaleString()}</span>{/if}
        <span class="part-count" class:have={enough}>
          {#if enough}Have {compact(p.owned)}{:else}{compact(p.owned)} / {compact(p.needed)} owned{/if}
        </span>
      </span>
      <span class="kind">{PART_LABEL[p.kind]}</span>
    </div>

    {#if p.recipe}
      <div class="sub-recipe">
        <span class="muted">Its blueprint builds it with</span>
        {#each p.recipe.ingredients as r}
          <span class="res" class:have={r.owned >= r.needed} title="{r.name}: {r.owned.toLocaleString()} owned, {r.needed.toLocaleString()} needed">
            {#if r.image}<img src={imageUrl(r.image)} alt="" onerror={hideOnError} />{/if}
            {r.needed.toLocaleString()} {r.name}
          </span>
        {/each}
        <span class="muted">· {p.recipe.credits.toLocaleString()} credits · {duration(p.recipe.time)}</span>
      </div>
    {/if}

    {#if p.sources.length}
      {@render sourceList(p.sources, key)}
    {:else if p.kind !== "resource"}
      <p class="none muted">No source listed. Check the wiki.</p>
    {/if}
  </li>
{/snippet}

{#snippet sourceList(list: Source[], key: string, flush = false)}
  {@const relics = list.filter((s) => s.kind === "relic")}
  {@const others = list.filter((s) => s.kind !== "relic")}
  {@const open = showAll[key]}
  {@const hidden = Math.max(0, others.length - SHOWN) + Math.max(0, relics.length - SHOWN_RELICS)}
  {#if others.length}
    <ul class="sources" class:flush>
      {#each open ? others : others.slice(0, SHOWN) as s}
        <li>
          <span class="badge {s.kind}">{KIND_LABEL[s.kind]}</span>
          <span class="where">
            {#if s.link}
              <button class="place" onclick={() => openWiki(s.link)} title="Open {s.link} on the Warframe wiki">{s.place}</button>
            {:else}
              <b class="place">{s.place}</b>
            {/if}
            {#if s.detail}<span class="muted"> · {s.detail}</span>{/if}
          </span>
          <span class="value">
            {#if s.cost}{s.cost}{:else if s.chance !== null}{chance(s.chance)}{/if}
          </span>
        </li>
      {/each}
    </ul>
  {/if}
  {#if relics.length}
    <ul class="drops" class:flush>
      {#each open ? relics : relics.slice(0, SHOWN_RELICS) as r}
        <li class:vaulted={r.vaulted} title={r.vaulted ? "Vaulted relic" : "Drop chance at Intact"}>
          <span class="relic">{r.place}</span>
          {#if r.chance !== null}<span class="chance">{chance(r.chance)}</span>{/if}
        </li>
      {/each}
    </ul>
  {/if}
  {#if hidden > 0 || open}
    <button class="more" class:flush onclick={() => (showAll[key] = !open)}>
      {open ? "Show fewer" : `Show ${hidden} more`}
    </button>
  {/if}
{/snippet}

<style>
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
    width: min(680px, 100%);
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
    margin-bottom: 6px;
  }

  .head-text {
    min-width: 0;
  }

  .dialog-head h2 {
    margin: 0;
    font-size: 18px;
  }

  .dialog-head p {
    margin: 2px 0 0;
    font-size: 13px;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .dialog-pic {
    width: 64px;
    height: 52px;
    object-fit: contain;
  }

  .tag {
    font-size: 12px;
    padding: 1px 8px;
    border-radius: 4px;
    background: var(--surface-2);
    color: var(--muted);
  }

  .wiki {
    margin-left: auto;
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 5px 8px;
    border: none;
    border-radius: 6px;
    background: none;
    font: inherit;
    font-size: 12px;
    color: var(--muted);
    cursor: pointer;
  }

  .wiki:hover {
    background: var(--surface-2);
    color: var(--text);
  }

  .wiki svg {
    width: 10px;
    height: 10px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
  }

  .close {
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

  .dialog-sub {
    margin: 16px 0 8px;
    font-size: 13px;
    font-weight: 600;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
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

  .need-icon.enough {
    border-color: var(--good);
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

  .part-count {
    display: block;
    font-size: 12px;
    font-weight: 400;
    color: var(--muted);
  }

  .have {
    color: var(--good);
  }

  .kind {
    flex: none;
    font-size: 12px;
    color: var(--muted);
  }

  /* What a part's own blueprint takes: resource chips. */
  .sub-recipe {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin: 8px 0 0 50px;
    font-size: 13px;
  }

  .res {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 2px 9px 2px 4px;
    border-radius: 999px;
    background: var(--surface);
    border: 1px solid var(--line);
  }

  .res.have {
    border-color: rgba(52, 211, 153, 0.45);
  }

  .res img {
    width: 18px;
    height: 18px;
    object-fit: contain;
  }

  .sources {
    list-style: none;
    margin: 8px 0 0 50px;
    padding: 0;
    display: grid;
    gap: 4px;
    font-size: 13px;
  }

  /* Item-level sources sit under a heading, not under a part icon. */
  .sources.flush,
  .drops.flush,
  .more.flush {
    margin-left: 0;
  }

  .sources li {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .badge {
    flex: none;
    width: 62px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--muted);
  }

  /* One colour per kind of place, so a list reads at a glance. */
  .badge.mission,
  .badge.bounty {
    color: #93b8f8;
  }

  .badge.vendor,
  .badge.market {
    color: var(--gold);
  }

  .badge.research {
    color: #6fb8a8;
  }

  .badge.enemy {
    color: var(--bad);
  }

  .where {
    flex: 1;
    min-width: 0;
  }

  .place {
    padding: 0;
    border: none;
    background: none;
    font: inherit;
    font-weight: 600;
    color: var(--text);
  }

  button.place {
    cursor: pointer;
  }

  button.place:hover {
    color: #93b8f8;
    text-decoration: underline;
  }

  .value {
    flex: none;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
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

  .drops li.vaulted {
    opacity: 0.55;
  }

  .relic {
    font-weight: 600;
  }

  .chance {
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }

  .more {
    margin: 6px 0 0 50px;
    padding: 0;
    border: none;
    background: none;
    font: inherit;
    font-size: 12px;
    color: #93b8f8;
    cursor: pointer;
  }

  .more:hover {
    text-decoration: underline;
  }

  .none {
    margin: 8px 0 0 50px;
    font-size: 13px;
  }

  .dialog-foot {
    margin-top: 14px;
    font-size: 12px;
  }

  .place.credit {
    font-weight: 400;
    color: inherit;
    text-decoration: underline;
    cursor: pointer;
  }

  .muted {
    color: var(--muted);
  }
</style>
