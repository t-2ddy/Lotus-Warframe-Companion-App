<script lang="ts">
  import { goto } from "$app/navigation";
  import { closeWelcome } from "$lib/welcomeState.svelte";

  function toTools() {
    closeWelcome();
    goto("/tools");
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") closeWelcome();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && closeWelcome()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="welcome-title">
    <header>
      <img src="/lotus.png" alt="" />
      <h1 id="welcome-title">Welcome to Lotus</h1>
    </header>
    <p class="lead">A companion for Warframe that keeps track of your inventory on its own. Here's how to get going.</p>

    <ol class="steps">
      <li>
        <b>Keep Lotus open and log in to Warframe.</b>
        Your inventory is read when you log in and after each mission. Foundry, Inventory, Mastery Helper, Rivens and
        Ducanator fill in after the first one.
      </li>
      <li>
        <b>For the in-game overlays, play in Borderless or Windowed mode</b> (Warframe: Options → Display). Exclusive
        fullscreen draws over every overlay.
      </li>
      <li>
        <b>Turn on the overlays you want in Tools.</b> The riven overlay grades riven cards from chat links and the
        reroll screen. The relic rewards overlay shows prices on the Void Fissure reward screen. Both are off until
        you switch them on.
      </li>
    </ol>

    <section class="reads">
      <h2>What Lotus reads</h2>
      <ul>
        <li>The game's log file (<code>EE.log</code>), to know when you log in or finish a mission.</li>
        <li>Your inventory from the game's memory, <b>read-only</b>. Lotus never changes the game or sends it input.</li>
        <li>Small areas of the screen (riven cards, reward names), only while an overlay is on.</li>
      </ul>
      <p>
        Your data stays on this PC. Lotus only downloads public data: item info, warframe.market prices, the
        arbitration schedule and the world state.
      </p>
    </section>

    <p class="risk">
      <b>Use at your own risk.</b> Lotus is a free, unofficial community tool, not made or supported by Digital
      Extremes.
    </p>

    <footer>
      <button class="secondary" onclick={toTools}>Set up overlays</button>
      <button class="primary" onclick={closeWelcome}>Get started</button>
    </footer>
  </div>
</div>

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
    width: min(620px, 100%);
    max-height: 100%;
    overflow-y: auto;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    padding: 24px 28px;
    box-shadow: var(--shadow);
  }

  header {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  header img {
    width: 40px;
  }

  h1 {
    margin: 0;
    font-size: 24px;
  }

  .lead {
    color: var(--muted);
    margin: 10px 0 16px;
  }

  .steps {
    margin: 0;
    padding-left: 20px;
    display: grid;
    gap: 10px;
    line-height: 1.5;
  }

  .steps b {
    display: block;
  }

  .reads {
    margin-top: 18px;
    padding: 12px 16px;
    background: var(--surface-2);
    border-radius: 8px;
    font-size: 13px;
    line-height: 1.5;
  }

  .reads h2 {
    margin: 0 0 6px;
    font-size: 14px;
  }

  .reads ul {
    margin: 0 0 8px;
    padding-left: 18px;
  }

  .reads p {
    margin: 0;
    color: var(--muted);
  }

  code {
    font-size: 12px;
  }

  .risk {
    margin: 14px 0 0;
    font-size: 13px;
    color: var(--muted);
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    margin-top: 20px;
  }

  button {
    font: inherit;
    font-weight: 600;
    padding: 8px 18px;
    border-radius: 6px;
    cursor: pointer;
  }

  .primary {
    border: none;
    background: var(--accent);
    color: white;
  }

  .primary:hover {
    background: var(--accent-hover);
  }

  .secondary {
    background: var(--surface-2);
    border: 1px solid var(--line-strong);
    color: var(--text);
  }

  .secondary:hover {
    background: var(--surface-hover);
  }
</style>
