<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { friendlyError } from "$lib/errors";
  import { onMount } from "svelte";

  type Fissure = {
    tag: string;
    node: string;
    planet: string;
    missionType: string;
    enemy: string;
    era: string;
    eraOrder: number;
    activationMs: number;
    expiryMs: number;
    steelPath: boolean;
  };

  type Mode = "normal" | "steel";
  const MODE_KEY = "fissureMode";

  let fissures = $state<Fissure[] | null>(null);
  let error = $state<string | null>(null);
  let now = $state(Date.now());
  let mode = $state<Mode>(
    (() => {
      try {
        return localStorage.getItem(MODE_KEY) === "steel" ? "steel" : "normal";
      } catch {
        return "normal";
      }
    })(),
  );

  function setMode(m: Mode) {
    mode = m;
    try {
      localStorage.setItem(MODE_KEY, m);
    } catch {}
  }

  async function load() {
    try {
      fissures = await invoke<Fissure[]>("get_fissures");
      error = null;
    } catch (e) {
      error = friendlyError(e, "the Warframe world state");
    }
  }

  /** Still running, per mode. */
  const live = $derived((fissures ?? []).filter((f) => f.expiryMs > now));
  const counts = $derived({
    normal: live.filter((f) => !f.steelPath).length,
    steel: live.filter((f) => f.steelPath).length,
  });

  /** The chosen mode's fissures grouped by relic era (the backend sorts by era, then time left). */
  const eras = $derived.by(() => {
    const groups: { era: string; items: Fissure[] }[] = [];
    for (const f of live.filter((x) => x.steelPath === (mode === "steel"))) {
      const last = groups.at(-1);
      if (last && last.era === f.era) last.items.push(f);
      else groups.push({ era: f.era, items: [f] });
    }
    return groups;
  });

  /** Relic images per era (static/icons/relics). Omnia takes any era, so it cycles through them. */
  const RELICS = ["lith", "meso", "neo", "axi", "requiem"];
  const relicsFor = (era: string) => {
    const e = era.toLowerCase();
    return e === "omnia" ? RELICS : RELICS.includes(e) ? [e] : [];
  };

  /**
   * Omnia's image while its heading is hovered: fades to the next era every 900 ms. On leave it
   * stops on the relic it reached (that fade still finishes to full opacity), starting from Lith.
   */
  let omniaIndex = $state(0);
  let omniaTimer: ReturnType<typeof setInterval> | undefined;
  function omniaHover(on: boolean) {
    clearInterval(omniaTimer);
    if (on) {
      omniaIndex = (omniaIndex + 1) % RELICS.length;
      omniaTimer = setInterval(() => (omniaIndex = (omniaIndex + 1) % RELICS.length), 900);
    }
  }

  const left = (ms: number) => {
    const mins = Math.max(0, Math.ceil((ms - now) / 60_000));
    return mins >= 60 ? `${Math.floor(mins / 60)}h ${mins % 60}m` : `${mins}m`;
  };

  onMount(() => {
    load();
    // New fissures replace expired ones at random times; the backend reuses a download for 60 s.
    const poll = setInterval(load, 60_000);
    const tick = setInterval(() => (now = Date.now()), 15_000);
    return () => {
      clearInterval(poll);
      clearInterval(tick);
      clearInterval(omniaTimer);
    };
  });
</script>

<section class="fissures">
  <div class="head">
    <h2>Void Fissures</h2>
    <div class="tabs" role="tablist">
      <button role="tab" aria-selected={mode === "normal"} class:active={mode === "normal"} onclick={() => setMode("normal")}>
        Normal<span class="count">{counts.normal}</span>
      </button>
      <button role="tab" aria-selected={mode === "steel"} class:active={mode === "steel"} onclick={() => setMode("steel")}>
        Steel Path<span class="count">{counts.steel}</span>
      </button>
    </div>
  </div>

  {#if fissures === null}
    <p class="muted">{error ? `Couldn't load fissures. ${error}` : "Loading fissures…"}</p>
  {:else}
    <div class="list">
      {#if !eras.length}
        <p class="muted empty">No {mode === "steel" ? "Steel Path" : "normal"} fissures right now.</p>
      {/if}
      {#each eras as g}
        {@const imgs = relicsFor(g.era)}
        {@const omnia = imgs.length > 1}
        <h3
          class="era era-{g.era.toLowerCase()}"
          onmouseenter={omnia ? () => omniaHover(true) : undefined}
          onmouseleave={omnia ? () => omniaHover(false) : undefined}
        >
          {#if imgs.length}
            <span class="relic" aria-hidden="true">
              {#each imgs as r, i}
                <img src="/icons/relics/{r}.png" alt="" class:shown={!omnia || i === omniaIndex} />
              {/each}
            </span>
          {/if}
          {g.era}<span class="era-count">{g.items.length}</span>
        </h3>
        <ul>
          {#each g.items as f (f.tag + f.activationMs)}
            <li class="row">
              <span class="where">
                <span class="node">{f.node}</span>
                {#if f.planet}<span class="planet">{f.planet}</span>{/if}
              </span>
              <span class="mission">{f.missionType || "–"}</span>
              <span class="enemy">{f.enemy}</span>
              <span class="left" title="Ends {new Date(f.expiryMs).toLocaleTimeString()}">{left(f.expiryMs)}</span>
            </li>
          {/each}
        </ul>
      {/each}
    </div>
    <p class="credit muted">Live from the Warframe world state{error ? " · offline, showing the last list" : ""}</p>
  {/if}
</section>

<style>
  .fissures {
    margin-top: 28px;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-bottom: 10px;
  }

  h2 {
    margin: 0;
    font-size: 20px;
  }

  .muted {
    color: var(--muted);
    margin: 0;
  }

  .tabs {
    margin-left: auto;
    display: flex;
    gap: 4px;
  }

  .tabs button {
    font: inherit;
    font-size: 14px;
    color: var(--text);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 4px 12px;
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

  .count,
  .era-count {
    margin-left: 6px;
    font-size: 12px;
    opacity: 0.7;
  }

  .list {
    max-height: 460px;
    overflow-y: auto;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 4px 0;
  }

  .empty {
    padding: 16px;
  }

  .era {
    position: sticky;
    top: -4px;
    z-index: 1;
    margin: 0;
    padding: 11px 16px;
    font-size: 16px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.8px;
    background: var(--surface-2);
    border-top: 1px solid var(--line-strong);
    border-bottom: 1px solid var(--line-strong);
    color: var(--era);
    box-shadow: inset 3px 0 0 var(--era);
  }

  .era {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  /* Relic image; Omnia stacks all five and fades between them. */
  .relic {
    position: relative;
    width: 30px;
    height: 30px;
    flex: none;
    margin: -4px 0;
  }

  .relic img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
    opacity: 0;
    transition: opacity 0.45s ease;
  }

  .relic img.shown {
    opacity: 1;
  }

  .era-count {
    margin-left: 0;
    color: var(--muted);
    font-size: 14px;
    font-weight: 500;
  }

  /* Era colours, flat: the heading text and its left bar. */
  .era { --era: var(--muted); }
  .era-lith { --era: #b8c4d6; }
  .era-meso { --era: #6fcf7f; }
  .era-neo { --era: #5aa9f0; }
  .era-axi { --era: #f2c14e; }
  .era-requiem { --era: #f87171; }
  .era-omnia { --era: #a78bfa; }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 150px 150px 70px;
    align-items: center;
    gap: 12px;
    padding: 9px 16px;
    border-bottom: 1px solid var(--line);
    font-size: 15px;
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

  .left {
    justify-self: end;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .credit {
    font-size: 12px;
    margin-top: 8px;
  }
</style>
