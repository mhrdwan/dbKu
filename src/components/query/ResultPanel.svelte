<script lang="ts">
  import { CircleAlert, CircleCheck, LoaderCircle, TriangleAlert } from "@lucide/svelte";
  import type { ExecOutcome, SortSpec } from "../../lib/types";
  import { formatMs, plural } from "../../lib/util/format";
  import DataGrid from "../grid/DataGrid.svelte";
  import ExportButton from "../grid/ExportButton.svelte";

  interface Props {
    outcome: ExecOutcome | null;
    running: boolean;
    runError: string | null;
  }

  let { outcome, running, runError }: Props = $props();

  let selected = $state<number | "messages">(0);
  let sort = $state<SortSpec[]>([]);
  // Column positions for `sort` (names can repeat, e.g. SELECT a.id, b.id).
  let sortIndex = $state<number[]>([]);

  const grids = $derived((outcome?.results ?? []).map((r, i) => ({ r, i })).filter(({ r }) => r.columns.length > 0));

  // Show the first grid after each run, or messages when there is none.
  $effect(() => {
    void outcome;
    sort = [];
    sortIndex = [];
    selected = grids.length > 0 && !outcome?.error ? grids[0].i : "messages";
  });

  const current = $derived(typeof selected === "number" ? outcome?.results[selected] : undefined);

  // Client-side sort of a result set (it is already in memory).
  const sortedRows = $derived.by(() => {
    if (!current || sort.length === 0) return current?.rows ?? [];
    const idx = sort.map((s, n) => ({ i: sortIndex[n], dir: s.dir === "asc" ? 1 : -1 }));
    const cmp = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });
    return [...current.rows].sort((a, b) => {
      for (const { i, dir } of idx) {
        const x = a[i];
        const y = b[i];
        if (x === y) continue;
        if (x === null) return 1;
        if (y === null) return -1;
        const n = cmp.compare(x, y);
        if (n !== 0) return n * dir;
      }
      return 0;
    });
  });

  function toggleSort(column: string, additive: boolean, index: number) {
    const at = sortIndex.indexOf(index);
    const existing = at >= 0 ? sort[at] : undefined;
    const next: SortSpec | null = !existing ? { column, dir: "asc" } : existing.dir === "asc" ? { column, dir: "desc" } : null;
    const keep = additive ? sort.map((s, n) => ({ s, i: sortIndex[n] })).filter(({ i }) => i !== index) : [];
    const all = next ? [...keep, { s: next, i: index }] : keep;
    sort = all.map(({ s }) => s);
    sortIndex = all.map(({ i }) => i);
  }
</script>

<section class="panel" aria-label="Results">
  <div class="bar">
    <div class="tabs" role="tablist">
      {#each grids as { r, i }, n (i)}
        <button role="tab" aria-selected={selected === i} class:on={selected === i} onclick={() => (selected = i)}>
          Result {grids.length > 1 ? n + 1 : ""}
          <span class="count">{r.rows.length.toLocaleString()}{r.truncated ? "+" : ""}</span>
        </button>
      {/each}
      {#if outcome || runError}
        <button role="tab" aria-selected={selected === "messages"} class:on={selected === "messages"} onclick={() => (selected = "messages")}>
          Messages
          {#if outcome?.error || runError}<CircleAlert size={12} class="err-dot" />{/if}
        </button>
      {/if}
    </div>
    <div class="meta">
      {#if running}
        <span class="running"><LoaderCircle size={13} class="spin" /> Running…</span>
      {:else if current}
        {#if current.truncated}
          <span class="warn" title="Raise the row limit in Settings to fetch more">
            <TriangleAlert size={12} /> first {plural(current.rows.length, "row")}
          </span>
        {:else}
          <span>{plural(current.rows.length, "row")}</span>
        {/if}
        <span>{formatMs(current.elapsedMs)}</span>
        <ExportButton name="result" columns={current.columns.map((c) => c.name)} rows={sortedRows} />
      {:else if outcome}
        <span>{formatMs(outcome.elapsedMs)}</span>
      {/if}
    </div>
  </div>

  <div class="content">
    {#if !outcome && !runError && !running}
      <div class="idle">
        <p>Run a query with <kbd>⌘</kbd> <kbd>↵</kbd>. Select text first to run only that part.</p>
      </div>
    {:else if selected === "messages"}
      <div class="messages scroll selectable">
        {#if runError}
          <div class="msg error"><CircleAlert size={14} /><pre>{runError}</pre></div>
        {/if}
        {#each outcome?.results ?? [] as r, i (i)}
          <div class="msg ok">
            <CircleCheck size={14} />
            <div>
              <pre class="stmt">{r.statement}</pre>
              <p>
                {#if r.columns.length > 0}{plural(r.rows.length, "row")} returned{r.truncated ? " (limited)" : ""}
                {:else if r.rowsAffected !== null}{plural(r.rowsAffected, "row")} affected
                {:else}Done{/if}
                <span class="time">{formatMs(r.elapsedMs)}</span>
              </p>
            </div>
          </div>
        {/each}
        {#if outcome?.error}
          <div class="msg error">
            <CircleAlert size={14} />
            <div>
              <pre class="stmt">{outcome.error.statement}</pre>
              <pre>{outcome.error.message}</pre>
            </div>
          </div>
        {/if}
        {#each outcome?.notices ?? [] as n, i (i)}
          <div class="msg warn"><TriangleAlert size={14} /><p>{n}</p></div>
        {/each}
      </div>
    {:else if current}
      <DataGrid
        columns={current.columns}
        rows={sortedRows}
        {sort}
        onsort={toggleSort}
        emptyText="The query returned no rows"
      />
    {/if}
  </div>
</section>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--surface);
  }
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    height: 34px;
    padding: 0 8px 0 4px;
    border-bottom: 1px solid var(--rule);
    background: var(--surface-2);
  }
  .tabs {
    display: flex;
    align-self: stretch;
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .tabs button {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 12px;
    border: 0;
    background: none;
    color: var(--muted);
    font-weight: 500;
    white-space: nowrap;
    box-shadow: inset 0 -2px 0 transparent;
  }
  .tabs button:hover {
    color: var(--ink);
  }
  .tabs button.on {
    color: var(--ink);
    box-shadow: inset 0 -2px 0 var(--accent);
  }
  .count {
    font-size: var(--text-xs);
    padding: 1px 6px;
    border-radius: 9px;
    background: var(--sunken);
    color: var(--ink-2);
    font-variant-numeric: tabular-nums;
  }
  .tabs :global(.err-dot) {
    color: var(--danger);
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 14px;
    color: var(--muted);
    font-size: var(--text-sm);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .warn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--readonly);
  }
  .running {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--accent);
  }
  .meta :global(.spin) {
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .content {
    flex: 1;
    min-height: 0;
  }
  .messages {
    height: 100%;
    padding: 10px 14px;
  }
  .msg {
    display: flex;
    gap: 10px;
    padding: 8px 0;
    border-bottom: 1px solid var(--rule);
  }
  .msg > :global(svg) {
    flex: none;
    margin-top: 3px;
  }
  .msg.ok > :global(svg) {
    color: var(--ok);
  }
  .msg.error > :global(svg) {
    color: var(--danger);
  }
  .msg.warn > :global(svg) {
    color: var(--readonly);
  }
  .msg p {
    margin: 2px 0 0;
    color: var(--ink-2);
  }
  pre {
    margin: 0;
    font-family: var(--font-code);
    font-size: var(--text-sm);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .msg.error pre:last-child {
    color: var(--danger);
    margin-top: 4px;
  }
  .stmt {
    color: var(--muted);
    max-height: 4.8em;
    overflow: hidden;
  }
  .time {
    margin-left: 8px;
    color: var(--faint);
  }
  .idle {
    display: grid;
    place-items: center;
    height: 100%;
    color: var(--muted);
  }
</style>
