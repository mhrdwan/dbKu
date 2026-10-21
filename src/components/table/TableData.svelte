<script lang="ts">
  import { ask } from "@tauri-apps/plugin-dialog";
  import { untrack } from "svelte";
  import { ChevronLeft, ChevronRight, ChevronsLeft, ChevronsRight, FileUp, Lock, Plus, RefreshCw, Search, Trash2, X } from "@lucide/svelte";
  import { api, errorMessage } from "../../lib/api";
  import { connections } from "../../lib/state/connections.svelte";
  import { schema } from "../../lib/state/schema.svelte";
  import { settings } from "../../lib/state/settings.svelte";
  import { dataVersions, dialogs, toasts } from "../../lib/state/ui.svelte";
  import type { Cell, ColumnInfo, ObjectKind, ResultSet, SortSpec, TableQuery, TableRef } from "../../lib/types";
  import {
    addInsert,
    editCount,
    emptyEdits,
    removeInsert,
    setCell,
    setInsertCell,
    toggleDelete,
    toRowChanges,
    type PendingEdits,
  } from "../../lib/util/edits";
  import { plural } from "../../lib/util/format";
  import { equalsPredicate } from "../../lib/util/sql";
  import DataGrid, { type GridColumn } from "../grid/DataGrid.svelte";
  import ExportButton from "../grid/ExportButton.svelte";
  import ChangePreview from "./ChangePreview.svelte";

  interface Props {
    connId: string;
    table: TableRef;
    objectKind: ObjectKind;
    initialSearch?: string;
    ondirty?: (dirty: boolean) => void;
  }

  let { connId, table, objectKind, initialSearch = "", ondirty }: Props = $props();

  const EXPORT_CHUNK = 5000;
  const EXPORT_MAX = 500_000;

  let info = $state<ColumnInfo[]>([]);
  let result = $state<ResultSet | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let total = $state<number | null>(null);
  let counting = $state(false);
  let offset = $state(0);
  let sort = $state<SortSpec[]>([]);
  let filter = $state("");
  let filterDraft = $state("");
  // svelte-ignore state_referenced_locally
  let search = $state(initialSearch);
  // svelte-ignore state_referenced_locally
  let searchDraft = $state(initialSearch);
  let edits = $state<PendingEdits>(emptyEdits());
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let preview = $state<string[] | null>(null);
  let saving = $state(false);

  const profile = $derived(connections.byId(connId));
  const limit = $derived(settings.value.pageSize);
  const keyColumns = $derived(info.filter((c) => c.isPrimaryKey).map((c) => c.name));
  const pending = $derived(editCount(edits));

  const readOnlyReason = $derived.by(() => {
    if (profile?.readOnly) return "Read-only connection";
    if (objectKind !== "table") return "Views can't be edited here";
    if (info.length > 0 && keyColumns.length === 0) return "No primary key, so rows can't be identified for editing";
    return null;
  });
  const editable = $derived(!readOnlyReason && info.length > 0);

  const gridColumns = $derived<GridColumn[]>(
    (result?.columns ?? []).map((c) => {
      const meta = info.find((i) => i.name === c.name);
      return { name: c.name, typeName: meta?.dataType ?? c.typeName, isKey: meta?.isPrimaryKey };
    }),
  );

  function query(extra: Partial<TableQuery> = {}): TableQuery {
    return {
      ...table,
      limit,
      offset,
      sort,
      filter: filter || null,
      search: search || null,
      searchColumns: [],
      ...extra,
    };
  }

  // Only the newest request may update the view; slower older ones are dropped.
  let loadSeq = 0;
  let countSeq = 0;

  async function load() {
    const seq = ++loadSeq;
    loading = true;
    error = null;
    try {
      const page = await api.fetchTable(connId, query());
      if (seq !== loadSeq) return;
      result = page;
      edits = emptyEdits();
    } catch (e) {
      if (seq === loadSeq) error = errorMessage(e);
    } finally {
      if (seq === loadSeq) loading = false;
    }
  }

  async function count() {
    const seq = ++countSeq;
    counting = true;
    total = null;
    try {
      const n = await api.countRows(connId, query());
      if (seq === countSeq) total = n;
    } catch {
      if (seq === countSeq) total = null;
    } finally {
      if (seq === countSeq) counting = false;
    }
  }

  // First load only; later loads are triggered explicitly (paging, sorting, …).
  $effect(() => {
    untrack(() => {
      schema
        .loadColumns(connId, table)
        .then((cols) => (info = cols))
        .catch((e) => toasts.show(`Couldn't read the table's columns, so editing is off. ${errorMessage(e)}`, "error"));
      void load();
      void count();
    });
  });

  // "Find in database" can re-open this table with a new search term.
  let lastInitialSearch = untrack(() => initialSearch);
  $effect(() => {
    const next = initialSearch;
    if (next === lastInitialSearch) return;
    lastInitialSearch = next;
    untrack(() => {
      searchDraft = next;
      void applyFilter();
    });
  });

  // Tell the tab bar about unsaved edits so closing the tab asks first.
  $effect(() => {
    ondirty?.(pending > 0);
  });

  $effect(() => () => clearTimeout(searchTimer));

  // Reload after an import into this table.
  let seenVersion = untrack(() => dataVersions.of(connId, table));
  $effect(() => {
    const v = dataVersions.of(connId, table);
    if (v === seenVersion) return;
    seenVersion = v;
    untrack(() => {
      edits = emptyEdits();
      void load();
      void count();
    });
  });

  async function confirmDiscard(): Promise<boolean> {
    if (pending === 0) return true;
    return ask(`Discard ${plural(pending, "unsaved change")}?`, { title: "Unsaved changes", kind: "warning" });
  }

  async function goTo(newOffset: number) {
    if (!(await confirmDiscard())) return;
    offset = Math.max(0, newOffset);
    await load();
  }

  async function changePageSize(select: HTMLSelectElement) {
    if (!(await confirmDiscard())) {
      select.value = String(limit);
      return;
    }
    settings.set("pageSize", Number(select.value));
    offset = 0;
    await load();
  }

  async function reload(recount = true) {
    if (!(await confirmDiscard())) return;
    await load();
    if (recount) void count();
  }

  async function applyFilter() {
    clearTimeout(searchTimer);
    if (!(await confirmDiscard())) return;
    filter = filterDraft.trim();
    search = searchDraft.trim();
    offset = 0;
    await load();
    void count();
  }

  async function toggleSort(column: string, additive: boolean) {
    if (!(await confirmDiscard())) return;
    const existing = sort.find((s) => s.column === column);
    const next: SortSpec | null = !existing ? { column, dir: "asc" } : existing.dir === "asc" ? { column, dir: "desc" } : null;
    const others = additive ? sort.filter((s) => s.column !== column) : [];
    sort = next ? [...others, next] : others;
    offset = 0;
    await load();
  }

  function filterBy(column: string, value: Cell) {
    filterDraft = equalsPredicate(profile?.driver, column, value);
    void applyFilter();
  }

  function onSearchInput() {
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => void applyFilter(), 400);
  }

  function editCell(r: number, column: string, value: Cell) {
    const row = result?.rows[r];
    if (!result || !row) return;
    const original = row[result.columns.findIndex((c) => c.name === column)] ?? null;
    edits = setCell(edits, r, column, value, original);
  }

  function addRow() {
    edits = addInsert(edits, crypto.randomUUID());
  }

  function deleteRows(rows: number[]) {
    const n = result?.rows.length ?? 0;
    edits = rows.reduce((acc, r) => {
      if (r >= n) {
        const ins = acc.inserts[r - n];
        return ins ? removeInsert(acc, ins.id) : acc;
      }
      return toggleDelete(acc, r);
    }, edits);
  }

  async function reviewChanges() {
    if (!result) return;
    try {
      const changes = toRowChanges(edits, result.rows, result.columns.map((c) => c.name), keyColumns);
      preview = await api.previewChanges(connId, table, changes);
    } catch (e) {
      toasts.show(errorMessage(e), "error");
    }
  }

  async function applyChanges() {
    if (!result) return;
    saving = true;
    try {
      const changes = toRowChanges(edits, result.rows, result.columns.map((c) => c.name), keyColumns);
      await api.applyChanges(connId, table, changes);
      toasts.show(`Saved ${plural(changes.length, "change")}`, "success");
      preview = null;
      edits = emptyEdits();
      await load();
      void count();
    } catch (e) {
      toasts.show(`Nothing was saved. ${errorMessage(e)}`, "error");
    } finally {
      saving = false;
    }
  }

  async function loadAll(): Promise<Cell[][]> {
    const all: Cell[][] = [];
    for (let at = 0; at < EXPORT_MAX; at += EXPORT_CHUNK) {
      const page = await api.fetchTable(connId, query({ offset: at, limit: EXPORT_CHUNK }));
      all.push(...page.rows);
      if (page.rows.length < EXPORT_CHUNK) return all;
    }
    toasts.show(`Export stopped at ${EXPORT_MAX.toLocaleString()} rows.`, "info");
    return all;
  }

  function onkeydown(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s" && pending > 0) {
      e.preventDefault();
      void reviewChanges();
    }
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "r") {
      e.preventDefault();
      void reload();
    }
  }

  const rowCount = $derived(result?.rows.length ?? 0);
  const lastOffset = $derived(total === null ? null : Math.max(0, Math.floor((total - 1) / limit) * limit));
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="data" {onkeydown}>
  <div class="toolbar">
    <label class="where">
      <span class="kw">WHERE</span>
      <input
        class="field code"
        placeholder="status = 'active' AND created_at > '2026-01-01'"
        bind:value={filterDraft}
        onkeydown={(e) => e.key === "Enter" && applyFilter()}
        spellcheck="false"
        aria-label="Filter rows (SQL WHERE clause)"
      />
      {#if filter}
        <button class="btn ghost small icon" aria-label="Clear filter" onclick={() => ((filterDraft = ""), applyFilter())}><X size={13} /></button>
      {/if}
    </label>
    <label class="search">
      <Search size={13} />
      <input
        class="field"
        placeholder="Search all columns"
        bind:value={searchDraft}
        oninput={onSearchInput}
        onkeydown={(e) => e.key === "Enter" && applyFilter()}
        aria-label="Search all columns"
      />
    </label>
    <button class="btn small icon ghost" onclick={() => reload()} title="Refresh (⌘R)" aria-label="Refresh"><RefreshCw size={14} class={loading ? "spin" : ""} /></button>
  </div>

  <div class="grid-wrap">
    {#if error}
      <div class="error selectable">
        <strong>Couldn't load rows.</strong>
        <pre>{error}</pre>
        <button class="btn small" onclick={() => reload()}>Try again</button>
      </div>
    {:else if result}
      <DataGrid
        columns={gridColumns}
        rows={result.rows}
        rowOffset={offset}
        {editable}
        {edits}
        {sort}
        emptyText={filter || search ? "No rows match the filter" : "This table is empty"}
        onsort={toggleSort}
        onedit={editCell}
        oninsertedit={(id, col, v) => (edits = setInsertCell(edits, id, col, v))}
        ondelete={deleteRows}
        onfilter={filterBy}
      />
    {/if}
  </div>

  <footer class="footer">
    <div class="left">
      {#if editable}
        <button class="btn small" onclick={addRow}><Plus size={13} /> Add row</button>
        <button class="btn small ghost" onclick={() => (dialogs.transfer = { kind: "import", connId, table })}><FileUp size={13} /> Import CSV</button>
      {:else if readOnlyReason}
        <span class="locked"><Lock size={12} /> {readOnlyReason}</span>
      {/if}
      {#if pending > 0}
        <span class="pending">{plural(pending, "change")} not saved</span>
        <button class="btn small ghost" onclick={() => (edits = emptyEdits())}><Trash2 size={13} /> Discard</button>
        <button class="btn small primary" onclick={reviewChanges}>Review & save <kbd>⌘S</kbd></button>
      {/if}
    </div>
    <div class="right">
      <ExportButton name={table.table} columns={result?.columns.map((c) => c.name) ?? []} rows={result?.rows ?? []} {loadAll} />
      <span class="range">
        {#if rowCount > 0}{(offset + 1).toLocaleString()}–{(offset + rowCount).toLocaleString()}{:else}0{/if}
        of {counting ? "…" : total === null ? "?" : total.toLocaleString()}
      </span>
      <div class="pager">
        <button class="btn small icon ghost" aria-label="First page" disabled={offset === 0} onclick={() => goTo(0)}><ChevronsLeft size={14} /></button>
        <button class="btn small icon ghost" aria-label="Previous page" disabled={offset === 0} onclick={() => goTo(offset - limit)}><ChevronLeft size={14} /></button>
        <button class="btn small icon ghost" aria-label="Next page" disabled={rowCount < limit} onclick={() => goTo(offset + limit)}><ChevronRight size={14} /></button>
        <button class="btn small icon ghost" aria-label="Last page" disabled={lastOffset === null || offset >= lastOffset} onclick={() => lastOffset !== null && goTo(lastOffset)}><ChevronsRight size={14} /></button>
      </div>
      <select class="field page-size" value={limit} onchange={(e) => void changePageSize(e.currentTarget)} aria-label="Rows per page">
        {#each [50, 100, 200, 500, 1000, 5000] as n (n)}<option value={n}>{n} / page</option>{/each}
      </select>
    </div>
  </footer>
</div>

{#if preview}
  <ChangePreview statements={preview} {saving} onapply={applyChanges} onclose={() => (preview = null)} />
{/if}

<style>
  .data {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 10px;
    border-bottom: 1px solid var(--rule);
  }
  .where {
    flex: 1;
    display: flex;
    align-items: center;
    min-width: 0;
    border: 1px solid var(--rule-strong);
    border-radius: var(--radius-m);
    background: var(--surface);
    padding-right: 2px;
  }
  .where:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .kw {
    padding: 0 8px;
    align-self: stretch;
    display: grid;
    place-items: center;
    font: 600 var(--text-xs) var(--font-code);
    color: var(--accent);
    background: var(--accent-soft);
    border-radius: var(--radius-m) 0 0 var(--radius-m);
  }
  .where .field {
    flex: 1;
    border: 0;
    box-shadow: none;
    background: transparent;
  }
  .search {
    position: relative;
    display: flex;
    align-items: center;
    width: 220px;
  }
  .search :global(svg) {
    position: absolute;
    left: 8px;
    color: var(--faint);
  }
  .search .field {
    width: 100%;
    padding-left: 27px;
  }
  .grid-wrap {
    flex: 1;
    min-height: 0;
  }
  .error {
    padding: 24px;
  }
  .error pre {
    margin: 8px 0 14px;
    color: var(--danger);
    font: var(--text-sm) / 1.5 var(--font-code);
    white-space: pre-wrap;
  }
  .footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    height: 38px;
    padding: 0 10px;
    border-top: 1px solid var(--rule);
    background: var(--surface-2);
  }
  .left,
  .right {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .locked {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--muted);
    font-size: var(--text-sm);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .pending {
    padding: 2px 8px;
    border-radius: 10px;
    background: var(--edited);
    color: var(--ink);
    font-size: var(--text-sm);
    font-weight: 600;
    box-shadow: inset 0 -2px 0 var(--edited-mark);
  }
  .range {
    color: var(--muted);
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .pager {
    display: flex;
  }
  .page-size {
    height: 24px;
    font-size: var(--text-sm);
  }
  .primary kbd {
    background: transparent;
    color: inherit;
    border-color: color-mix(in srgb, currentColor 40%, transparent);
  }
  .data :global(.spin) {
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
