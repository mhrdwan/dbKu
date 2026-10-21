<script lang="ts">
  import { KeyRound } from "@lucide/svelte";
  import { api, errorMessage } from "../../lib/api";
  import { schema } from "../../lib/state/schema.svelte";
  import type { ColumnInfo, IndexInfo, TableRef } from "../../lib/types";

  interface Props {
    connId: string;
    table: TableRef;
  }

  let { connId, table }: Props = $props();

  let columns = $state<ColumnInfo[]>([]);
  let indexes = $state<IndexInfo[]>([]);
  let error = $state<string | null>(null);
  let loading = $state(true);

  $effect(() => {
    loading = true;
    Promise.all([schema.loadColumns(connId, table), api.tableIndexes(connId, table)])
      .then(([c, i]) => {
        columns = c;
        indexes = i;
      })
      .catch((e) => (error = errorMessage(e)))
      .finally(() => (loading = false));
  });
</script>

<div class="structure scroll">
  {#if error}
    <p class="error selectable">{error}</p>
  {:else if loading}
    <p class="muted">Loading structure…</p>
  {:else}
    <section>
      <h3>Columns <span>{columns.length}</span></h3>
      <table>
        <thead>
          <tr><th class="n">#</th><th>Name</th><th>Type</th><th>Null</th><th>Default</th><th>Comment</th></tr>
        </thead>
        <tbody class="selectable">
          {#each columns as c, i (c.name)}
            <tr>
              <td class="n">{i + 1}</td>
              <td class="name">
                {#if c.isPrimaryKey}<KeyRound size={12} class="key" aria-label="Primary key" />{/if}
                {c.name}
                {#if c.isAutoIncrement}<span class="tag">auto</span>{/if}
              </td>
              <td class="code">{c.dataType}</td>
              <td>{c.nullable ? "yes" : "no"}</td>
              <td class="code dim">{c.defaultValue ?? ""}</td>
              <td class="dim">{c.comment ?? ""}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </section>

    <section>
      <h3>Indexes <span>{indexes.length}</span></h3>
      {#if indexes.length === 0}
        <p class="muted">This table has no indexes.</p>
      {:else}
        <table>
          <thead><tr><th>Name</th><th>Columns</th><th>Kind</th></tr></thead>
          <tbody class="selectable">
            {#each indexes as ix (ix.name)}
              <tr>
                <td class="name">{ix.name}</td>
                <td class="code">{ix.columns.join(", ")}</td>
                <td>{ix.isPrimary ? "primary key" : ix.isUnique ? "unique" : "index"}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </section>
  {/if}
</div>

<style>
  .structure {
    height: 100%;
    padding: 18px 22px 40px;
  }
  section + section {
    margin-top: 28px;
  }
  h3 {
    display: flex;
    align-items: baseline;
    gap: 8px;
    margin: 0 0 10px;
    font-size: var(--text-md);
    font-weight: 600;
  }
  h3 span {
    color: var(--faint);
    font-weight: 500;
    font-size: var(--text-sm);
  }
  table {
    width: 100%;
    max-width: 1100px;
    border-collapse: collapse;
    font-variant-numeric: tabular-nums;
  }
  th {
    text-align: left;
    font-weight: 500;
    font-size: var(--text-sm);
    color: var(--muted);
    padding: 6px 10px;
    border-bottom: 1px solid var(--rule-strong);
  }
  td {
    padding: 6px 10px;
    border-bottom: 1px solid var(--rule);
    vertical-align: top;
  }
  tbody tr:hover {
    background: var(--row-hover);
  }
  .n {
    width: 36px;
    color: var(--faint);
    text-align: right;
  }
  .name {
    font-weight: 600;
    white-space: nowrap;
  }
  .name :global(.key) {
    color: var(--readonly);
    margin-right: 4px;
    vertical-align: -1px;
  }
  .code {
    font-family: var(--font-code);
    font-size: var(--text-sm);
  }
  .dim {
    color: var(--muted);
  }
  .tag {
    margin-left: 6px;
    padding: 0 5px;
    border-radius: 4px;
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--ok);
    background: var(--ok-soft);
  }
  .muted {
    color: var(--muted);
  }
  .error {
    color: var(--danger);
    font-family: var(--font-code);
    white-space: pre-wrap;
  }
</style>
