<script lang="ts">
  /**
   * Where the riven data (stat values, dispositions) stands. Full: date, source and a "Check for
   * updates" button (Tools page). Compact: only a line while new data is being fetched.
   */
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";

  type DataStatus = {
    state: "idle" | "checking" | "ready" | "failed";
    built: string;
    source: "bundled" | "downloaded";
    checkedAt: number | null;
    error: string | null;
  };

  let { compact = false }: { compact?: boolean } = $props();
  let status = $state<DataStatus | null>(null);

  onMount(() => {
    invoke<DataStatus>("riven_data_status").then((s) => (status = s)).catch(() => {});
    const off = listen<DataStatus>("riven-data", (e) => (status = e.payload));
    return () => {
      off.then((f) => f());
    };
  });

  async function check() {
    status = await invoke<DataStatus>("check_riven_data");
  }

  const checking = $derived(status?.state === "checking");
  const checked = $derived(
    status?.checkedAt ? new Date(status.checkedAt).toLocaleString([], { dateStyle: "medium", timeStyle: "short" }) : null,
  );
</script>

{#if compact}
  {#if checking}
    <div class="status compact">
      <div class="bar" role="progressbar" aria-label="Fetching riven data"><span></span></div>
      <span>Fetching riven data…</span>
    </div>
  {/if}
{:else if status}
  <div class="status">
    {#if checking}
      <div class="bar" role="progressbar" aria-label="Fetching riven data"><span></span></div>
      <span>Fetching riven data…</span>
    {:else if status.state === "failed"}
      <span class="bad">Couldn't reach the riven data source; using the copy from {status.built}.</span>
    {:else}
      <span>
        Riven data: <b>{status.built}</b> ({status.source === "downloaded" ? "downloaded" : "bundled with the app"}){#if checked}
          · checked {checked}{/if}
      </span>
    {/if}
    <button onclick={check} disabled={checking}>Check for updates</button>
  </div>
{/if}

<style>
  .status {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    margin-top: 12px;
    font-size: 13px;
    color: var(--muted);
  }

  .status b {
    color: var(--text);
    font-weight: 600;
  }

  .status button {
    margin-left: auto;
  }

  .compact {
    margin: 0 0 12px;
  }

  .bad {
    color: var(--bad);
  }

  /* A flat sliding bar, no glow. */
  .bar {
    width: 120px;
    height: 3px;
    overflow: hidden;
    border-radius: 2px;
    background: var(--accent-soft);
  }

  .bar span {
    display: block;
    width: 35%;
    height: 100%;
    background: var(--accent);
    animation: slide 1.1s ease-in-out infinite;
  }

  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(290%);
    }
  }
</style>
