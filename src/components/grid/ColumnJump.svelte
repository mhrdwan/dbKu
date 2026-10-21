<script lang="ts">
  import { Columns3 } from "@lucide/svelte";
  import { findColumns } from "../../lib/util/columns";
  import type { GridColumn } from "./DataGrid.svelte";

  interface Props {
    columns: GridColumn[];
    onpick: (index: number) => void;
    /** Shown in the tooltip when the parent binds a shortcut to `open()`. */
    shortcut?: string;
  }

  let { columns, onpick, shortcut }: Props = $props();

  let isOpen = $state(false);
  let query = $state("");
  let cursor = $state(0);
  let root = $state<HTMLDivElement>();
  let list = $state<HTMLUListElement>();

  const matches = $derived(findColumns(columns.map((c) => c.name), query));

  export function open() {
    if (columns.length === 0) return;
    query = "";
    cursor = 0;
    isOpen = true;
  }

  function close() {
    isOpen = false;
  }

  function pick(index: number) {
    close();
    onpick(index);
  }

  function move(delta: number) {
    if (matches.length === 0) return;
    cursor = (cursor + delta + matches.length) % matches.length;
    list?.children[cursor]?.scrollIntoView({ block: "nearest" });
  }

  function onkeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "ArrowDown") {
      e.preventDefault();
      move(1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      move(-1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const m = matches[cursor];
      if (m) pick(m.index);
    } else if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  }

  // Close when clicking anywhere outside the popover.
  $effect(() => {
    if (!isOpen) return;
    const away = (e: PointerEvent) => {
      if (root && !root.contains(e.target as Node)) close();
    };
    window.addEventListener("pointerdown", away, true);
    return () => window.removeEventListener("pointerdown", away, true);
  });

  function focusInput(node: HTMLInputElement) {
    node.focus();
  }
</script>

<div class="jump" bind:this={root}>
  <button
    class="btn small ghost"
    onclick={() => (isOpen ? close() : open())}
    disabled={columns.length === 0}
    title={shortcut ? `Jump to column (${shortcut})` : "Jump to column"}
    aria-haspopup="listbox"
    aria-expanded={isOpen}
  >
    <Columns3 size={14} /> Columns <span class="count">{columns.length}</span>
  </button>

  {#if isOpen}
    <div class="pop" role="dialog" aria-label="Jump to column">
      <input
        class="field"
        placeholder="Column name, e.g. RoomSuite"
        bind:value={query}
        oninput={() => (cursor = 0)}
        {onkeydown}
        use:focusInput
        spellcheck="false"
        role="combobox"
        aria-controls="column-jump-list"
        aria-expanded="true"
        aria-activedescendant={matches[cursor] ? `column-jump-${matches[cursor].index}` : undefined}
        aria-label="Column name"
      />
      {#if matches.length === 0}
        <p class="none">No column matches “{query.trim()}”</p>
      {:else}
        <ul id="column-jump-list" role="listbox" bind:this={list}>
          {#each matches as m, i (m.index)}
            {@const col = columns[m.index]}
            <li
              id="column-jump-{m.index}"
              role="option"
              aria-selected={i === cursor}
              class:current={i === cursor}
              onmousemove={() => (cursor = i)}
              onmousedown={(e) => {
                e.preventDefault();
                pick(m.index);
              }}
            >
              <span class="name">
                {#if m.start >= 0}
                  {col.name.slice(0, m.start)}<mark>{col.name.slice(m.start, m.start + m.length)}</mark>{col.name.slice(m.start + m.length)}
                {:else}
                  {col.name}
                {/if}
              </span>
              <span class="type">{col.typeName}</span>
            </li>
          {/each}
        </ul>
      {/if}
      <p class="hint"><kbd>↑</kbd><kbd>↓</kbd> choose · <kbd>↵</kbd> jump · <kbd>esc</kbd> close</p>
    </div>
  {/if}
</div>

<style>
  .jump {
    position: relative;
  }
  .count {
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }
  .pop {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 20;
    width: 300px;
    padding: 6px;
    border: 1px solid var(--rule-strong);
    border-radius: var(--radius-m);
    background: var(--surface);
    box-shadow: var(--shadow-pop);
  }
  .pop .field {
    width: 100%;
  }
  ul {
    max-height: 320px;
    margin: 6px 0 0;
    padding: 0;
    overflow-y: auto;
    list-style: none;
  }
  li {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
    padding: 5px 8px;
    border-radius: var(--radius-s, 4px);
    cursor: default;
  }
  li.current {
    background: var(--accent-soft);
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
  }
  mark {
    background: none;
    color: var(--accent);
    font-weight: 700;
  }
  .type {
    flex: none;
    font-size: var(--text-xs);
    color: var(--faint);
  }
  .none {
    margin: 10px 8px;
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .hint {
    margin: 6px 4px 0;
    padding-top: 6px;
    border-top: 1px solid var(--rule);
    color: var(--faint);
    font-size: var(--text-xs);
  }
  .hint kbd {
    margin-right: 2px;
  }
</style>
