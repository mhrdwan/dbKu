<script lang="ts">
  import { Copy, FileCode, Rows3, SquareTerminal, TableProperties } from "@lucide/svelte";
  import { api, errorMessage } from "../../lib/api";
  import { copyText, newQueryFor, selectStatement } from "../../lib/actions";
  import { connections } from "../../lib/state/connections.svelte";
  import { tabs, type TableTab, type TableView } from "../../lib/state/tabs.svelte";
  import SqlEditor from "../query/SqlEditor.svelte";
  import TableData from "./TableData.svelte";
  import TableStructure from "./TableStructure.svelte";

  interface Props {
    tab: TableTab;
  }

  let { tab }: Props = $props();

  let ddl = $state<string | null>(null);
  let ddlError = $state<string | null>(null);
  // Views stay mounted once visited so switching back keeps edits and scroll.
  let visited = $state<Set<TableView>>(new Set());

  const profile = $derived(connections.byId(tab.connId));
  const kindLabel = $derived(
    { table: "Table", view: "View", materialized_view: "Materialized view", foreign_table: "Foreign table" }[tab.objectKind],
  );
  const location = $derived([tab.table.database, tab.table.schema].filter(Boolean).join(" / "));

  $effect(() => {
    if (!visited.has(tab.view)) visited = new Set([...visited, tab.view]);
  });

  $effect(() => {
    if (tab.view !== "ddl" || ddl !== null) return;
    api
      .tableDdl(tab.connId, tab.table)
      .then((d) => (ddl = d))
      .catch((e) => (ddlError = errorMessage(e)));
  });

  const views: { id: TableView; label: string; icon: typeof Rows3 }[] = [
    { id: "data", label: "Data", icon: Rows3 },
    { id: "structure", label: "Structure", icon: TableProperties },
    { id: "ddl", label: "DDL", icon: FileCode },
  ];
</script>

<div class="table-tab">
  <header>
    <div class="title">
      <h1 class="selectable">{tab.table.table}</h1>
      <span class="kind">{kindLabel}</span>
      {#if location}<span class="where">{location}</span>{/if}
    </div>
    <div class="views" role="tablist" aria-label="Table view">
      {#each views as v (v.id)}
        <button role="tab" aria-selected={tab.view === v.id} class:on={tab.view === v.id} onclick={() => tabs.update(tab.id, { view: v.id })}>
          <v.icon size={13} />
          {v.label}
        </button>
      {/each}
    </div>
    <button
      class="btn small ghost"
      title="Open a query on this table"
      onclick={() => newQueryFor(tab.connId, tab.table.database, tab.table.schema, selectStatement(profile?.driver, tab.table))}
    >
      <SquareTerminal size={14} /> Query
    </button>
  </header>

  <div class="body">
    {#if visited.has("data")}
      <div class="view" hidden={tab.view !== "data"}>
        <TableData
          connId={tab.connId}
          table={tab.table}
          objectKind={tab.objectKind}
          initialSearch={tab.search}
          ondirty={(dirty) => tabs.setDirty(tab.id, dirty)}
        />
      </div>
    {/if}
    {#if visited.has("structure")}
      <div class="view" hidden={tab.view !== "structure"}>
        <TableStructure connId={tab.connId} table={tab.table} />
      </div>
    {/if}
    {#if tab.view === "ddl"}
      <div class="view ddl">
        {#if ddlError}
          <p class="error selectable">{ddlError}</p>
        {:else if ddl === null}
          <p class="muted">Loading definition…</p>
        {:else}
          <div class="ddl-bar">
            <button class="btn small" onclick={() => copyText(ddl ?? "", "DDL copied")}><Copy size={13} /> Copy</button>
          </div>
          <div class="ddl-editor">
            <SqlEditor value={ddl} driver={profile?.driver ?? null} readonly />
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .table-tab {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  header {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 10px 12px 10px 16px;
    border-bottom: 1px solid var(--rule);
    background: var(--surface);
  }
  .title {
    display: flex;
    align-items: baseline;
    gap: 10px;
    min-width: 0;
    flex: 1;
  }
  h1 {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: 650;
    letter-spacing: -0.015em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .kind {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .where {
    font-size: var(--text-sm);
    color: var(--faint);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .views {
    display: flex;
    padding: 2px;
    border-radius: var(--radius-m);
    background: var(--sunken);
  }
  .views button {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 10px;
    border: 0;
    border-radius: 4px;
    background: none;
    color: var(--muted);
    font-weight: 500;
  }
  .views button:hover {
    color: var(--ink);
  }
  .views button.on {
    background: var(--surface);
    color: var(--ink);
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.08), 0 0 0 1px var(--rule);
  }
  .body {
    position: relative;
    flex: 1;
    min-height: 0;
    background: var(--surface);
  }
  .view {
    position: absolute;
    inset: 0;
  }
  .view[hidden] {
    display: none;
  }
  .ddl {
    display: flex;
    flex-direction: column;
  }
  .ddl-bar {
    display: flex;
    justify-content: flex-end;
    padding: 8px 12px;
    border-bottom: 1px solid var(--rule);
  }
  .ddl-editor {
    flex: 1;
    min-height: 0;
  }
  .muted {
    padding: 20px;
    color: var(--muted);
  }
  .error {
    padding: 20px;
    color: var(--danger);
    font-family: var(--font-code);
    white-space: pre-wrap;
  }
</style>
