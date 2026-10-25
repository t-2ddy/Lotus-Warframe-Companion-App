<script lang="ts">
  /**
   * The game overlay, drawn over the game in its own transparent, click-through window.
   * Riven panels: one at the left for a chat-link riven; the current riven (left) and the new
   * roll (right) at the reroll screen. Relic rewards: a panel under each reward card.
   * Ctrl+Tab makes it clickable.
   */
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import type { RivenInfo } from "$lib/RivenPopup.svelte";
  import { hideOnError, imageUrl, markMissing } from "$lib/format";
  import BestStats from "$lib/BestStats.svelte";

  type Panel = {
    side: "left" | "right";
    role: string;
    title: string | null;
    status: "reading" | "fetching" | "ready" | "failed";
    message: string | null;
    note: string | null;
    read: string[];
    riven: RivenInfo | null;
    own: boolean;
    rankAssumed: boolean;
  };
  type SetPart = { name: string; image: string | null; overlay: string | null; owned: number; needed: number; this: boolean };
  type Reward = {
    name: string;
    path: string;
    rarity: string;
    plat: number | null;
    priceStatus: "loading" | "fresh" | "stale" | "untradable";
    ducats: number | null;
    vaulted: boolean;
    owned: number;
    needed: number;
    setName: string | null;
    setPath: string | null;
    setPlat: number | null;
    crafted: boolean;
    mastered: boolean;
    setParts: SetPart[];
    slug: string | null;
    x: number;
    width: number;
    top: number;
  };
  type OverlayState = {
    visible: boolean;
    interactive: boolean;
    mode: string;
    panels: Panel[];
    rewards: Reward[];
    platinum: number | null;
    ducats: number | null;
  };

  type Listing = {
    id: string;
    price: number;
    direct: boolean;
    stats: { text: string; positive: boolean; shared: boolean }[];
    status: string;
    url: string;
  };
  type Similar = { level: string; total: number; lowest: number | null; median: number | null; listings: Listing[] };

  let ov = $state<OverlayState>({ visible: false, interactive: false, mode: "", panels: [], rewards: [], platinum: null, ducats: null });
  /** Market listings per riven (keyed by weapon + stats), loaded once each. */
  let similar = $state<Record<string, Similar | "loading" | "error">>({});

  const key = (r: RivenInfo) => r.compat + "|" + r.stats.map((s) => s.tag).join(",");

  function loadSimilar(r: RivenInfo) {
    const k = key(r);
    if (similar[k]) return;
    similar[k] = "loading";
    invoke<Similar>("similar_rivens", {
      weapon: r.compat,
      weaponName: r.weapon,
      positives: r.stats.filter((s) => s.good).map((s) => s.tag),
      negative: r.stats.find((s) => !s.good)?.tag ?? null,
      wanted: r.stats.filter((s) => s.good && s.wanted).map((s) => s.tag),
    })
      .then((v) => (similar[k] = v))
      .catch(() => (similar[k] = "error"));
  }

  $effect(() => {
    for (const p of ov.panels) if (p.riven) loadSimilar(p.riven);
  });

  onMount(() => {
    // Transparent window: no page background.
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    invoke<OverlayState>("overlay_state").then((s) => (ov = s));
    const off = listen<OverlayState>("riven-overlay-state", (e) => (ov = e.payload));
    return () => {
      off.then((f) => f());
    };
  });

  const letterClass = (l: string) => "l-" + l[0].toLowerCase();
  const gradeClass = (g: string | null) => "g-" + (g ?? "none").toLowerCase();
  /** Short stat text for listing tiles ("+143.2% Slash" → "Slash"). */
  const shortStat = (t: string) => t.replace(/^[+\-x]?[\d.]+%?s?\s*/, "");
  const open = (url: string) => openUrl(url).catch(() => {});

  function platText(r: Reward) {
    if (r.priceStatus === "untradable") return "–";
    if (r.plat === null) return r.priceStatus === "loading" ? "…" : "–";
    return String(r.plat);
  }
  const craftState = (r: Reward) => (r.mastered ? "Mastered" : r.crafted ? "Crafted" : "Not crafted");
  const DUCAT = imageUrl("Ducat.png");
  /** The box behind the reward panels (vw/vh): the cards' span plus a margin. */
  const box = $derived.by(() => {
    if (!ov.rewards.length) return null;
    const left = Math.min(...ov.rewards.map((r) => r.x - r.width / 2)) * 100 - 0.45;
    const right = Math.max(...ov.rewards.map((r) => r.x + r.width / 2)) * 100 + 0.45;
    return { left, width: right - left, top: Math.min(...ov.rewards.map((r) => r.top)) * 100 - 0.7 };
  });
</script>

{#if ov.visible && ov.mode === "rewards"}
  <!-- Laid out like AlecaFrame's: one box under the player names, a panel lined up with each card,
       and a footer with the hint and the user's platinum and ducats. -->
  <div class="layer" class:interactive={ov.interactive}>
    {#if box}
      <div class="rewards" style="left: {box.left}vw; top: {box.top}vh; width: {box.width}vw">
        <div class="cards">
          {#each ov.rewards as r, i (i + r.path)}
            <section class="reward" style="left: {(r.x - r.width / 2) * 100 - box.left}vw; width: {r.width * 100}vw">
              {#if ov.interactive && r.slug}
                <button class="name" title="Open on warframe.market" onclick={() => open(`https://warframe.market/items/${r.slug}`)}>
                  {r.name}
                </button>
              {:else}
                <h3 class="name">{r.name}</h3>
              {/if}
              <div class="values">
                <span class:dim={r.priceStatus === "stale"} title="Lowest sell order on warframe.market">
                  {platText(r)} <img src="/icons/platinum.png" alt="platinum" />
                </span>
                {#if r.vaulted}<span class="vault">Vaulted</span>{/if}
                <span title="Ducats">{r.ducats ?? "–"} <img src={DUCAT} alt="ducats" onerror={hideOnError} /></span>
              </div>
              <div class="craft">
                {#if r.setName}
                  <span class="state" class:built={r.crafted || r.mastered}>{craftState(r)}</span>
                  <span class="have" title="Owned / needed for the set">{r.owned}/{r.needed}</span>
                {:else}
                  <span class="state">Owned</span>
                  <span class="have">{r.owned}</span>
                {/if}
              </div>
              {#if r.setParts.length > 1}
                <ul class="set">
                  {#each r.setParts as p}
                    <li
                      class:this={p.this}
                      class:enough={p.owned >= p.needed}
                      class:none={p.owned === 0}
                      title="{p.name}: {p.owned} owned, {p.needed} needed"
                    >
                      {#if p.image}<img src={imageUrl(p.image)} alt={p.name} onerror={markMissing} />{/if}
                      {#if p.overlay}<img class="over" src={imageUrl(p.overlay)} alt="" onerror={hideOnError} />{/if}
                      <span class="badge">{p.owned}</span>
                    </li>
                  {/each}
                </ul>
                <div class="set-price" title="{r.setName} set on warframe.market">
                  {r.setPlat ?? "–"} <img src="/icons/platinum.png" alt="platinum" />
                </div>
              {/if}
            </section>
          {/each}
        </div>
        <footer>
          <span class="brand"><img src="/lotus.png" alt="" /> Lotus</span>
          <span class="hint">
            {#if ov.interactive}
              <button class="done" onclick={() => invoke("overlay_done")}>Done</button> Ctrl+Tab to return to the game
            {:else}
              Use Ctrl+Tab to interact with the overlay
            {/if}
          </span>
          <span class="wallet">
            {#if ov.platinum !== null}<span>{ov.platinum.toLocaleString()} <img src="/icons/platinum.png" alt="platinum" /></span>{/if}
            {#if ov.ducats !== null}<span>{ov.ducats.toLocaleString()} <img src={DUCAT} alt="ducats" onerror={hideOnError} /></span>{/if}
          </span>
        </footer>
      </div>
    {/if}
  </div>
{:else if ov.visible}
  <div class="layer" class:interactive={ov.interactive}>
    {#each ov.panels as p (p.side)}
      <section class="panel {p.side}">
        <header>
          {#if p.riven}
            <span
              class="grade {gradeClass(p.riven.grade)}"
              title={p.riven.gradePending ? "Has a trait whose numbers aren't known yet, so it can't be graded" : "Grade from the community riven sheet"}
            >
              {p.riven.grade ?? (p.riven.gradePending ? "Collecting data" : "?")}
            </span>
            <h1>
              <span class="weapon">{p.riven.weapon}</span>
              <span class="rname">{p.riven.name}</span>
            </h1>
          {:else}
            <h1><span class="rname">{p.title ?? p.role}</span></h1>
          {/if}
          <span class="role">{p.role}</span>
        </header>

        {#if p.status === "reading"}
          <p class="note">Reading the card…</p>
        {:else if p.status === "fetching"}
          <div class="fetching" role="progressbar" aria-label="Fetching riven data"><span></span></div>
          <p class="note">Fetching riven data…</p>
          {#if p.read.length}
            <ul class="read">{#each p.read as line}<li>{line}</li>{/each}</ul>
          {/if}
        {:else if p.status === "failed"}
          <p class="note bad">{p.message}</p>
          {#if p.read.length}
            <ul class="read">{#each p.read as line}<li>{line}</li>{/each}</ul>
          {/if}
        {:else if p.riven}
          {@const r = p.riven}
          <ul class="stats">
            {#each r.stats as s}
              <li class:good={s.good} class:bad={!s.good} class:wanted={s.good && s.wanted === true}>
                <span class="stext">{s.text}</span>
                {#if s.pending}
                  {#if s.unread}
                    <span class="pending" title="Its number couldn't be read off the card: somewhere from {s.worst} to {s.best}">not read</span>
                  {:else if s.legacy}
                    <span class="pending" title="No longer rolls on this riven type (DE removed it), so it isn't graded">no longer rolls</span>
                  {:else}
                    <span class="pending" title="Spliced or new trait: its roll range isn't known yet, so it isn't graded">collecting data</span>
                  {/if}
                {:else}
                  <span class="letter {letterClass(s.letter)}">{s.letter}</span>
                  <span class="bar"><span style="width: {s.quality * 100}%"></span></span>
                {/if}
              </li>
            {/each}
          </ul>
          <div class="meta">
            {Math.round(r.perfectness)}% perfect · stats at max rank (riven is rank {r.rank}{p.rankAssumed ? ", estimated" : ""}) · disposition {r.disposition.toFixed(2)}
            {#if p.own}· <b>yours</b>{/if}
          </div>
          {#if p.note}<p class="note assumed">{p.note}</p>{/if}

          {#if r.best}
            <h2>Best stats for {r.weapon}</h2>
            <BestStats best={r.best} />
          {/if}

          <h2>Similar rivens</h2>
          {@const sim = similar[key(r)]}
          {#if !sim || sim === "loading"}
            <p class="note">Searching warframe.market…</p>
          {:else if sim === "error"}
            <p class="note">Market unavailable</p>
          {:else if !sim.listings.length}
            <p class="note">None listed</p>
          {:else}
            <div class="sim-head">
              {sim.level}{#if sim.lowest !== null} · from <b>{sim.lowest}</b>p{/if}{#if sim.median !== null} · median <b>{sim.median}</b>p{/if}
            </div>
            <div class="tiles" class:scroll={ov.interactive}>
              {#each sim.listings.filter((l) => l.direct).slice(0, ov.interactive ? 30 : 6) as l (l.id)}
                <button class="tile" onclick={() => open(l.url)} disabled={!ov.interactive}>
                  <span class="price">{l.price}p{#if l.status !== "offline"}<i class="on"></i>{/if}</span>
                  {#each l.stats as s}
                    <span class:pos={s.positive} class:neg={!s.positive} class:shared={s.shared}>{shortStat(s.text)}</span>
                  {/each}
                </button>
              {/each}
            </div>
          {/if}

        {/if}

        <footer>
          {#if ov.interactive}
            <button class="done" onclick={() => invoke("overlay_done")}>Done</button> Clickable · Ctrl+Tab to return
          {:else}
            Press <b>Ctrl+Tab</b> to interact
          {/if}
        </footer>
      </section>
    {/each}
  </div>
{/if}

<style>
  /* Sizes follow the game window's height so the panel looks the same at any resolution. */
  .layer {
    position: fixed;
    inset: 0;
    font-family: system-ui, "Segoe UI", sans-serif;
    font-size: clamp(11px, 1.15vh, 20px);
    color: #ece4fb;
    pointer-events: none;
  }

  .layer.interactive {
    pointer-events: auto;
  }

  .panel {
    position: absolute;
    top: 21vh;
    /* The layer's size + 2px; a little wider to match, so lines don't wrap more. */
    font-size: calc(clamp(11px, 1.15vh, 20px) + 2px);
    width: clamp(280px, 23vw, 570px);
    max-height: 62vh;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    background: rgba(28, 16, 46, 0.94);
    border: 2px solid #8c62cf;
    border-radius: 8px;
    padding: 0.6em 0.75em 0.5em;
    box-shadow: 0 0.6em 1.6em rgba(0, 0, 0, 0.5);
    /* Best stats chips (shared component) in the panel's palette. */
    --good: #6fcf7f;
    --bad: #ff8a95;
    --muted: #b9a3e0;
  }

  .panel.left {
    left: 1.2vw;
  }

  .panel.right {
    right: 1.2vw;
  }

  header {
    display: flex;
    align-items: center;
    gap: 0.5em;
    margin-bottom: 0.4em;
  }

  h1 {
    margin: 0;
    font-size: 1.35em;
    font-weight: 600;
    line-height: 1.15;
    min-width: 0;
  }

  .weapon {
    color: #c6aef0;
  }

  .rname {
    color: #fff;
  }

  .role {
    margin-left: auto;
    font-size: 0.75em;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: #b9a3e0;
    white-space: nowrap;
  }

  .grade {
    flex: none;
    font-size: 0.8em;
    font-weight: 800;
    padding: 0.15em 0.5em;
    border-radius: 4px;
    color: #160b26;
  }

  .g-great { background: #f2c14e; }
  .g-good { background: #6fcf7f; }
  .g-ok { background: #c9b3ec; }
  .g-bad { background: #f87171; }
  .g-none { background: #6b5a88; color: #ece4fb; }

  .note {
    margin: 0.3em 0;
    color: #c9b8e8;
  }

  .note.bad {
    color: #ff9aa4;
  }

  .note.assumed {
    font-size: 0.85em;
    color: #b9a3e0;
  }

  /* Waiting for the log or new riven data: a flat sliding bar, no glow. */
  .fetching {
    height: 3px;
    margin: 0.2em 0 0.1em;
    overflow: hidden;
    border-radius: 2px;
    background: rgba(140, 98, 207, 0.25);
  }

  .fetching span {
    display: block;
    width: 35%;
    height: 100%;
    background: #8c62cf;
    animation: fetching 1.1s ease-in-out infinite;
  }

  @keyframes fetching {
    from { transform: translateX(-100%); }
    to { transform: translateX(290%); }
  }

  .read {
    list-style: none;
    margin: 0.2em 0 0;
    padding: 0;
    display: grid;
    gap: 0.25em;
  }

  .read li {
    padding: 0.3em 0.5em;
    border-radius: 4px;
    background: rgba(255, 255, 255, 0.05);
    color: #ddd0f5;
  }

  .stats {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.25em;
  }

  .stats li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 2.2em 4.5em;
    align-items: center;
    gap: 0.5em;
    padding: 0.3em 0.5em;
    border-radius: 999px;
    background: rgba(47, 125, 69, 0.35);
  }

  .stats li.bad {
    background: rgba(140, 40, 55, 0.45);
  }

  .stats li.wanted .stext {
    font-weight: 700;
  }

  .stext {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .letter {
    text-align: center;
    font-weight: 800;
    font-size: 0.85em;
    border-radius: 3px;
    color: #160b26;
  }

  .pending {
    grid-column: span 2;
    font-size: 0.75em;
    font-style: italic;
    opacity: 0.75;
    text-align: right;
  }

  .l-s { background: #f2c14e; }
  .l-a { background: #6fcf7f; }
  .l-b { background: #5aa9f0; }
  .l-c { background: #e8875b; }
  .l-f { background: #f87171; }

  .bar {
    height: 0.55em;
    border-radius: 999px;
    border: 1px solid rgba(255, 255, 255, 0.6);
    overflow: hidden;
  }

  .bar span {
    display: block;
    height: 100%;
    background: #7be08f;
  }

  .stats li.bad .bar span {
    background: #ff6b7a;
  }

  .meta {
    margin-top: 0.35em;
    font-size: 0.8em;
    color: #b9a3e0;
  }

  h2 {
    margin: 0.6em 0 0.25em;
    font-size: 0.95em;
    font-weight: 600;
  }

  .sim-head {
    font-size: 0.8em;
    color: #c9b8e8;
    margin-bottom: 0.3em;
  }

  /* When the panel hits its height limit, the listings give way first (clipped, or scrolled when
     clickable), then the best stats; everything else, the Ctrl+Tab footer included, keeps its
     size. Shrinking is weighted by flex-shrink, so the listings take nearly all of it. */
  .panel > :global(*) {
    flex-shrink: 0;
  }

  .panel > .tiles {
    flex-shrink: 1000;
    min-height: 0;
    overflow: hidden;
  }

  .panel > :global(.best) {
    flex-shrink: 1;
    min-height: 0;
    overflow: hidden;
  }

  .tiles {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 0.3em;
  }

  .panel > .tiles.scroll {
    max-height: 22vh;
    overflow-y: auto;
  }

  .tile {
    all: unset;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.05em;
    padding: 0.3em 0.2em;
    border-radius: 5px;
    background: rgba(255, 255, 255, 0.06);
    font-size: 0.78em;
    line-height: 1.25;
    text-align: center;
  }

  .interactive .tile {
    cursor: pointer;
  }

  .interactive .tile:hover {
    background: rgba(255, 255, 255, 0.14);
  }

  .price {
    font-weight: 700;
    color: #fff;
    display: flex;
    align-items: center;
    gap: 0.3em;
  }

  .price .on {
    width: 0.5em;
    height: 0.5em;
    border-radius: 50%;
    background: #6fdc8c;
  }

  .tile .pos {
    color: #7be08f;
  }

  .tile .neg {
    color: #ff8a96;
  }

  .tile span:not(.shared):not(.price) {
    opacity: 0.6;
  }

  footer {
    margin-top: 0.5em;
    padding-top: 0.35em;
    border-top: 1px solid rgba(255, 255, 255, 0.12);
    font-size: 0.8em;
    text-align: center;
    color: #c9b8e8;
  }

  /* Relic rewards, laid out like AlecaFrame's (sizes from the window height). */
  .rewards {
    position: absolute;
    box-sizing: border-box;
    padding-top: 0.7vh;
    background: rgba(16, 23, 36, 0.96);
    border: 1px solid #2c3a55;
    border-radius: 0.7vh;
    font-size: clamp(11px, 1.3vh, 24px);
    color: #e3ebf8;
  }

  .cards {
    position: relative;
    height: 17.6vh;
  }

  .reward {
    position: absolute;
    top: 0;
    bottom: 0;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: space-evenly;
    padding: 0.4vh 0.3vw;
    background: #1d2840;
    border-radius: 0.5vh;
  }

  .name {
    margin: 0;
    max-width: 100%;
    font-size: 1.05em;
    font-weight: 600;
    line-height: 1.2;
    text-align: center;
    color: #fff;
  }

  button.name {
    all: unset;
    cursor: pointer;
    font-size: 1.05em;
    font-weight: 600;
    line-height: 1.2;
    text-align: center;
    color: #8fb6ff;
  }

  button.name:hover {
    text-decoration: underline;
  }

  .values {
    width: 80%;
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-weight: 600;
  }

  .values img,
  .set-price img,
  .wallet img {
    height: 1.25em;
    vertical-align: -0.25em;
  }

  .values .dim {
    opacity: 0.7;
  }

  .vault {
    font-size: 0.75em;
    padding: 0.1em 0.45em;
    border-radius: 0.3em;
    background: #4a3a14;
    color: #f2d58a;
  }

  /* "Crafted | 0/1", like Aleca's bar: the set's state, then owned / needed. */
  .craft {
    width: 80%;
    height: 1.7em;
    display: flex;
    border-radius: 0.4em;
    overflow: hidden;
    background: #141c2b;
    font-size: 0.9em;
  }

  .craft span {
    display: grid;
    place-items: center;
  }

  .craft .state {
    flex: 2;
    background: #2a3550;
    color: #c9d6ee;
  }

  .craft .state.built {
    background: #2f6b45;
    color: #e6f6ea;
  }

  .craft .have {
    flex: 1;
    font-weight: 600;
  }

  /* The set's parts as round icons with owned counts: green ring when there are enough, faded
     when there are none, blue for this card's part. */
  .set {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    justify-content: center;
    gap: 0.35em;
  }

  .set li {
    position: relative;
    box-sizing: border-box;
    width: 2.5em;
    height: 2.5em;
    border-radius: 50%;
    background: #141c2b;
    border: 2px solid #3a4866;
    display: grid;
    place-items: center;
  }

  .set li.enough {
    border-color: #3fae6a;
  }

  .set li.this {
    border-color: #5a8de0;
  }

  .set img {
    width: 75%;
    height: 75%;
    object-fit: contain;
  }

  .set img.over {
    position: absolute;
    width: 50%;
    height: 50%;
  }

  .set img:global(.missing) {
    visibility: hidden;
  }

  .set li.none img {
    opacity: 0.35;
    filter: grayscale(1);
  }

  .badge {
    position: absolute;
    right: -0.35em;
    bottom: -0.3em;
    min-width: 1.1em;
    padding: 0 0.2em;
    border-radius: 0.6em;
    background: #070a10;
    border: 1px solid #3a4866;
    font-size: 0.7em;
    font-weight: 700;
    line-height: 1.3;
    text-align: center;
  }

  .set-price {
    padding-top: 0.15em;
    border-top: 1px solid #3a4866;
    min-width: 55%;
    text-align: center;
    font-size: 0.9em;
    font-weight: 600;
  }

  .rewards footer {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    height: 3.1vh;
    padding: 0 0.8vw;
    font-size: 0.95em;
    color: #c9d6ee;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 0.4em;
    font-weight: 600;
  }

  .brand img {
    height: 1.1em;
  }

  .wallet {
    justify-self: end;
    display: flex;
    gap: 1.2em;
    font-weight: 600;
  }

  .done {
    all: unset;
    cursor: pointer;
    padding: 0.1em 0.7em;
    margin-right: 0.4em;
    border-radius: 4px;
    background: #6d4ad1;
    color: white;
    font-weight: 700;
  }
</style>
