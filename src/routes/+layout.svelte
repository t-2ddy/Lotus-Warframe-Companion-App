<script lang="ts">
  import { page } from "$app/state";
  import { afterNavigate } from "$app/navigation";
  import { onMount, type Snippet } from "svelte";
  import { captureNow, pipeline, startStatus } from "$lib/status.svelte";
  import { time } from "$lib/format";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import type { Component } from "svelte";
  import House from "@lucide/svelte/icons/house";
  import Backpack from "@lucide/svelte/icons/backpack";
  import MaskIcon from "$lib/icons/MaskIcon.svelte";
  import Welcome from "$lib/Welcome.svelte";
  import { welcome } from "$lib/welcomeState.svelte";
  import WhatsNew from "$lib/WhatsNew.svelte";
  import { startWhatsNew, whatsNew } from "$lib/whatsNewState.svelte";
  import { appUpdate, installUpdate, startUpdateChecks } from "$lib/appUpdate.svelte";
  import Ducat from "$lib/icons/Ducat.svelte";
  import Riven from "$lib/icons/Riven.svelte";
  import Chat from "$lib/icons/Chat.svelte";
  import Wrench from "@lucide/svelte/icons/wrench";

  let { children }: { children: Snippet } = $props();

  /** `icon` is a component, or a picture path drawn white with `MaskIcon`. */
  type NavItem = { label: string; icon: Component | string; href?: string; iconScale?: number };
  // Pages without an href aren't built yet and show as "Soon". Icons: Lucide, plus our own Riven, Chat and Ducat; pictures (string paths) are drawn white with MaskIcon.
  const nav: NavItem[] = [
    { label: "Home", icon: House, href: "/" },
    { label: "Foundry", icon: "/icons/foundry.png", href: "/foundry" },
    { label: "Inventory", icon: Backpack, href: "/inventory" },
    { label: "Mastery Helper", icon: "/icons/mastered.png", href: "/mastery", iconScale: 1.15 },
    { label: "Rivens", icon: Riven, href: "/rivens" },
    { label: "Chat", icon: Chat, href: "/chat" },
    { label: "Ducanator", icon: Ducat, href: "/ducanator", iconScale: 1.15 },
    { label: "Tools", icon: Wrench, href: "/tools" },
  ];

  let statusOpen = $state(false);
  /** False while Windows reports no network: a banner explains why live data isn't updating. */
  let online = $state(typeof navigator === "undefined" ? true : navigator.onLine);

  /** The riven overlay window shows only its page: no nav, no status, transparent. */
  const isOverlay = $derived(page.url.pathname.startsWith("/overlay"));

  // The main window has no system title bar (`decorations: false`): the top bar is the title bar,
  // with its own minimize / maximize / close buttons.
  let maximized = $state(false);

  onMount(() => {
    if (page.url.pathname.startsWith("/overlay")) return;
    startStatus();
    void startWhatsNew(welcome.open);
    const stopUpdates = startUpdateChecks();
    const win = getCurrentWindow();
    const sync = () => win.isMaximized().then((m) => (maximized = m)).catch(() => {});
    sync();
    const off = win.onResized(sync);
    return () => {
      stopUpdates();
      void off.then((f) => f());
    };
  });

  const win = () => getCurrentWindow();

  // Pages scroll inside .content, not the window, so a new page would open at the last page's
  // scroll position: start each page at the top.
  let contentEl = $state<HTMLElement | null>(null);
  afterNavigate(({ from, to }) => {
    if (contentEl && from?.url.pathname !== to?.url.pathname) contentEl.scrollTop = 0;
  });

  const status = $derived(pipeline.status);
  /** good: game running, no error · idle: game not running · bad: last capture failed */
  const tone = $derived(!status ? "idle" : status.lastError ? "bad" : status.gameRunning ? "good" : "idle");
  const label = $derived.by(() => {
    if (!status) return "Starting…";
    if (status.lastError) return "Capture problem";
    if (!status.gameRunning) return "Game not running";
    if (status.lastCapture) return `Updated ${time(status.lastCapture.capturedAtMs)}`;
    return "Waiting for first capture";
  });

  function closeOnOutside(e: MouseEvent) {
    if (statusOpen && !(e.target as HTMLElement).closest(".status")) statusOpen = false;
  }

  /** No browser right-click menu (Back, Reload, Inspect…) in the app; text fields keep theirs
   *  for cut, copy and paste. */
  function blockContextMenu(e: MouseEvent) {
    const field = (e.target as HTMLElement | null)?.closest("input, textarea, [contenteditable='true']");
    if (!field) e.preventDefault();
  }
</script>

<svelte:window
  onclick={closeOnOutside}
  oncontextmenu={blockContextMenu}
  ononline={() => (online = true)}
  onoffline={() => (online = false)}
/>

{#snippet navIcon(icon: Component | string, scale = 1)}
  {#if typeof icon === "string"}
    <MaskIcon class="nav-icon" src={icon} size={20} {scale} aria-hidden="true" />
  {:else}
    {@const Icon = icon}
    <Icon class="nav-icon" size={20} strokeWidth={1.75} style="transform: scale({scale})" aria-hidden="true" />
  {/if}
{/snippet}

{#if isOverlay}
  {@render children()}
{:else}
<div class="shell">
  <nav class="sidebar" aria-label="Main">
    <div class="brand" data-tauri-drag-region>
      <img class="mark" src="/lotus.png" alt="" />
      <span class="brand-text">Lotus</span>
    </div>
    <ul>
      {#each nav as item}
        <li>
          {#if item.href}
            <a href={item.href} class:active={page.url.pathname === item.href}>
              {@render navIcon(item.icon, item.iconScale)}{item.label}
            </a>
          {:else}
            <span class="disabled" title="Coming in a later milestone">
              {@render navIcon(item.icon, item.iconScale)}{item.label}<small>Soon</small>
            </span>
          {/if}
        </li>
      {/each}
    </ul>
  </nav>

  <div class="main">
    <header class="topbar" data-tauri-drag-region>
      <div class="drag" data-tauri-drag-region></div>
      <div class="status">
        <button class="pill {tone}" onclick={() => (statusOpen = !statusOpen)} aria-expanded={statusOpen}>
          <span class="dot"></span>{label}
        </button>
        {#if statusOpen && status}
          <div class="popover" role="dialog" aria-label="Capture status">
            <dl>
              <dt>Game</dt>
              <dd>{status.gameRunning ? "Running" : "Not running"}</dd>
              <dt>Last capture</dt>
              <dd>
                {#if status.lastCapture}
                  {time(status.lastCapture.capturedAtMs)} · {Math.round(status.lastCapture.bytes / 1024).toLocaleString()} KB
                  · {status.lastCapture.durationMs} ms
                {:else}
                  None this session
                {/if}
              </dd>
              <dt>Last log event</dt>
              <dd class="mono">{status.lastEvent ?? "None yet"}</dd>
            </dl>
            {#if status.lastError}
              <p class="error">{status.lastError}</p>
            {/if}
            <button class="primary" onclick={captureNow} disabled={pipeline.capturing || !status.gameRunning}>
              {pipeline.capturing ? "Scanning…" : "Capture now"}
            </button>
            <p class="hint">Captures happen automatically at login and at the end of each mission.</p>
          </div>
        {/if}
      </div>
      <div class="window-controls">
        <button class="win-btn" onclick={() => win().minimize()} aria-label="Minimize" title="Minimize">
          <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5.5h10" /></svg>
        </button>
        <button
          class="win-btn"
          onclick={() => win().toggleMaximize()}
          aria-label={maximized ? "Restore" : "Maximize"}
          title={maximized ? "Restore" : "Maximize"}
        >
          {#if maximized}
            <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M2.5 2.5V.5h7v7h-2M.5 2.5h7v7h-7z" /></svg>
          {:else}
            <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M.5.5h9v9h-9z" /></svg>
          {/if}
        </button>
        <button class="win-btn close" onclick={() => win().close()} aria-label="Close" title="Close">
          <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M.5.5l9 9M9.5.5l-9 9" /></svg>
        </button>
      </div>
    </header>
    {#if appUpdate.available && !appUpdate.dismissed}
      <div class="update" role="status">
        {#if appUpdate.installing}
          Updating to Lotus {appUpdate.available.version}{appUpdate.progress !== null ? ` · ${appUpdate.progress}%` : "…"} Lotus
          restarts when it's done.
        {:else}
          <span><b>Lotus {appUpdate.available.version}</b> is available{appUpdate.current ? ` (you have ${appUpdate.current})` : ""}.</span>
          <a href="https://github.com/t-2ddy/Lotus-Warframe-Companion-App/releases/latest" target="_blank" rel="noreferrer">What's new</a>
          <button class="update-now" onclick={installUpdate}>Update and restart</button>
          <button class="update-later" onclick={() => (appUpdate.dismissed = true)}>Later</button>
        {/if}
      </div>
    {/if}
    {#if !online}
      <div class="offline" role="status">
        No internet connection. Showing saved data; prices, fissures, arbitrations and riven listings update when
        you're back online. Inventory capture still works.
      </div>
    {/if}
    <main class="content" bind:this={contentEl}>
      {@render children()}
    </main>
    {#if welcome.open}<Welcome />{:else if whatsNew.open}<WhatsNew />{/if}
  </div>
</div>
{/if}

<style>
  /* Theme: flat dark navy and black, one solid blue accent. Dark only. */
  :global(:root) {
    --bg: #070a10;
    --surface: #0f1522;
    --surface-solid: #0f1522;
    --surface-2: #161e2e;
    --surface-hover: #1a2436;
    --text: #e4e9f2;
    --muted: #8691a5;
    --line: #1e2738;
    --line-strong: #2b3850;
    --accent: #3b82f6;
    --accent-hover: #2f6fde;
    --accent-soft: rgba(59, 130, 246, 0.18);
    --good: #34d399;
    --good-tint: rgba(52, 211, 153, 0.06);
    --bad: #f87171;
    --gold: #fbbf24;
    --helminth: #f472b6;
    --shadow: 0 6px 18px rgba(0, 0, 0, 0.4);
    --radius: 10px;
    font-family: "Segoe UI Variable Text", "Segoe UI", Inter, system-ui, sans-serif;
    font-size: 14px;
    color: var(--text);
    background: var(--bg);
    color-scheme: dark;
  }

  /* Only the content area scrolls: the window never does, even if something pokes out of it. */
  :global(html),
  :global(body) {
    overflow: hidden;
  }

  :global(body) {
    margin: 0;
    background: linear-gradient(180deg, #0b111d 0%, #070a10 55%, #05070b 100%);
    background-attachment: fixed;
  }

  :global(*) {
    box-sizing: border-box;
    scrollbar-width: thin;
    scrollbar-color: #2b3850 transparent;
  }

  :global(::selection) {
    background: rgba(59, 130, 246, 0.4);
  }

  .shell {
    display: grid;
    grid-template-columns: 232px 1fr;
    height: 100vh;
  }

  .sidebar {
    background: #0a0e17;
    border-right: 1px solid var(--line);
    padding: 18px 10px;
    overflow-y: auto;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 10px 20px;
  }

  .mark {
    width: 36px;
    height: auto;
  }

  .brand-text {
    font-weight: 700;
    font-size: 20px;
    letter-spacing: 0.2px;
  }

  .sidebar ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 4px;
  }

  .sidebar a,
  .sidebar .disabled {
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 10px 12px;
    border-radius: 6px;
    font-size: 15px;
    color: var(--text);
    text-decoration: none;
  }

  .sidebar a:hover {
    background: var(--surface-2);
  }

  .sidebar a.active {
    background: var(--surface-2);
    font-weight: 600;
    box-shadow: inset 3px 0 0 var(--accent);
  }

  .sidebar .disabled {
    color: var(--muted);
    cursor: default;
  }

  .sidebar :global(.nav-icon) {
    flex-shrink: 0;
    color: white;
  }

  .sidebar small {
    margin-left: auto;
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--muted);
    border: 1px solid var(--line);
    border-radius: 4px;
    padding: 0 5px;
  }

  .main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  /* Doubles as the window's title bar: drag it to move, double-click to maximize. */
  .topbar {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 40px;
    padding-left: 20px;
    border-bottom: 1px solid var(--line);
    background: #0a0e17;
    user-select: none;
  }

  .drag {
    flex: 1;
    align-self: stretch;
  }

  .window-controls {
    display: flex;
    align-self: stretch;
  }

  .win-btn {
    width: 46px;
    border: none;
    background: transparent;
    color: var(--muted);
    display: grid;
    place-items: center;
    cursor: default;
  }

  .win-btn svg {
    width: 10px;
    height: 10px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1;
  }

  .win-btn:hover {
    background: var(--surface-hover);
    color: var(--text);
  }

  .win-btn.close:hover {
    background: #c42b1c;
    color: white;
  }

  .update {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 14px;
    padding: 8px 20px;
    font-size: 13px;
    background: var(--accent-soft);
    border-bottom: 1px solid var(--accent);
  }

  .update a {
    color: var(--text);
  }

  .update button {
    font: inherit;
    border-radius: 6px;
    padding: 4px 12px;
    cursor: pointer;
  }

  .update-now {
    margin-left: auto;
    border: none;
    background: var(--accent);
    color: white;
    font-weight: 600;
  }

  .update-later {
    background: transparent;
    border: 1px solid var(--line-strong);
    color: var(--muted);
  }

  .offline {
    padding: 8px 20px;
    font-size: 13px;
    background: rgba(251, 191, 36, 0.12);
    border-bottom: 1px solid rgba(251, 191, 36, 0.35);
    color: var(--gold);
  }

  .content {
    flex: 1;
    overflow-y: auto;
    padding: 20px;
  }

  .status {
    position: relative;
  }

  .pill {
    font: inherit;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 12px;
    border-radius: 6px;
    border: 1px solid var(--line);
    background: var(--surface);
    color: var(--text);
    cursor: pointer;
  }

  .pill:hover {
    border-color: var(--line-strong);
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--muted);
  }

  .pill.good .dot {
    background: var(--good);
  }

  .pill.bad .dot {
    background: var(--bad);
  }

  .popover {
    position: absolute;
    right: 0;
    top: calc(100% + 8px);
    width: 360px;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    padding: 16px;
    z-index: 10;
    box-shadow: var(--shadow);
  }

  .popover dl {
    display: grid;
    grid-template-columns: 110px 1fr;
    gap: 6px 12px;
    margin: 0 0 14px;
  }

  .popover dt {
    color: var(--muted);
  }

  .popover dd {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .mono {
    font-family: "Cascadia Mono", Consolas, monospace;
    font-size: 12px;
  }

  .error {
    color: var(--bad);
    margin: 0 0 12px;
    font-size: 13px;
    overflow-wrap: anywhere;
  }

  .hint {
    color: var(--muted);
    font-size: 12px;
    margin: 10px 0 0;
  }

  .primary {
    font: inherit;
    font-weight: 600;
    padding: 7px 16px;
    border-radius: 6px;
    border: none;
    background: var(--accent);
    color: white;
    cursor: pointer;
  }

  .primary:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .primary:disabled {
    opacity: 0.45;
    cursor: default;
  }
</style>
