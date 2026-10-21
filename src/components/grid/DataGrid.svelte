<script lang="ts" module>
  export interface GridColumn {
    name: string;
    typeName: string;
    isKey?: boolean;
  }
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import { ArrowDown, ArrowUp, ClipboardCopy, Copy, Eye, Filter, KeyRound, RotateCcw, Trash2, Ban } from "@lucide/svelte";
  import { copyText } from "../../lib/actions";
  import { menu, type MenuItem } from "../../lib/state/ui.svelte";
  import type { Cell, SortSpec } from "../../lib/types";
  import { cellValue, emptyEdits, isCellEdited, type PendingEdits } from "../../lib/util/edits";
  import { toClipboardTsv, toJson } from "../../lib/util/export";
  import { displayValue, estimateWidth, isNumericType } from "../../lib/util/format";
  import ValueViewer from "./ValueViewer.svelte";

  interface Props {
    columns: GridColumn[];
    rows: Cell[][];
    rowOffset?: number;
    editable?: boolean;
    edits?: PendingEdits;
    sort?: SortSpec[];
    emptyText?: string;
    onsort?: (column: string, additive: boolean, index: number) => void;
    onedit?: (row: number, column: string, value: Cell) => void;
    oninsertedit?: (id: string, column: string, value: Cell) => void;
    ondelete?: (rows: number[]) => void;
    onfilter?: (column: string, value: Cell) => void;
  }

  let {
    columns,
    rows,
    rowOffset = 0,
    editable = false,
    edits = emptyEdits(),
    sort = [],
    emptyText = "No rows",
    onsort,
    onedit,
    oninsertedit,
    ondelete,
    onfilter,
  }: Props = $props();

  const ROW = 26;
  const HEAD = 42;
  const OVERSCAN = 8;

  let scroller = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let viewport = $state(400);
  let widths = $state<number[]>([]);
  let active = $state<{ r: number; c: number } | null>(null);
  let selected = $state<Set<number>>(new Set());
  let anchor = $state<number | null>(null);
  // `typed` is false while the editor still shows the value it opened with,
  // so opening and leaving a NULL cell does not turn it into ''.
  let editing = $state<{ r: number; c: number; value: string; opened: string; typed: boolean } | null>(null);
  let viewer = $state<{ r: number; c: number } | null>(null);

  const total = $derived(rows.length + edits.inserts.length);
  const numeric = $derived(columns.map((c) => isNumericType(c.typeName)));
  const offsets = $derived(widths.reduce<number[]>((acc, w, i) => [...acc, (acc[i] ?? 0) + w], [0]));
  const gutter = $derived(Math.max(44, String(rowOffset + total).length * 8 + 20));
  const canvasWidth = $derived(gutter + (offsets.at(-1) ?? 0));
  const first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  const last = $derived(Math.min(total, Math.ceil((scrollTop + viewport) / ROW) + OVERSCAN));
  const visible = $derived(Array.from({ length: Math.max(0, last - first) }, (_, i) => first + i));
  const sortOf = $derived(new Map(sort.map((s, i) => [s.column, { dir: s.dir, order: i + 1 }])));

  // Re-measure column widths whenever the column set changes.
  let widthKey = "";
  $effect(() => {
    const k = columns.map((c) => c.name).join("\u0000");
    if (k === widthKey) return;
    widthKey = k;
    widths = columns.map((c, i) => estimateWidth(c.name, rows.map((r) => r[i])));
    active = null;
    selected = new Set();
    editing = null;
  });

  // Row indexes refer to the current rows; a new page or sort invalidates them.
  $effect(() => {
    void rows;
    untrack(() => {
      selected = new Set();
      anchor = null;
      editing = null;
      if (active && active.r >= rows.length + edits.inserts.length) active = null;
    });
  });

  $effect(() => {
    if (!scroller) return;
    const ro = new ResizeObserver(() => (viewport = scroller!.clientHeight - HEAD));
    ro.observe(scroller);
    return () => ro.disconnect();
  });

  function isInserted(r: number) {
    return r >= rows.length;
  }

  function insertAt(r: number) {
    return edits.inserts[r - rows.length];
  }

  function rawValue(r: number, c: number): Cell | undefined {
    const col = columns[c].name;
    if (isInserted(r)) {
      const ins = insertAt(r);
      return ins && col in ins.values ? ins.values[col] : undefined;
    }
    return cellValue(edits, r, col, rows[r]?.[c] ?? null);
  }

  function rowValues(r: number): Cell[] {
    return columns.map((_, c) => rawValue(r, c) ?? null);
  }

  function canEdit(r: number) {
    return editable && !(r < rows.length && edits.deletes.includes(r));
  }

  // ---------- editing ----------

  function startEdit(r: number, c: number, initial?: string) {
    if (!canEdit(r)) return;
    const current = rawValue(r, c);
    if (initial === undefined && typeof current === "string" && /[\r\n]/.test(current)) {
      viewer = { r, c };
      return;
    }
    const opened = initial ?? current ?? "";
    editing = { r, c, value: opened, opened, typed: initial !== undefined };
  }

  function commit(value: Cell, r: number, c: number) {
    const col = columns[c].name;
    if (isInserted(r)) {
      const ins = insertAt(r);
      if (ins) oninsertedit?.(ins.id, col, value);
    } else {
      onedit?.(r, col, value);
    }
  }

  function finishEdit(move: "down" | "right" | "left" | "none" | "blur") {
    if (!editing) return;
    const { r, c, value, opened, typed } = editing;
    editing = null;
    if (typed || value !== opened) commit(value, r, c);
    // Blur means the user clicked elsewhere: don't pull focus or selection back.
    if (move === "blur") return;
    if (move === "down") setActive(Math.min(total - 1, r + 1), c);
    else if (move === "right") setActive(r, Math.min(columns.length - 1, c + 1));
    else if (move === "left") setActive(r, Math.max(0, c - 1));
    else setActive(r, c);
    scroller?.focus();
  }

  function editorKey(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Enter") {
      e.preventDefault();
      finishEdit("down");
    } else if (e.key === "Tab") {
      e.preventDefault();
      finishEdit(e.shiftKey ? "left" : "right");
    } else if (e.key === "Escape") {
      e.preventDefault();
      editing = null;
      scroller?.focus();
    }
  }

  function focusEditor(node: HTMLInputElement) {
    node.focus();
    const end = node.value.length;
    node.setSelectionRange(end, end);
  }

  // ---------- selection & keyboard ----------

  function setActive(r: number, c: number) {
    if (total === 0 || columns.length === 0) return;
    active = { r: Math.max(0, Math.min(total - 1, r)), c: Math.max(0, Math.min(columns.length - 1, c)) };
    revealActive();
  }

  function revealActive() {
    if (!active || !scroller) return;
    const top = active.r * ROW;
    if (top < scroller.scrollTop) scroller.scrollTop = top;
    else if (top + ROW > scroller.scrollTop + viewport) scroller.scrollTop = top + ROW - viewport;
    const left = offsets[active.c];
    const right = offsets[active.c + 1];
    const visibleWidth = scroller.clientWidth - gutter;
    if (left < scroller.scrollLeft) scroller.scrollLeft = left;
    else if (right > scroller.scrollLeft + visibleWidth) scroller.scrollLeft = right - visibleWidth;
  }

  function selectRow(r: number, e: MouseEvent) {
    if (e.shiftKey && anchor !== null) {
      const [a, b] = anchor < r ? [anchor, r] : [r, anchor];
      selected = new Set(Array.from({ length: b - a + 1 }, (_, i) => a + i));
    } else if (e.metaKey || e.ctrlKey) {
      const next = new Set(selected);
      if (next.has(r)) next.delete(r);
      else next.add(r);
      selected = next;
      anchor = r;
    } else {
      selected = new Set([r]);
      anchor = r;
    }
    active = { r, c: active?.c ?? 0 };
  }

  function targetRows(): number[] {
    if (selected.size > 0) return [...selected].sort((a, b) => a - b);
    return active ? [active.r] : [];
  }

  function copySelection(asJson = false) {
    const rs = targetRows();
    if (rs.length === 0) return;
    if (!asJson && selected.size === 0 && active) {
      const v = rawValue(active.r, active.c) ?? null;
      void copyText(v ?? "NULL", "Value copied");
      return;
    }
    const data = rs.map(rowValues);
    const names = columns.map((c) => c.name);
    void copyText(asJson ? toJson(names, data) : toClipboardTsv(data), `${rs.length} row(s) copied`);
  }

  function requestDelete() {
    const rs = targetRows();
    if (editable && rs.length > 0) ondelete?.(rs);
  }

  function onkeydown(e: KeyboardEvent) {
    if (editing || viewer) return;
    const mod = e.metaKey || e.ctrlKey;
    if (mod && e.key.toLowerCase() === "c") {
      e.preventDefault();
      copySelection();
      return;
    }
    if (mod && e.key.toLowerCase() === "a") {
      e.preventDefault();
      selected = new Set(Array.from({ length: rows.length }, (_, i) => i));
      return;
    }
    if (mod && (e.key === "Backspace" || e.key === "Delete")) {
      e.preventDefault();
      requestDelete();
      return;
    }
    if (!active) {
      if (e.key.startsWith("Arrow")) setActive(0, 0);
      return;
    }
    const { r, c } = active;
    const page = Math.max(1, Math.floor(viewport / ROW) - 1);
    const moves: Record<string, [number, number]> = {
      ArrowDown: [r + 1, c],
      ArrowUp: [r - 1, c],
      ArrowLeft: [r, c - 1],
      ArrowRight: [r, c + 1],
      PageDown: [r + page, c],
      PageUp: [r - page, c],
      Home: [mod ? 0 : r, 0],
      End: [mod ? total - 1 : r, columns.length - 1],
      Tab: [r, e.shiftKey ? c - 1 : c + 1],
    };
    if (e.key in moves) {
      e.preventDefault();
      const [nr, nc] = moves[e.key];
      setActive(nr, nc);
      if (!e.shiftKey) selected = new Set();
      return;
    }
    if (e.key === "Escape") {
      selected = new Set();
      return;
    }
    if (e.key === "Enter" || e.key === "F2") {
      e.preventDefault();
      startEdit(r, c);
      return;
    }
    if (e.key === " " && e.shiftKey) {
      e.preventDefault();
      viewer = { r, c };
      return;
    }
    if (!mod && !e.altKey && e.key.length === 1) {
      e.preventDefault();
      startEdit(r, c, e.key);
    } else if (e.key === "Backspace" || e.key === "Delete") {
      e.preventDefault();
      startEdit(r, c, "");
    }
  }

  // ---------- column resize ----------

  function startResize(e: MouseEvent, c: number) {
    e.preventDefault();
    e.stopPropagation();
    const startX = e.clientX;
    const startW = widths[c];
    const move = (ev: MouseEvent) => {
      widths = widths.map((w, i) => (i === c ? Math.max(48, startW + ev.clientX - startX) : w));
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      document.body.style.cursor = "";
    };
    document.body.style.cursor = "col-resize";
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }

  function autosize(c: number) {
    const values = Array.from({ length: Math.min(total, 500) }, (_, r) => rawValue(r, c) ?? null);
    widths = widths.map((w, i) => (i === c ? estimateWidth(columns[c].name, values) : w));
  }

  // ---------- context menu ----------

  function cellMenu(e: MouseEvent, r: number, c: number) {
    if (!selected.has(r)) selected = new Set();
    active = { r, c };
    const v = rawValue(r, c) ?? null;
    const col = columns[c];
    const deleted = !isInserted(r) && edits.deletes.includes(r);
    const many = selected.size > 1;
    const items: MenuItem[] = [
      { label: "Copy value", icon: Copy, shortcut: "⌘C", action: () => void copyText(v ?? "NULL", "Value copied") },
      { label: many ? `Copy ${selected.size} rows` : "Copy row", icon: ClipboardCopy, action: () => copySelectionOrRow(r) },
      { label: "Copy as JSON", icon: ClipboardCopy, action: () => copySelection(true) },
      { label: "Copy column name", action: () => void copyText(col.name, "Column name copied") },
      { separator: true, label: "" },
      { label: "View value", icon: Eye, shortcut: "⇧Space", action: () => (viewer = { r, c }) },
    ];
    if (onfilter && !isInserted(r)) {
      items.push({ label: `Filter where ${col.name} = this`, icon: Filter, action: () => onfilter?.(col.name, rows[r][c]) });
    }
    if (editable) {
      items.push({ separator: true, label: "" });
      items.push({ label: "Set NULL", icon: Ban, disabled: deleted, action: () => commit(null, r, c) });
      if (isInserted(r)) {
        items.push({ label: "Remove new row", icon: Trash2, danger: true, action: () => insertAt(r) && ondelete?.([r]) });
      } else {
        items.push(
          deleted
            ? { label: "Restore row", icon: RotateCcw, action: () => ondelete?.([r]) }
            : { label: many ? `Delete ${selected.size} rows` : "Delete row", icon: Trash2, danger: true, shortcut: "⌘⌫", action: requestDelete },
        );
      }
    }
    menu.show(e, items);
  }

  function copySelectionOrRow(r: number) {
    if (selected.size > 1) copySelection();
    else void copyText(toClipboardTsv([rowValues(r)]), "Row copied");
  }

  function headerClick(e: MouseEvent, col: GridColumn, index: number) {
    onsort?.(col.name, e.shiftKey, index);
  }
</script>

<div
  class="grid scroll"
  bind:this={scroller}
  onscroll={() => (scrollTop = scroller?.scrollTop ?? 0)}
  tabindex="0"
  role="grid"
  aria-rowcount={total}
  aria-colcount={columns.length}
  {onkeydown}
>
  <div class="canvas" style:width="{canvasWidth}px" style:height="{HEAD + total * ROW}px">
    <div class="head" role="row" style:width="{canvasWidth}px">
      <div class="corner" style:width="{gutter}px"></div>
      {#each columns as col, c (col.name + c)}
        {@const s = sortOf.get(col.name)}
        <div class="th" class:num={numeric[c]} role="columnheader" style:width="{widths[c]}px" aria-sort={s ? (s.dir === "asc" ? "ascending" : "descending") : "none"}>
          <button class="th-btn" disabled={!onsort} onclick={(e) => headerClick(e, col, c)} title="{col.name} ({col.typeName}){onsort ? ' — click to sort, shift-click to add' : ''}">
            <span class="name">
              {#if col.isKey}<KeyRound size={11} class="key" />{/if}
              {col.name}
            </span>
            <span class="type">{col.typeName}</span>
          </button>
          {#if s}
            <span class="sort" aria-hidden="true">
              {#if s.dir === "asc"}<ArrowUp size={12} />{:else}<ArrowDown size={12} />{/if}
              {#if sort.length > 1}<small>{s.order}</small>{/if}
            </span>
          {/if}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <span class="resize" role="separator" aria-orientation="vertical" onmousedown={(e) => startResize(e, c)} ondblclick={() => autosize(c)}></span>
        </div>
      {/each}
    </div>

    <div class="body" style:transform="translateY({first * ROW}px)">
      {#each visible as r (r)}
        {@const inserted = isInserted(r)}
        {@const deleted = !inserted && edits.deletes.includes(r)}
        <div
          class="tr"
          role="row"
          class:alt={r % 2 === 1}
          class:selected={selected.has(r)}
          class:inserted
          class:deleted
          aria-rowindex={r + 1}
        >
          <div class="gutter" style:width="{gutter}px" role="rowheader" tabindex="-1" onmousedown={(e) => selectRow(r, e)} oncontextmenu={(e) => cellMenu(e, r, active?.c ?? 0)}>
            {inserted ? "+" : rowOffset + r + 1}
          </div>
          {#each columns as col, c (col.name + c)}
            {@const v = rawValue(r, c)}
            <div
              class="td"
              role="gridcell"
              tabindex="-1"
              style:width="{widths[c]}px"
              class:num={numeric[c]}
              class:null={v === null}
              class:default={v === undefined}
              class:edited={!inserted && isCellEdited(edits, r, col.name)}
              class:active={active?.r === r && active?.c === c}
              onmousedown={() => {
                active = { r, c };
                if (!selected.has(r)) selected = new Set();
              }}
              ondblclick={() => startEdit(r, c)}
              oncontextmenu={(e) => cellMenu(e, r, c)}
            >
              {v === undefined ? "default" : displayValue(v)}
            </div>
          {/each}
        </div>
      {/each}
    </div>

    {#if editing}
      <input
        class="editor"
        style:left="{gutter + offsets[editing.c]}px"
        style:top="{HEAD + editing.r * ROW}px"
        style:width="{Math.max(widths[editing.c], 160)}px"
        bind:value={editing.value}
        onkeydown={editorKey}
        onblur={() => finishEdit("blur")}
        use:focusEditor
        aria-label="Edit {columns[editing.c].name}"
      />
    {/if}
  </div>

  {#if total === 0}
    <div class="empty">{columns.length === 0 ? "" : emptyText}</div>
  {/if}
</div>

{#if viewer}
  {@const at = viewer}
  <ValueViewer
    column={columns[at.c]}
    value={rawValue(at.r, at.c) ?? null}
    editable={canEdit(at.r)}
    onsave={(v) => commit(v, at.r, at.c)}
    onclose={() => {
      viewer = null;
      scroller?.focus();
    }}
  />
{/if}

<style>
  .grid {
    position: relative;
    height: 100%;
    background: var(--surface);
    outline: none;
    font-variant-numeric: tabular-nums;
    contain: strict;
  }
  .grid:focus-visible {
    box-shadow: inset 0 0 0 1px var(--accent-rule);
    border-radius: 0;
  }
  .canvas {
    position: relative;
    min-width: 100%;
  }

  /* header */
  .head {
    position: sticky;
    top: 0;
    z-index: 3;
    display: flex;
    height: 42px;
    background: var(--surface-2);
    border-bottom: 1px solid var(--rule-strong);
  }
  .corner {
    position: sticky;
    left: 0;
    z-index: 4;
    flex: none;
    background: var(--surface-2);
    border-right: 1px solid var(--rule);
  }
  .th {
    position: relative;
    flex: none;
    display: flex;
    align-items: stretch;
    border-right: 1px solid var(--rule);
  }
  .th-btn {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 1px;
    padding: 0 22px 0 9px;
    border: 0;
    background: none;
    text-align: left;
  }
  .th.num .th-btn {
    align-items: flex-end;
    text-align: right;
  }
  .th-btn:hover:not(:disabled) {
    background: var(--sunken);
  }
  .name {
    display: flex;
    align-items: center;
    gap: 4px;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
    font-size: var(--text-sm);
  }
  .name :global(.key) {
    flex: none;
    color: var(--readonly);
  }
  .type {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-xs);
    color: var(--faint);
  }
  .sort {
    position: absolute;
    right: 7px;
    top: 50%;
    transform: translateY(-50%);
    display: flex;
    align-items: center;
    color: var(--accent);
    pointer-events: none;
  }
  .sort small {
    font-size: 9px;
    font-weight: 700;
  }
  .resize {
    position: absolute;
    right: -4px;
    top: 0;
    bottom: 0;
    width: 8px;
    z-index: 2;
    cursor: col-resize;
  }
  .resize:hover {
    background: linear-gradient(to right, transparent 3px, var(--accent) 3px, var(--accent) 5px, transparent 5px);
  }

  /* rows */
  .body {
    position: absolute;
    top: 42px;
    left: 0;
    right: 0;
    will-change: transform;
  }
  .tr {
    display: flex;
    height: 26px;
    background: var(--surface);
  }
  .tr.alt {
    background: var(--row-alt);
  }
  .tr:hover {
    background: var(--row-hover);
  }
  .tr.selected,
  .tr.selected .gutter {
    background: var(--row-selected);
  }
  .gutter {
    position: sticky;
    left: 0;
    z-index: 1;
    flex: none;
    padding: 0 8px;
    line-height: 26px;
    text-align: right;
    font-size: var(--text-xs);
    color: var(--faint);
    background: inherit;
    border-right: 1px solid var(--rule);
    box-shadow: 1px 0 0 var(--rule);
  }
  .tr.inserted .gutter {
    color: var(--ok);
    font-weight: 700;
    box-shadow: inset 3px 0 0 var(--ok);
  }
  .tr.inserted {
    background: var(--ok-soft);
  }
  .tr.deleted {
    background: var(--danger-soft);
  }
  .tr.deleted .td {
    text-decoration: line-through;
    text-decoration-color: color-mix(in srgb, var(--danger) 70%, transparent);
    color: var(--muted);
  }
  .tr.deleted .gutter {
    box-shadow: inset 3px 0 0 var(--danger);
  }
  .td {
    flex: none;
    padding: 0 9px;
    line-height: 26px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    border-right: 1px solid var(--rule);
    border-bottom: 1px solid color-mix(in srgb, var(--rule) 55%, transparent);
  }
  .td.num {
    text-align: right;
    color: var(--number);
  }
  .td.null,
  .td.default {
    color: var(--null);
    font-style: italic;
    font-size: var(--text-sm);
  }
  .td.edited {
    background: var(--edited);
    box-shadow: inset 0 -2px 0 var(--edited-mark);
  }
  .td.active {
    box-shadow: inset 0 0 0 2px var(--accent);
    border-radius: 2px;
  }
  .td.edited.active {
    box-shadow: inset 0 0 0 2px var(--accent), inset 0 -4px 0 var(--edited-mark);
  }

  .editor {
    position: absolute;
    z-index: 5;
    height: 26px;
    padding: 0 8px;
    border: 2px solid var(--accent);
    border-radius: 2px;
    background: var(--surface);
    outline: none;
    box-shadow: var(--shadow-pop);
  }

  .empty {
    position: absolute;
    top: 42px;
    left: 0;
    right: 0;
    padding: 28px;
    text-align: center;
    color: var(--muted);
  }
</style>
