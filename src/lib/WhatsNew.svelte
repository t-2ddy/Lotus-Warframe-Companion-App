<script lang="ts">
  import { closeWhatsNew, whatsNew } from "$lib/whatsNewState.svelte";

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") closeWhatsNew();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && closeWhatsNew()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="whats-new-title">
    <header>
      <img src="/lotus.png" alt="" />
      <h1 id="whats-new-title">Lotus updated{whatsNew.version ? ` to ${whatsNew.version}` : ""}</h1>
    </header>

    {#each whatsNew.notes as note (note.version)}
      <section>
        <h2>Fixed{whatsNew.notes.length > 1 ? ` in ${note.version}` : ""}</h2>
        <ul>
          {#each note.fixes as fix, i (i)}
            <li>{fix}</li>
          {/each}
        </ul>
      </section>
    {/each}

    <footer>
      <button class="primary" onclick={closeWhatsNew}>Got it</button>
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
    width: min(520px, 100%);
    max-height: 100%;
    overflow-y: auto;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    padding: 22px 26px;
    box-shadow: var(--shadow);
  }

  header {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  header img {
    width: 32px;
  }

  h1 {
    margin: 0;
    font-size: 20px;
  }

  section {
    margin-top: 14px;
  }

  h2 {
    margin: 0 0 6px;
    font-size: 14px;
    color: var(--muted);
  }

  ul {
    margin: 0;
    padding-left: 20px;
    display: grid;
    gap: 6px;
    line-height: 1.45;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    margin-top: 20px;
  }

  button {
    font: inherit;
    font-weight: 600;
    padding: 8px 18px;
    border-radius: 6px;
    cursor: pointer;
    border: none;
    background: var(--accent);
    color: white;
  }

  button:hover {
    background: var(--accent-hover);
  }
</style>
