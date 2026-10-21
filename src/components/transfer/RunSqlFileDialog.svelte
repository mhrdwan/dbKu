<script lang="ts">
  import { CircleAlert, CircleCheck, FileUp, Lock } from "@lucide/svelte";
  import { api, errorMessage } from "../../lib/api";
  import { connections } from "../../lib/state/connections.svelte";
  import { defaultSchema, schema } from "../../lib/state/schema.svelte";
  import { dialogs, toasts } from "../../lib/state/ui.svelte";
  import type { PickedFile, ScriptOutcome } from "../../lib/types";
  import { plural } from "../../lib/util/format";
  import Modal from "../ui/Modal.svelte";
  import TaskProgress from "./TaskProgress.svelte";

  interface Props {
    connId: string;
    database: string | null;
    schemaName: string | null;
  }

  let { connId, database, schemaName }: Props = $props();

  let file = $state<PickedFile | null>(null);
  // svelte-ignore state_referenced_locally — starts from where it was opened.
  let db = $state(database);
  // svelte-ignore state_referenced_locally
  let sc = $state(schemaName);
  let taskId = $state<string | null>(null);
  let outcome = $state<ScriptOutcome | null>(null);
  let error = $state<string | null>(null);

  const profile = $derived(connections.byId(connId));
  const databases = $derived(schema.databases[connId] ?? []);
  const schemas = $derived(db ? (schema.schemasOf(connId, db) ?? []) : []);

  $effect(() => {
    if (db && !schema.schemasOf(connId, db)) void schema.loadSchemas(connId, db);
  });

  // Opened from a database (no schema yet): start in its usual schema.
  $effect(() => {
    if (schemas.length === 0 || (sc && schemas.includes(sc))) return;
    const preferred = profile ? defaultSchema(profile.driver) : null;
    sc = preferred && schemas.includes(preferred) ? preferred : schemas[0];
  });

  async function choose() {
    const picked = await api.pickFile("sql");
    if (picked) {
      file = picked;
      outcome = null;
      error = null;
    }
  }

  async function run() {
    if (!file) return;
    error = null;
    outcome = null;
    taskId = crypto.randomUUID();
    try {
      outcome = await api.runSqlFile(connId, file.token, taskId, { database: db, schema: schemas.length > 0 ? sc : null });
      if (db) schema.forgetDatabase(connId, db);
      if (!outcome.error && !outcome.cancelled) {
        toasts.show(`Ran ${plural(outcome.executed, "statement")} from ${file.name}`, "success");
      }
    } catch (e) {
      error = errorMessage(e);
    } finally {
      taskId = null;
    }
  }

  function size(n: number | null) {
    if (n === null) return "";
    return n < 1024 * 1024 ? `${Math.max(1, Math.round(n / 1024))} KB` : `${(n / 1024 / 1024).toFixed(1)} MB`;
  }

  const close = () => (dialogs.transfer = null);
</script>

<Modal title="Run SQL file" width={560} onclose={close}>
  <div class="file">
    <button class="btn" onclick={choose} disabled={taskId !== null}><FileUp size={14} /> {file ? "Choose another file…" : "Choose .sql file…"}</button>
    {#if file}<span class="name">{file.name} <span class="size">{size(file.size)}</span></span>{/if}
  </div>

  <div class="target">
    <span>Run on</span>
    <strong>{profile?.name}</strong>
    {#if databases.length > 0}
      <select class="field" bind:value={db} disabled={taskId !== null} aria-label="Database">
        {#each databases as d (d)}<option value={d}>{d}</option>{/each}
      </select>
    {/if}
    {#if schemas.length > 0}
      <select class="field" bind:value={sc} disabled={taskId !== null} aria-label="Schema">
        {#each schemas as s (s)}<option value={s}>{s}</option>{/each}
      </select>
    {/if}
  </div>
  {#if profile?.readOnly}
    <p class="ro"><Lock size={12} /> Read-only connection: the file only runs if every statement is a read.</p>
  {/if}
  <p class="note">
    Statements run one after another and stop at the first error. Statements before it stay applied
    unless the file itself wraps them in a transaction.
  </p>

  {#if taskId}<TaskProgress {taskId} noun="statements" />{/if}

  {#if outcome}
    {#if outcome.error}
      <div class="result bad selectable">
        <CircleAlert size={16} />
        <div>
          <p><strong>Stopped at statement {outcome.error.index + 1} of {outcome.total}.</strong> {plural(outcome.executed, "statement")} ran before it.</p>
          <pre class="stmt">{outcome.error.statement}</pre>
          <pre>{outcome.error.message}</pre>
        </div>
      </div>
    {:else if outcome.cancelled}
      <p class="muted">Stopped after {plural(outcome.executed, "statement")} of {outcome.total}.</p>
    {:else}
      <div class="result good">
        <CircleCheck size={16} />
        <p>Ran all {plural(outcome.executed, "statement")}{outcome.rowsAffected ? `, ${plural(outcome.rowsAffected, "row")} affected` : ""}.</p>
      </div>
    {/if}
  {/if}
  {#if error}<p class="error selectable">{error}</p>{/if}

  {#snippet footer()}
    <span class="spacer"></span>
    <button class="btn" onclick={close} disabled={taskId !== null}>{outcome ? "Close" : "Cancel"}</button>
    <button class="btn primary" onclick={run} disabled={!file || taskId !== null}>Run file</button>
  {/snippet}
</Modal>

<style>
  .file,
  .target {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .target {
    margin: 14px 0 6px;
  }
  .target .field {
    max-width: 180px;
  }
  .name {
    font-family: var(--font-code);
    font-size: var(--text-sm);
  }
  .size {
    color: var(--faint);
  }
  .ro {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--readonly);
    font-size: var(--text-sm);
  }
  .note,
  .muted {
    color: var(--muted);
    font-size: var(--text-sm);
    max-width: 62ch;
  }
  .result {
    display: flex;
    gap: 10px;
    margin-top: 12px;
    padding: 10px 12px;
    border-radius: var(--radius-m);
  }
  .result :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .result p {
    margin: 0;
  }
  .good {
    background: var(--ok-soft);
    color: var(--ok);
  }
  .bad {
    background: var(--danger-soft);
    color: var(--danger);
  }
  pre {
    margin: 6px 0 0;
    font: var(--text-sm) / 1.5 var(--font-code);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .stmt {
    color: var(--ink-2);
    max-height: 6em;
    overflow: hidden;
  }
  .error {
    color: var(--danger);
    white-space: pre-wrap;
  }
  .spacer {
    flex: 1;
  }
</style>
