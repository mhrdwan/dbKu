<script lang="ts">
  import { CircleCheck, Eye, Table2 } from "@lucide/svelte";
  import { api, errorMessage } from "../../lib/api";
  import { connections } from "../../lib/state/connections.svelte";
  import { schema } from "../../lib/state/schema.svelte";
  import { dialogs, toasts } from "../../lib/state/ui.svelte";
  import type { DbObject, DumpOptions, DumpSummary } from "../../lib/types";
  import { plural } from "../../lib/util/format";
  import Modal from "../ui/Modal.svelte";
  import TaskProgress from "./TaskProgress.svelte";

  interface Props {
    connId: string;
    database: string | null;
    schemaName: string | null;
    only?: string[];
  }

  let { connId, database, schemaName, only }: Props = $props();

  const CANCELLED = "Cancelled.";
  let objects = $state<DbObject[] | null>(null);
  let chosen = $state<Set<string>>(new Set());
  let options = $state<DumpOptions>({ structure: true, data: true, dropExisting: true, rowsPerInsert: 500 });
  let taskId = $state<string | null>(null);
  let done = $state<DumpSummary | null>(null);
  let error = $state<string | null>(null);

  const profile = $derived(connections.byId(connId));
  const where = $derived([profile?.name, database, schemaName].filter(Boolean).join(" / "));

  $effect(() => {
    if (!database) return;
    void schema.loadObjects(connId, database, schemaName).then(() => {
      const all = schema.objectsOf(connId, database, schemaName) ?? [];
      objects = only ? all.filter((o) => only.includes(o.name)) : all;
      chosen = new Set(objects.map((o) => o.name));
    });
  });

  function toggle(name: string) {
    const next = new Set(chosen);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    chosen = next;
  }

  function toggleAll() {
    chosen = chosen.size === (objects?.length ?? 0) ? new Set() : new Set(objects?.map((o) => o.name));
  }

  async function start() {
    error = null;
    const file = await api.pickFile("dump", `${only?.length === 1 ? only[0] : (database ?? "dump")}.sql`);
    if (!file) return;
    taskId = crypto.randomUUID();
    try {
      done = await api.exportDump({
        id: connId,
        token: file.token,
        taskId,
        database,
        schema: schemaName,
        objects: (objects ?? []).filter((o) => chosen.has(o.name)),
        options,
      });
    } catch (e) {
      const message = errorMessage(e);
      if (message === CANCELLED) toasts.show("Export cancelled. No file was written.", "info");
      else error = message;
    } finally {
      taskId = null;
    }
  }

  const close = () => (dialogs.transfer = null);
</script>

<Modal title="Export as SQL" width={560} onclose={close}>
  {#if done}
    <div class="done">
      <CircleCheck size={20} />
      <div>
        <p><strong>Exported {plural(done.objects, "object")} and {plural(done.rows, "row")}.</strong></p>
        <p class="path selectable">{done.path}</p>
        <p class="muted">Run this file with “Run SQL file…” (or any SQL client) to recreate the data.</p>
      </div>
    </div>
  {:else}
    <p class="lead">{where}</p>
    <div class="objects scroll">
      {#if objects === null}
        <p class="muted">Loading tables…</p>
      {:else if objects.length === 0}
        <p class="muted">Nothing to export here.</p>
      {:else}
        <label class="all">
          <input type="checkbox" checked={chosen.size === objects.length} indeterminate={chosen.size > 0 && chosen.size < objects.length} onchange={toggleAll} />
          {chosen.size} of {objects.length} selected
        </label>
        {#each objects as o (o.name)}
          <label class="object">
            <input type="checkbox" checked={chosen.has(o.name)} onchange={() => toggle(o.name)} disabled={taskId !== null} />
            {#if o.kind === "table"}<Table2 size={13} />{:else}<Eye size={13} />{/if}
            {o.name}
          </label>
        {/each}
      {/if}
    </div>

    <fieldset disabled={taskId !== null}>
      <label><input type="checkbox" bind:checked={options.structure} /> Structure (CREATE statements, indexes, foreign keys)</label>
      <label><input type="checkbox" bind:checked={options.data} /> Data (INSERT statements)</label>
      <label><input type="checkbox" bind:checked={options.dropExisting} disabled={!options.structure} /> Drop existing objects first</label>
      <label class="rows">
        Rows per INSERT
        <select class="field" bind:value={options.rowsPerInsert}>
          {#each [1, 100, 500, 1000] as n (n)}<option value={n}>{n}</option>{/each}
        </select>
      </label>
    </fieldset>

    <p class="note">
      Covers tables, keys, indexes, views and data. Custom types, functions, triggers (except SQLite)
      and permissions are not included; use the database's own dump tool for a full backup.
    </p>

    {#if taskId}
      <TaskProgress {taskId} noun="tables" />
    {/if}
    {#if error}<p class="error selectable">{error}</p>{/if}
  {/if}

  {#snippet footer()}
    <span class="spacer"></span>
    {#if done}
      <button class="btn primary" onclick={close}>Done</button>
    {:else}
      <button class="btn" onclick={close} disabled={taskId !== null}>Cancel</button>
      <button class="btn primary" onclick={start} disabled={taskId !== null || chosen.size === 0 || (!options.structure && !options.data)}>
        Choose file & export
      </button>
    {/if}
  {/snippet}
</Modal>

<style>
  .lead {
    margin: 0 0 10px;
    color: var(--muted);
  }
  .objects {
    max-height: 220px;
    padding: 6px 8px;
    border: 1px solid var(--rule);
    border-radius: var(--radius-m);
    background: var(--surface-2);
  }
  label {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 3px 0;
  }
  .all {
    padding-bottom: 6px;
    margin-bottom: 4px;
    border-bottom: 1px solid var(--rule);
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .object :global(svg) {
    color: var(--muted);
  }
  fieldset {
    margin: 14px 0 0;
    padding: 0;
    border: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .rows {
    gap: 10px;
  }
  .rows .field {
    width: 90px;
  }
  .note {
    margin: 12px 0;
    font-size: var(--text-sm);
    color: var(--muted);
    max-width: 62ch;
  }
  .done {
    display: flex;
    gap: 12px;
    color: var(--ink);
  }
  .done > :global(svg) {
    flex: none;
    color: var(--ok);
    margin-top: 2px;
  }
  .done p {
    margin: 0 0 6px;
  }
  .path {
    font-family: var(--font-code);
    font-size: var(--text-sm);
    word-break: break-all;
  }
  .muted {
    color: var(--muted);
  }
  .error {
    margin: 10px 0 0;
    color: var(--danger);
    white-space: pre-wrap;
  }
  .spacer {
    flex: 1;
  }
</style>
