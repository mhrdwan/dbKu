<script lang="ts">
  import { Columns3, Database, Search, Square, Table2 } from "@lucide/svelte";
  import { api, errorMessage } from "../../lib/api";
  import { defaultDatabase, openConnection } from "../../lib/actions";
  import { connections } from "../../lib/state/connections.svelte";
  import { defaultSchema, schema } from "../../lib/state/schema.svelte";
  import { tabs, type SearchTab } from "../../lib/state/tabs.svelte";
  import type { DbObject, ResultSet, SearchHit } from "../../lib/types";
  import { displayValue, plural } from "../../lib/util/format";

  interface Props {
    tab: SearchTab;
  }

  let { tab }: Props = $props();

  type Mode = "names" | "data";
  interface DataHit {
    object: DbObject;
    result: ResultSet;
  }

  const ROWS_PER_TABLE = 20;

  let mode = $state<Mode>("names");
  let term = $state("");
  let includeViews = $state(false);
  let running = $state(false);
  let stopRequested = false;
  let nameHits = $state<SearchHit[] | null>(null);
  let dataHits = $state<DataHit[]>([]);
  let progress = $state({ done: 0, total: 0, current: "" });
  let failures = $state<string[]>([]);
  let error = $state<string | null>(null);

  const profile = $derived(connections.byId(tab.connId));
  const databases = $derived(tab.connId ? (schema.databases[tab.connId] ?? []) : []);
  const schemas = $derived(tab.connId && tab.database ? (schema.schemasOf(tab.connId, tab.database) ?? []) : []);
  const usesSchemas = $derived(schemas.length > 0);

  $effect(() => {
    const id = tab.connId;
    if (!id || !connections.isConnected(id)) return;
    if (!schema.databases[id]) void schema.loadDatabases(id);
    if (!tab.database) tabs.update(tab.id, { database: defaultDatabase(id) });
    else if (!schema.schemasOf(id, tab.database)) void schema.loadSchemas(id, tab.database);
  });

  // Closing the tab stops a running data search.
  $effect(() => () => {
    stopRequested = true;
  });

  async function chooseConnection(id: string) {
    const p = connections.byId(id);
    tabs.update(tab.id, { connId: id || null, database: null, schema: p ? defaultSchema(p.driver) : null });
    if (id && (await openConnection(id))) tabs.update(tab.id, { database: defaultDatabase(id) });
  }

  async function runSearch() {
    const id = tab.connId;
    const q = term.trim();
    if (!id || !tab.database || !q || running) return;
    running = true; // before the first await, so a double submit runs once
    const connected = await openConnection(id);
    running = false;
    if (!connected) return;
    error = null;
    if (mode === "names") await searchNames(id, tab.database, q);
    else await searchData(id, tab.database, q);
  }

  async function searchNames(id: string, db: string, q: string) {
    running = true;
    try {
      const hits = await api.searchObjects(id, db, q);
      nameHits = usesSchemas && tab.schema ? hits.filter((h) => !h.schema || h.schema === tab.schema) : hits;
    } catch (e) {
      error = errorMessage(e);
    } finally {
      running = false;
    }
  }

  async function searchData(id: string, db: string, q: string) {
    const sc = usesSchemas ? tab.schema : null;
    running = true;
    stopRequested = false;
    dataHits = [];
    failures = [];
    try {
      await schema.loadObjects(id, db, sc);
      const objects = (schema.objectsOf(id, db, sc) ?? []).filter((o) => includeViews || o.kind === "table");
      progress = { done: 0, total: objects.length, current: "" };
      for (const object of objects) {
        if (stopRequested) break;
        progress = { ...progress, current: object.name };
        try {
          const result = await api.fetchTable(id, {
            database: db,
            schema: sc,
            table: object.name,
            limit: ROWS_PER_TABLE,
            offset: 0,
            sort: [],
            search: q,
          });
          if (result.rows.length > 0) dataHits = [...dataHits, { object, result }];
        } catch (e) {
          failures = [...failures, `${object.name}: ${errorMessage(e)}`];
        }
        progress = { ...progress, done: progress.done + 1 };
      }
    } catch (e) {
      error = errorMessage(e);
    } finally {
      running = false;
    }
  }

  function openHit(table: string, schemaName: string | null, kind: string, search?: string) {
    if (!tab.connId) return;
    const objectKind = kind === "view" ? "view" : "table";
    tabs.openTable(tab.connId, { database: tab.database, schema: schemaName, table }, objectKind, "data", search);
  }

  /** Columns whose value contains the term, for highlighting. */
  function matchingColumns(r: ResultSet, q: string): Set<number> {
    const needle = q.toLowerCase();
    const hits = r.columns.map((_, i) => i).filter((i) => r.rows.some((row) => row[i]?.toLowerCase().includes(needle)));
    return new Set(hits);
  }
</script>

<div class="search-tab">
  <form class="query" onsubmit={(e) => { e.preventDefault(); void runSearch(); }}>
    <div class="scope">
      <select class="field" value={tab.connId ?? ""} onchange={(e) => chooseConnection(e.currentTarget.value)} aria-label="Connection">
        <option value="">Choose connection…</option>
        {#each connections.profiles as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
      </select>
      {#if databases.length > 0}
        <select class="field" value={tab.database ?? ""} onchange={(e) => tabs.update(tab.id, { database: e.currentTarget.value, schema: profile ? defaultSchema(profile.driver) : null })} aria-label="Database">
          {#each databases as d (d)}<option value={d}>{d}</option>{/each}
        </select>
      {/if}
      {#if usesSchemas}
        <select class="field" value={tab.schema ?? ""} onchange={(e) => tabs.update(tab.id, { schema: e.currentTarget.value })} aria-label="Schema">
          {#each schemas as s (s)}<option value={s}>{s}</option>{/each}
        </select>
      {/if}
    </div>

    <div class="input-row">
      <div class="modes" role="radiogroup" aria-label="What to search">
        <button type="button" role="radio" aria-checked={mode === "names"} class:on={mode === "names"} onclick={() => (mode = "names")}>Table & column names</button>
        <button type="button" role="radio" aria-checked={mode === "data"} class:on={mode === "data"} onclick={() => (mode = "data")}>Data in every table</button>
      </div>
      <label class="term">
        <Search size={16} />
        <input class="field" bind:value={term} placeholder={mode === "names" ? "e.g. invoice, guest_id" : "e.g. an email, a booking number"} aria-label="Search term" />
      </label>
      {#if running && mode === "data"}
        <button type="button" class="btn danger" onclick={() => (stopRequested = true)}><Square size={12} fill="currentColor" /> Stop</button>
      {:else}
        <button class="btn primary" disabled={!tab.connId || !term.trim() || running}>Search</button>
      {/if}
    </div>
    {#if mode === "data"}
      <label class="check"><input type="checkbox" bind:checked={includeViews} /> Include views (can be slow)</label>
    {/if}
  </form>

  <div class="results scroll">
    {#if error}
      <p class="error selectable">{error}</p>
    {/if}

    {#if mode === "names"}
      {#if nameHits === null}
        <p class="hint">Find tables and columns by name across the whole database.</p>
      {:else if nameHits.length === 0}
        <p class="hint">Nothing named like “{term}”.</p>
      {:else}
        <ul class="names">
          {#each nameHits as h, i (i)}
            <li>
              <button onclick={() => openHit(h.table, h.schema, h.kind)}>
                {#if h.column}<Columns3 size={14} />{:else}<Table2 size={14} />{/if}
                <span class="label">
                  {#if h.schema}<span class="dim">{h.schema}.</span>{/if}{h.table}{#if h.column}<span class="col">.{h.column}</span>{/if}
                </span>
                <span class="kind">{h.kind}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    {:else}
      {#if running || progress.total > 0}
        <div class="progress" aria-live="polite">
          <div class="track"><div class="fill" style:width="{progress.total ? (progress.done / progress.total) * 100 : 0}%"></div></div>
          <span>
            {#if running}Searching {progress.current} — {progress.done} of {progress.total} tables
            {:else}Searched {plural(progress.done, "table")}, found matches in {plural(dataHits.length, "table")}{/if}
          </span>
        </div>
      {:else}
        <p class="hint">
          Looks for the text in every column of every table, the way you'd search a spreadsheet. On large
          databases this reads a lot of data, so narrow the scope to a schema where you can.
        </p>
      {/if}

      {#each dataHits as hit (hit.object.name)}
        {@const cols = matchingColumns(hit.result, term)}
        <article class="hit">
          <header>
            <Database size={14} />
            <h3>{hit.object.name}</h3>
            <span class="dim">{hit.result.rows.length >= ROWS_PER_TABLE ? `${ROWS_PER_TABLE}+` : hit.result.rows.length} matching rows</span>
            <button class="btn small" onclick={() => openHit(hit.object.name, usesSchemas ? tab.schema : null, hit.object.kind, term)}>Open filtered</button>
          </header>
          <div class="mini scroll">
            <table>
              <thead>
                <tr>{#each hit.result.columns as c, i (i)}<th class:match={cols.has(i)}>{c.name}</th>{/each}</tr>
              </thead>
              <tbody class="selectable">
                {#each hit.result.rows.slice(0, 5) as row, r (r)}
                  <tr>{#each row as v, i (i)}<td class:match={cols.has(i)} class:null={v === null}>{displayValue(v)}</td>{/each}</tr>
                {/each}
              </tbody>
            </table>
          </div>
        </article>
      {/each}

      {#if failures.length > 0}
        <details class="failures">
          <summary>{plural(failures.length, "table")} couldn't be searched</summary>
          <ul class="selectable">{#each failures as f, i (i)}<li>{f}</li>{/each}</ul>
        </details>
      {/if}
    {/if}
  </div>
</div>

<style>
  .search-tab {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--surface);
  }
  .query {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 18px 22px 16px;
    border-bottom: 1px solid var(--rule);
  }
  .scope {
    display: flex;
    gap: 6px;
  }
  .scope .field {
    max-width: 220px;
  }
  .input-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .modes {
    display: flex;
    padding: 2px;
    border-radius: var(--radius-m);
    background: var(--sunken);
    flex: none;
  }
  .modes button {
    height: 30px;
    padding: 0 12px;
    border: 0;
    border-radius: 4px;
    background: none;
    color: var(--muted);
    font-weight: 500;
  }
  .modes button.on {
    background: var(--surface);
    color: var(--ink);
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.08), 0 0 0 1px var(--rule);
  }
  .term {
    position: relative;
    flex: 1;
    display: flex;
    align-items: center;
  }
  .term :global(svg) {
    position: absolute;
    left: 11px;
    color: var(--faint);
  }
  .term .field {
    width: 100%;
    height: 34px;
    padding-left: 34px;
    font-size: var(--text-md);
  }
  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--ink-2);
  }
  .results {
    flex: 1;
    min-height: 0;
    padding: 16px 22px 40px;
  }
  .hint {
    max-width: 60ch;
    color: var(--muted);
  }
  .error {
    color: var(--danger);
    font-family: var(--font-code);
    white-space: pre-wrap;
  }
  .names {
    margin: 0;
    padding: 0;
    list-style: none;
    max-width: 900px;
  }
  .names button {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 30px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-s);
    background: none;
    text-align: left;
    color: var(--ink-2);
  }
  .names button:hover {
    background: var(--row-hover);
    color: var(--ink);
  }
  .label {
    flex: 1;
    font-weight: 500;
    color: var(--ink);
  }
  .col {
    color: var(--accent);
  }
  .kind,
  .dim {
    color: var(--faint);
    font-size: var(--text-sm);
  }
  .progress {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 16px;
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .track {
    width: 200px;
    height: 4px;
    border-radius: 2px;
    background: var(--sunken);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--accent);
    transition: width var(--duration) var(--ease-out);
  }
  .hit {
    margin-bottom: 18px;
    border: 1px solid var(--rule);
    border-radius: var(--radius-m);
    overflow: hidden;
  }
  .hit header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px 8px 12px;
    background: var(--surface-2);
    border-bottom: 1px solid var(--rule);
  }
  .hit h3 {
    margin: 0;
    font-size: var(--text-md);
    font-weight: 600;
  }
  .hit header .btn {
    margin-left: auto;
  }
  .mini {
    max-height: 190px;
  }
  table {
    border-collapse: collapse;
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
  }
  th,
  td {
    max-width: 260px;
    padding: 4px 10px;
    border-right: 1px solid var(--rule);
    border-bottom: 1px solid var(--rule);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-align: left;
  }
  th {
    font-weight: 600;
    color: var(--muted);
  }
  th.match {
    color: var(--accent);
  }
  td.match {
    background: color-mix(in srgb, var(--edited) 55%, transparent);
  }
  td.null {
    color: var(--null);
    font-style: italic;
  }
  .failures {
    margin-top: 12px;
    color: var(--muted);
  }
  .failures li {
    font-family: var(--font-code);
    font-size: var(--text-sm);
  }
</style>
