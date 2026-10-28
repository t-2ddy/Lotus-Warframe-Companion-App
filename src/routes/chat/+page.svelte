<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import Copy from "@lucide/svelte/icons/copy";
  import Check from "@lucide/svelte/icons/check";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash from "@lucide/svelte/icons/trash-2";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import X from "@lucide/svelte/icons/x";
  import Heading from "@lucide/svelte/icons/heading";
  import EMOJI from "$lib/emoji.json";

  // Saved lines to paste into the game's chats: one list per tab, each line with its own Copy
  // button; Notes is one free text box. Kept in this app only (localStorage), like Foundry
  // favorites.
  type TabId = "trade" | "recruit" | "notes";
  /** `header`: an optional title shown above the line (not copied); `undefined` = none. */
  type Line = { id: number; text: string; header?: string };

  const TABS: { id: TabId; label: string; placeholder: string }[] = [
    { id: "trade", label: "Trade", placeholder: "WTS [Saryn Prime] set 120p" },
    { id: "recruit", label: "Recruit", placeholder: "LF2M Eidolon hunts, have Volt" },
    { id: "notes", label: "Notes", placeholder: "Prices, trade contacts, what to farm next…" },
  ];

  /** Longest message the game's Trade chat takes. */
  const TRADE_LIMIT = 180;

  const LINES_KEY = "chat.lines";
  const NOTES_KEY = "chat.notes";
  const TAB_KEY = "chat.tab";

  /** The Notes text; notes saved as lines (before Notes was one box) become its lines. */
  function loadNotes(): string {
    try {
      const saved = localStorage.getItem(NOTES_KEY);
      if (saved !== null) return saved;
      const old = JSON.parse(localStorage.getItem(LINES_KEY) ?? "{}").notes;
      return Array.isArray(old) ? old.map((l) => l?.text).filter((t) => typeof t === "string" && t).join("\n") : "";
    } catch {
      return "";
    }
  }

  function loadLines(): Record<TabId, Line[]> {
    const empty = { trade: [], recruit: [], notes: [] };
    try {
      const saved = JSON.parse(localStorage.getItem(LINES_KEY) ?? "{}");
      const list = (v: unknown): Line[] =>
        Array.isArray(v) ? v.filter((l) => typeof l?.text === "string").map((l, i) => ({ id: i + 1, text: l.text, header: typeof l.header === "string" ? l.header : undefined })) : [];
      return { trade: list(saved.trade), recruit: list(saved.recruit), notes: [] };
    } catch {
      return empty;
    }
  }
  function loadTab(): TabId {
    try {
      const saved = localStorage.getItem(TAB_KEY);
      return TABS.some((t) => t.id === saved) ? (saved as TabId) : "trade";
    } catch {
      return "trade";
    }
  }

  let lines = $state(loadLines());
  /** The Notes box, shaped like a line (its own id) so "[" suggestions work in it too. */
  const NOTES_ID = 0;
  let notes = $state<Line>({ id: NOTES_ID, text: loadNotes() });
  let tab = $state<TabId>(loadTab());
  let nextId = Math.max(0, ...Object.values(lines).flat().map((l) => l.id)) + 1;
  /** The line whose Copy button shows "Copied" for a moment. */
  let copied = $state<number | null>(null);
  let copiedTimer: ReturnType<typeof setTimeout> | undefined;

  const current = $derived(TABS.find((t) => t.id === tab)!);

  function save() {
    try {
      localStorage.setItem(LINES_KEY, JSON.stringify(lines));
    } catch {}
  }

  function saveNotes() {
    try {
      localStorage.setItem(NOTES_KEY, notes.text);
    } catch {}
  }

  function pickTab(id: TabId) {
    tab = id;
    try {
      localStorage.setItem(TAB_KEY, id);
    } catch {}
  }

  function addLine() {
    const id = nextId++;
    lines[tab].push({ id, text: "" });
    save();
    // Focus the new field once it's drawn.
    queueMicrotask(() => document.getElementById(`line-${id}`)?.focus());
  }

  /** Empties a line but keeps its field, ready to type into. */
  function clearLine(line: Line) {
    line.text = "";
    suggest = null;
    save();
    document.getElementById(`line-${line.id}`)?.focus();
  }

  async function addHeader(line: Line) {
    line.header = "";
    save();
    await tick();
    document.getElementById(`header-${line.id}`)?.focus();
  }

  function removeHeader(line: Line) {
    line.header = undefined;
    save();
  }

  function removeLine(id: number) {
    lines[tab] = lines[tab].filter((l) => l.id !== id);
    save();
  }

  // Item links: the game turns "[Saryn Prime Set]" into a link, so typing "[" suggests item
  // names (warframe.market's list) and the user's own rivens ("[Torid Crita-acriata]") to spell
  // it right.
  const MAX_SUGGESTIONS = 8;
  type Item = { name: string; lower: string; riven: boolean };
  let marketItems: Item[] = [];
  let rivenItems: Item[] = [];
  const toItem = (name: string, riven: boolean): Item => ({ name, lower: name.toLowerCase(), riven });
  onMount(() => {
    invoke<string[]>("item_names")
      .then((names) => (marketItems = names.map((n) => toItem(n, false))))
      .catch(() => {});
    invoke<{ rivens: { weapon: string; name: string }[] }>("get_rivens")
      .then((page) => {
        const names = new Set(page.rivens.filter((r) => r.name).map((r) => `${r.weapon} ${r.name}`));
        rivenItems = [...names].map((n) => toItem(n, true));
        owned = new Set([...names].map((n) => n.toLowerCase()));
        for (const n of owned) seenRivens.add(n);
        try {
          localStorage.setItem(SEEN_RIVENS_KEY, JSON.stringify([...seenRivens]));
        } catch {}
        rivensLoaded = true;
      })
      .catch(() => {});
  });

  // A linked riven the user no longer has (sold, traded, or rerolled into a new name) is marked
  // red. Every riven name seen owned is remembered, so "[...]" text that was once one of them
  // but isn't owned now is known to be a riven gone, not an item.
  const SEEN_RIVENS_KEY = "chat.seenRivens";
  const seenRivens: Set<string> = (() => {
    try {
      const saved = JSON.parse(localStorage.getItem(SEEN_RIVENS_KEY) ?? "[]");
      return new Set(Array.isArray(saved) ? saved.filter((n) => typeof n === "string") : []);
    } catch {
      return new Set<string>();
    }
  })();
  let owned = $state(new Set<string>());
  /** Nothing is marked until the owned rivens have loaded (or every riven would look gone). */
  let rivensLoaded = $state(false);

  function rivenGone(name: string): boolean {
    const key = name.trim().toLowerCase();
    return rivensLoaded && seenRivens.has(key) && !owned.has(key);
  }

  /** The line split into plain text and "[...]" links, each link marked if it's a riven gone. */
  function segments(text: string): { text: string; gone: boolean }[] {
    const out: { text: string; gone: boolean }[] = [];
    let last = 0;
    for (const m of text.matchAll(/\[([^\]]*)\]/g)) {
      if (m.index > last) out.push({ text: text.slice(last, m.index), gone: false });
      out.push({ text: m[0], gone: rivenGone(m[1]) });
      last = m.index + m[0].length;
    }
    if (last < text.length) out.push({ text: text.slice(last), gone: false });
    return out;
  }

  /** Rivens linked in a line that the user no longer owns. */
  function goneRivens(text: string): string[] {
    return [...new Set([...text.matchAll(/\[([^\]]*)\]/g)].map((m) => m[1]).filter(rivenGone))];
  }

  /** The open suggestion list: which line, where its "[" is, what's typed after it. */
  let suggest = $state<{
    lineId: number;
    start: number;
    matches: Item[];
    active: number;
    /** Notes only: where to open, at the cursor. */
    at?: { top: number; left: number };
  } | null>(null);

  /** Names with every typed word in them: names starting with the text first, then ones with a
   *  word starting with it, then the rest; owned rivens, then shorter names first within each. */
  function findItems(query: string): Item[] {
    const q = query.trim().toLowerCase();
    if (!q) return [];
    const words = q.split(/\s+/);
    const rank = (lower: string) => (lower.startsWith(q) ? 0 : lower.includes(` ${words[0]}`) ? 1 : 2);
    return [...rivenItems, ...marketItems]
      .filter((n) => words.every((w) => n.lower.includes(w)))
      .map((n) => ({ item: n, rank: rank(n.lower) }))
      .sort(
        (a, b) =>
          a.rank - b.rank ||
          Number(b.item.riven) - Number(a.item.riven) ||
          a.item.name.length - b.item.name.length ||
          a.item.lower.localeCompare(b.item.lower),
      )
      .slice(0, MAX_SUGGESTIONS)
      .map((n) => n.item);
  }

  /** Opens, updates or closes the list from the text before the cursor: an unclosed "[". */
  function updateSuggest(line: Line, input: HTMLTextAreaElement) {
    const caret = input.selectionStart ?? line.text.length;
    const before = line.text.slice(0, caret);
    const start = before.lastIndexOf("[");
    // A "[" on an earlier line of the notes doesn't count.
    if (start < 0 || before.indexOf("]", start) >= 0 || before.indexOf("\n", start) >= 0) {
      suggest = null;
      return;
    }
    const matches = findItems(before.slice(start + 1));
    const at = line.id === NOTES_ID ? caretPoint(input) : undefined;
    suggest = matches.length ? { lineId: line.id, start, matches, active: 0, at } : null;
  }

  /** Where the cursor is inside a textarea (just under its line), for the Notes box, where the
   *  list opens at the cursor rather than under the whole box. Measured with a hidden copy of
   *  the field holding the text up to the cursor. */
  function caretPoint(area: HTMLTextAreaElement): { top: number; left: number } {
    const style = getComputedStyle(area);
    const copy = document.createElement("div");
    for (const p of ["font", "letterSpacing", "lineHeight", "padding", "border", "boxSizing", "width"] as const) {
      copy.style[p] = style[p];
    }
    Object.assign(copy.style, { position: "absolute", visibility: "hidden", whiteSpace: "pre-wrap", overflowWrap: "anywhere" });
    copy.textContent = area.value.slice(0, area.selectionStart ?? 0);
    const mark = document.createElement("span");
    mark.textContent = "​";
    copy.append(mark);
    document.body.append(copy);
    const lineHeight = parseFloat(style.lineHeight) || 20;
    const point = { top: mark.offsetTop + lineHeight - area.scrollTop + 4, left: Math.min(mark.offsetLeft, area.clientWidth - 420) };
    copy.remove();
    return { top: point.top, left: Math.max(8, point.left) };
  }

  /** How an item is written in chat: a set links its item and says "set" after it
   *  ("[Saryn Prime] set"). */
  function chatLink(item: Item): string {
    const name = item.name;
    const set = item.riven ? null : name.match(/^(.*) Set$/);
    return set ? `[${set[1]}] set` : `[${name}]`;
  }

  /** Replaces "[typed" with the item's link and puts the cursor after it. */
  async function pickItem(line: Line, item: Item) {
    if (!suggest) return;
    const input = document.getElementById(`line-${line.id}`) as HTMLTextAreaElement | null;
    const caret = input?.selectionStart ?? line.text.length;
    // A "]" already right after the cursor is reused, not doubled.
    const after = line.text.slice(caret).replace(/^\]/, "");
    const link = chatLink(item);
    line.text = line.text.slice(0, suggest.start) + link + after;
    suggest = null;
    if (line.id === NOTES_ID) saveNotes();
    else save();
    await tick();
    const at = line.text.length - after.length;
    input?.focus();
    input?.setSelectionRange(at, at);
  }

  /** Keys for an open suggestion list; true if the key was used. */
  function suggestKey(e: KeyboardEvent, line: Line): boolean {
    const open = suggest?.lineId === line.id ? suggest : null;
    if (!open) return false;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      const n = open.matches.length;
      open.active = (open.active + (e.key === "ArrowDown" ? 1 : n - 1)) % n;
    } else if (e.key === "Enter" || e.key === "Tab") {
      void pickItem(line, open.matches[open.active]);
    } else if (e.key === "Escape") {
      suggest = null;
    } else {
      return false;
    }
    e.preventDefault();
    return true;
  }

  function onKey(e: KeyboardEvent, line: Line) {
    if (suggestKey(e, line)) return;
    if (e.key === "Enter") {
      // Copies; a chat line never gets a line break.
      e.preventDefault();
      void copy(line);
    }
  }

  // In-game emoji: the game shows ":smile:" as its picture (list and images from the wiki's Chat
  // page, see scripts/gen_emoji.py). A picked one goes in at the cursor of the line last edited.
  let emojiOpen = $state(false);
  let emojiSearch = $state("");
  const shownEmoji = $derived.by(() => {
    const q = emojiSearch.trim().toLowerCase();
    return q ? EMOJI.filter((e) => e.code.includes(q) || e.name.toLowerCase().includes(q)) : EMOJI;
  });
  /** The line last edited and where its cursor was. */
  let target: { id: number; caret: number } | null = null;
  function remember(line: Line, input: HTMLTextAreaElement) {
    target = { id: line.id, caret: input.selectionStart ?? line.text.length };
  }

  async function addEmoji(code: string) {
    // A line still being typed in (the dropdown doesn't take focus) wins over the remembered one.
    const active = document.activeElement;
    if (active instanceof HTMLTextAreaElement && active.id.startsWith("line-")) {
      target = { id: Number(active.id.slice(5)), caret: active.selectionStart ?? active.value.length };
    }
    const list = lines[tab];
    let line = list.find((l) => l.id === target?.id) ?? list[list.length - 1];
    if (!line) {
      addLine();
      line = lines[tab][lines[tab].length - 1];
    }
    const caret = target?.id === line.id ? Math.min(target.caret, line.text.length) : line.text.length;
    const before = line.text.slice(0, caret);
    const after = line.text.slice(caret);
    // Its own word: a space before it unless the line starts there or already has one.
    const emoji = `${before && !before.endsWith(" ") ? " " : ""}:${code}:`;
    line.text = before + emoji + after;
    save();
    const at = caret + emoji.length;
    target = { id: line.id, caret: at };
    await tick();
    const input = document.getElementById(`line-${line.id}`) as HTMLTextAreaElement | null;
    input?.setSelectionRange(at, at);
  }

  function closeEmojiOutside(e: MouseEvent) {
    if (emojiOpen && !(e.target as HTMLElement).closest(".emoji-menu")) emojiOpen = false;
  }

  async function copy(line: Line) {
    if (!line.text) return;
    try {
      await navigator.clipboard.writeText(line.text);
      copied = line.id;
      clearTimeout(copiedTimer);
      copiedTimer = setTimeout(() => (copied = null), 1500);
    } catch {}
  }
</script>

<div class="toolbar">
  <div class="tabs" role="tablist">
    {#each TABS as t}
      <button role="tab" aria-selected={tab === t.id} class:active={tab === t.id} onclick={() => pickTab(t.id)}>
        {t.label}{#if t.id !== "notes"}<span class="count">{lines[t.id].length}</span>{/if}
      </button>
    {/each}
  </div>
</div>

<svelte:window onclick={closeEmojiOutside} />

<!-- The "[" item list for a line (or the Notes box, where it opens at the cursor). -->
{#snippet suggestions(line: Line)}
  {#if suggest?.lineId === line.id}
    <ul
      id="suggest-{line.id}"
      class="suggest"
      role="listbox"
      aria-label="Items"
      style={suggest.at ? `top: ${suggest.at.top}px; left: ${suggest.at.left}px` : undefined}
    >
      {#each suggest.matches as item, i}
        <!-- mousedown, not click: the field would blur first and close the list. -->
        <li
          role="option"
          aria-selected={i === suggest.active}
          class:active={i === suggest.active}
          onmousedown={(e) => {
            e.preventDefault();
            void pickItem(line, item);
          }}
          onmouseenter={() => suggest && (suggest.active = i)}
        >
          <span class="link">{chatLink(item)}</span>
          {#if item.riven}<span class="tag">Your riven</span>{/if}
        </li>
      {/each}
      <li class="suggest-hint" aria-hidden="true">↑↓ choose · Enter or Tab insert · Esc close</li>
    </ul>
  {/if}
{/snippet}

{#if tab === "notes"}
  <div class="notes-wrap">
    <textarea
      id="line-{NOTES_ID}"
      class="notes"
      bind:value={notes.text}
      oninput={(e) => {
        saveNotes();
        updateSuggest(notes, e.currentTarget);
      }}
      onkeydown={(e) => suggestKey(e, notes)}
      onclick={(e) => updateSuggest(notes, e.currentTarget)}
      onscroll={() => (suggest = null)}
      onblur={() => (suggest = null)}
      placeholder={current.placeholder}
      aria-label="Notes"
      role="combobox"
      aria-controls="suggest-{NOTES_ID}"
      aria-autocomplete="list"
      aria-expanded={suggest?.lineId === NOTES_ID}
    ></textarea>
    {@render suggestions(notes)}
  </div>
{:else}
<div class="workspace">
<div class="list-col">
{#if lines[tab].length === 0}
  <div class="empty">
    <p>No {current.label.toLowerCase()} lines yet.</p>
    <button class="add" onclick={addLine}><Plus size={16} aria-hidden="true" />Add your first line</button>
  </div>
{:else}
  <ul class="lines">
    {#each lines[tab] as line (line.id)}
      {@const gone = goneRivens(line.text)}
      <li>
        <span class="field">
        {#if line.header !== undefined}
          <span class="header-row">
            <input
              id="header-{line.id}"
              class="header"
              bind:value={line.header}
              oninput={save}
              placeholder="Header"
              aria-label="Header"
              spellcheck="false"
            />
            <button class="header-remove" onclick={() => removeHeader(line)} aria-label="Remove header" title="Remove header">
              <X size={14} aria-hidden="true" />
            </button>
          </span>
        {/if}
        <span class="stack">
        <textarea
          id="line-{line.id}"
          rows="1"
          class:masked={gone.length > 0}
          aria-describedby={gone.length ? `gone-${line.id}` : undefined}
          bind:value={line.text}
          oninput={(e) => {
            // One chat message: pasted line breaks become spaces. The field wraps instead.
            if (/[\r\n]/.test(line.text)) {
              const caret = e.currentTarget.selectionStart;
              line.text = line.text.replace(/\r?\n|\r/g, " ");
              e.currentTarget.value = line.text;
              e.currentTarget.setSelectionRange(caret, caret);
            }
            save();
            updateSuggest(line, e.currentTarget);
          }}
          onkeydown={(e) => onKey(e, line)}
          onclick={(e) => updateSuggest(line, e.currentTarget)}
          onblur={(e) => {
            suggest = null;
            remember(line, e.currentTarget);
          }}
          placeholder={current.placeholder}
          aria-label="{current.label} line"
          role="combobox"
          aria-controls="suggest-{line.id}"
          aria-autocomplete="list"
          aria-expanded={suggest?.lineId === line.id}
          spellcheck="false"
        ></textarea>
        {#if gone.length}
          <!-- The same text drawn over the field (whose own text is hidden), with gone rivens red. -->
          <span class="mirror" aria-hidden="true"
            >{#each segments(line.text) as part}{#if part.gone}<span class="gone">{part.text}</span
                >{:else}{part.text}{/if}{/each}</span
          >
        {/if}
        {#if line.text}
          <button
            class="clear"
            onmousedown={(e) => e.preventDefault()}
            onclick={() => clearLine(line)}
            aria-label="Clear text"
            title="Clear text"
          >
            <X size={16} aria-hidden="true" />
          </button>
        {/if}
        </span>
        {#if gone.length}
          <span class="gone-warn" id="gone-{line.id}" role="status">
            <TriangleAlert size={14} aria-hidden="true" />
            {gone.length === 1 ? "Riven" : "Rivens"} no longer owned: {gone.join(", ")}
          </span>
        {/if}
        </span>
        {@render suggestions(line)}
        {#if tab === "trade"}
          {@const over = line.text.length > TRADE_LIMIT}
          <span
            class="chars"
            class:over
            title={over ? `Over Trade chat's ${TRADE_LIMIT}-character limit` : "Characters"}
          >
            {#if over}<TriangleAlert size={14} aria-hidden="true" />{/if}{line.text.length}/{TRADE_LIMIT}
          </span>
        {:else}
          <span class="chars" title="Characters">{line.text.length}</span>
        {/if}
        <button
          class="copy"
          class:done={copied === line.id}
          onclick={() => copy(line)}
          disabled={!line.text}
          title="Copy to clipboard (or press Enter in the field)"
        >
          {#if copied === line.id}
            <Check size={18} aria-hidden="true" />Copied
          {:else}
            <Copy size={18} aria-hidden="true" />Copy
          {/if}
        </button>
        {#if line.header === undefined}
          <button class="remove" onclick={() => addHeader(line)} aria-label="Add header" title="Add header">
            <Heading size={18} aria-hidden="true" />
          </button>
        {/if}
        <button class="remove" onclick={() => removeLine(line.id)} aria-label="Delete line" title="Delete line">
          <Trash size={18} aria-hidden="true" />
        </button>
      </li>
    {/each}
  </ul>
  <button class="add after" onclick={addLine}><Plus size={16} aria-hidden="true" />Add line</button>
{/if}
</div>

<div class="emoji-col">
  <div class="emoji-menu">
    <button
      class="emoji-toggle"
      class:open={emojiOpen}
      aria-expanded={emojiOpen}
      aria-controls="emoji-panel"
      onmousedown={(e) => e.preventDefault()}
      onclick={() => (emojiOpen = !emojiOpen)}
      title="Add an in-game emoji to the line you're editing"
    >
      <img src="/emoji/smile.png" alt="" />Emoji<ChevronDown size={16} aria-hidden="true" />
    </button>
    {#if emojiOpen}
      <div id="emoji-panel" class="emoji-panel" role="dialog" aria-label="Emoji">
        <input
          type="search"
          placeholder="Search emoji"
          bind:value={emojiSearch}
          aria-label="Search emoji"
          onkeydown={(e) => e.key === "Escape" && (emojiOpen = false)}
        />
        <div class="emoji-grid">
          {#each shownEmoji as em (em.code)}
            <!-- mousedown, not click: keeps the line's cursor where it was. -->
            <button
              class="emoji"
              title=":{em.code}:{em.shortcut ? `  (${em.shortcut})` : ''}"
              aria-label={em.name}
              onmousedown={(e) => {
                e.preventDefault();
                void addEmoji(em.code);
              }}
            >
              <img src="/emoji/{em.code}.png" alt="" loading="lazy" />
            </button>
          {:else}
            <p class="emoji-none">No emoji named “{emojiSearch}”.</p>
          {/each}
        </div>
      </div>
    {/if}
  </div>
</div>
</div>
{/if}

<style>
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 18px;
    padding-bottom: 16px;
    border-bottom: 1px solid var(--line);
  }

  .tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  button,
  input {
    font: inherit;
    color: var(--text);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 6px 12px;
  }

  button {
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  button:hover:not(:disabled) {
    background: var(--surface-hover);
    border-color: var(--line-strong);
  }

  button:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .tabs {
    gap: 8px;
  }

  .tabs button {
    padding: 10px 24px;
    font-size: 15px;
    gap: 8px;
  }

  .tabs button.active {
    background: var(--accent);
    border-color: var(--accent);
    color: white;
    font-weight: 600;
  }

  .count {
    font-size: 13px;
    opacity: 0.7;
  }

  .add {
    background: var(--accent);
    border-color: var(--accent);
    color: white;
    font-weight: 600;
  }

  .add:hover {
    background: var(--accent-hover) !important;
    border-color: var(--accent-hover) !important;
  }

  /* The line list, with the emoji dropdown to its right. */
  .workspace {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 16px;
    align-items: start;
  }

  .emoji-col {
    position: sticky;
    top: 0;
  }

  .emoji-menu {
    position: relative;
  }

  .emoji-toggle {
    padding: 8px 12px;
  }

  .emoji-toggle img {
    width: 20px;
    height: 20px;
  }

  .emoji-toggle.open {
    border-color: var(--accent);
  }

  .emoji-toggle :global(svg) {
    transition: transform 150ms ease-out;
  }

  .emoji-toggle.open :global(svg) {
    transform: rotate(180deg);
  }

  .emoji-panel {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 6;
    width: 352px;
    padding: 10px;
    background: var(--surface-solid);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }

  .emoji-panel input {
    width: 100%;
    margin-bottom: 8px;
    background: var(--surface-2);
    outline: none;
  }

  .emoji-panel input:focus {
    border-color: var(--accent);
  }

  .emoji-grid {
    display: grid;
    grid-template-columns: repeat(8, 1fr);
    gap: 2px;
    max-height: 320px;
    overflow-y: auto;
  }

  .emoji {
    padding: 4px;
    justify-content: center;
    background: transparent;
    border-color: transparent;
  }

  .emoji img {
    width: 28px;
    height: 28px;
  }

  .emoji-none {
    grid-column: 1 / -1;
    margin: 6px 2px;
    color: var(--muted);
    font-size: 13px;
  }

  .notes-wrap {
    position: relative;
  }

  .notes {
    display: block;
    width: 100%;
    min-height: 360px;
    resize: vertical;
    font: inherit;
    line-height: 1.5;
    color: var(--text);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 12px 14px;
    outline: none;
  }

  .notes:focus {
    border-color: var(--accent);
  }

  .add.after {
    margin-top: 10px;
  }

  .lines {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 10px;
  }

  .lines > li {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px;
    font-size: 15px;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }

  /* The text field with its clear button inside, at the right end. */
  .field {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  /* An optional title above the line: plain bold text until hovered or edited. */
  .header-row {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .lines input.header {
    flex: 1;
    min-width: 0;
    padding: 2px 6px;
    font-size: 18px;
    font-weight: 600;
    letter-spacing: 0.3px;
    background: transparent;
    border-color: transparent;
    outline: none;
  }

  .lines input.header:hover {
    border-color: var(--line);
  }

  .lines input.header:focus {
    border-color: var(--accent);
    background: var(--surface-2);
  }

  .header-remove {
    padding: 3px;
    border: none;
    background: transparent;
    color: var(--muted);
  }

  .header-remove:hover {
    color: var(--text);
  }

  /* Field and its mirror in one grid cell, exactly on top of each other. */
  .stack {
    position: relative;
    display: grid;
  }

  .stack > textarea,
  .mirror {
    grid-area: 1 / 1;
  }

  /* Must wrap exactly like the textarea: same box, font, spacing and breaking. */
  .mirror {
    z-index: 1;
    pointer-events: none;
    padding: 11px 40px 11px 14px;
    border: 1px solid transparent;
    font-size: 16px;
    line-height: 1.4;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--text);
  }

  .lines textarea.masked {
    color: transparent;
    caret-color: var(--text);
  }

  .gone {
    color: var(--bad);
    font-weight: inherit;
  }

  .gone-warn {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    color: var(--bad);
  }

  /* One line high, growing a line at a time as the text wraps, so all of it shows. */
  .lines textarea {
    flex: 1;
    min-width: 0;
    padding: 11px 40px 11px 14px;
    font: inherit;
    font-size: 16px;
    line-height: 1.4;
    color: var(--text);
    background: var(--surface-2);
    border: 1px solid var(--line);
    border-radius: 6px;
    outline: none;
    resize: none;
    overflow: hidden;
    field-sizing: content;
    overflow-wrap: anywhere;
  }

  /* Stays beside the first line of text when the field grows. */
  .clear {
    position: absolute;
    z-index: 2;
    top: 10px;
    right: 8px;
    padding: 4px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--muted);
  }

  .clear:hover {
    color: var(--text);
  }

  .lines textarea:focus {
    border-color: var(--accent);
  }

  /* Item suggestions: under the field, over the rows below. */
  .suggest {
    position: absolute;
    top: calc(100% - 4px);
    left: 8px;
    z-index: 5;
    width: min(420px, calc(100% - 16px));
    margin: 0;
    padding: 4px;
    list-style: none;
    background: var(--surface-solid);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }

  .suggest li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-radius: 6px;
    cursor: pointer;
  }

  .suggest .link {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .suggest .tag {
    flex-shrink: 0;
    font-size: 11px;
    padding: 0 6px;
    border-radius: 4px;
    color: var(--helminth);
    border: 1px solid currentColor;
  }

  .suggest li.active {
    background: var(--accent-soft);
  }

  .suggest .suggest-hint {
    cursor: default;
    margin-top: 2px;
    padding-top: 6px;
    border-top: 1px solid var(--line);
    border-radius: 0;
    font-size: 12px;
    color: var(--muted);
  }

  .chars {
    font-size: 13px;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    min-width: 3ch;
    text-align: right;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .chars.over {
    color: var(--bad);
    font-weight: 600;
  }

  .lines > li:has(.chars.over) textarea {
    border-color: var(--bad);
  }

  .copy {
    min-width: 112px;
    padding: 10px 16px;
    justify-content: center;
  }

  .copy.done {
    color: var(--good);
    border-color: var(--good);
  }

  .remove {
    color: var(--muted);
    padding: 10px 11px;
  }

  .remove:hover {
    color: var(--bad) !important;
  }

  .empty {
    display: grid;
    justify-items: start;
    gap: 10px;
    color: var(--muted);
  }

  .empty p {
    margin: 0;
  }
</style>
