<script lang="ts">
  import { FileUp, TriangleAlert } from "@lucide/svelte";
  import { api, errorMessage } from "../../lib/api";
  import { connections } from "../../lib/state/connections.svelte";
  import { schema } from "../../lib/state/schema.svelte";
  import { dataVersions, dialogs, toasts } from "../../lib/state/ui.svelte";
  import type { ColumnInfo, CsvPreview, PickedFile, TableRef } from "../../lib/types";
  import { autoMap } from "../../lib/util/mapping";
  import { plural } from "../../lib/util/format";
  import Modal from "../ui/Modal.svelte";
  import TaskProgress from "./TaskProgress.svelte";

  interface Props {
    connId: string;
    table: TableRef;
  }

  let { connId, table }: Props = $props();

  const CANCELLED = "Cancelled.";
  const DELIMITERS = [
    { value: ",", label: "Comma ," },
    { value: ";", label: "Semicolon ;" },
    { value: "\\t", label: "Tab" },
    { value: "|", label: "Pipe |" },
  ];

  let columns = $state<ColumnInfo[]>([]);
  let file = $state<PickedFile | null>(null);
  let preview = $state<CsvPreview | null>(null);
  let delimiter = $state<string | null>(null);
  let hasHeader = $state(true);
  let emptyAsNull = $state(true);
  let truncateFirst = $state(false);
  let sources = $state<number[]>([]);
  let taskId = $state<string | null>(null);
  let error = $state<string | null>(null);

  const profile = $derived(connections.byId(connId));
  const mapped = $derived(sources.filter((s) => s >= 0).length);

  $effect(() => {
    void schema.loadColumns(connId, table).then((c) => (columns = c)).catch((e) => (error = errorMessage(e)));
  });

  async function choose() {
    error = null;
    const picked = await api.pickFile("csv");
    if (!picked) return;
    file = picked;
    delimiter = null;
    await refresh();
  }

  async function refresh() {
    if (!file) return;
    try {
      preview = await api.previewCsv(file.token, delimiter, hasHeader);
      delimiter = preview.delimiter;
      sources = autoMap(columns.map((c) => c.name), preview.headers, hasHeader);
    } catch (e) {
      preview = null;
      error = errorMessage(e);
    }
  }

  async function start() {
    if (!file || !preview) return;
    error = null;
    taskId = crypto.randomUUID();
    try {
      const mapping = columns
        .map((c, i) => ({ column: c.name, source: sources[i] }))
        .filter((m) => m.source >= 0);
      const rows = await api.importCsv(connId, file.token, taskId, table, {
        delimiter: delimiter ?? ",",
        hasHeader,
        emptyAsNull,
        truncateFirst,
        mapping,
      });
      dataVersions.bump(connId, table);
      toasts.show(`Imported ${plural(rows, "row")} into ${table.table}`, "success");
      dialogs.transfer = null;
    } catch (e) {
      const message = errorMessage(e);
      if (message === CANCELLED) toasts.show("Import cancelled. Nothing was saved.", "info");
      else error = `${message}\nNothing was saved; the table is unchanged.`;
    } finally {
      taskId = null;
    }
  }

  const close = () => (dialogs.transfer = null);
</script>

<Modal title="Import CSV into {table.table}" width={720} onclose={close}>
  {#if profile?.readOnly}
    <p class="warn"><TriangleAlert size={14} /> This connection is read-only, so imports are turned off.</p>
  {:else}
    <div class="file">
      <button class="btn" onclick={choose} disabled={taskId !== null}><FileUp size={14} /> {file ? "Choose another file…" : "Choose CSV file…"}</button>
      {#if file}<span class="name">{file.name}</span>{/if}
    </div>

    {#if preview}
      <div class="options">
        <label>
          Delimiter
          <select class="field" bind:value={delimiter} onchange={refresh}>
            {#each DELIMITERS as d (d.value)}<option value={d.value}>{d.label}</option>{/each}
          </select>
        </label>
        <label><input type="checkbox" bind:checked={hasHeader} onchange={refresh} /> First row is a header</label>
        <label><input type="checkbox" bind:checked={emptyAsNull} /> Empty fields become NULL</label>
      </div>

      <div class="mapping scroll">
        <table>
          <thead>
            <tr><th>Table column</th><th>CSV field</th><th>First values</th></tr>
          </thead>
          <tbody>
            {#each columns as c, i (c.name)}
              <tr class:skipped={sources[i] < 0}>
                <td class="col">{c.name} <span class="type">{c.dataType}</span></td>
                <td>
                  <select class="field" bind:value={sources[i]} aria-label="Source for {c.name}">
                    <option value={-1}>Skip (use default)</option>
                    {#each preview.headers as h, j (j)}<option value={j}>{h || `Column ${j + 1}`}</option>{/each}
                  </select>
                </td>
                <td class="sample">
                  {sources[i] >= 0 ? preview.rows.slice(0, 3).map((r) => r[sources[i]] ?? "").join(", ") : ""}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>

      <label class="danger-option" class:on={truncateFirst}>
        <input type="checkbox" bind:checked={truncateFirst} />
        Delete all existing rows in {table.table} first
      </label>
      <p class="note">
        The whole file is imported in one transaction: if any row fails, nothing is saved. The file must be UTF-8.
      </p>
    {/if}

    {#if taskId}<TaskProgress {taskId} unit="bytes" />{/if}
    {#if error}<p class="error selectable">{error}</p>{/if}
  {/if}

  {#snippet footer()}
    <span class="spacer"></span>
    <button class="btn" onclick={close} disabled={taskId !== null}>Cancel</button>
    <button class="btn primary" onclick={start} disabled={!preview || mapped === 0 || taskId !== null || profile?.readOnly}>
      Import {mapped > 0 ? `${mapped} column${mapped === 1 ? "" : "s"}` : ""}
    </button>
  {/snippet}
</Modal>

<style>
  .file {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .name {
    font-family: var(--font-code);
    font-size: var(--text-sm);
    color: var(--ink-2);
  }
  .options {
    display: flex;
    align-items: center;
    gap: 18px;
    margin: 14px 0 10px;
  }
  label {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  .mapping {
    max-height: 300px;
    border: 1px solid var(--rule);
    border-radius: var(--radius-m);
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th {
    position: sticky;
    top: 0;
    background: var(--surface-2);
    text-align: left;
    font-weight: 500;
    font-size: var(--text-sm);
    color: var(--muted);
    padding: 6px 10px;
    border-bottom: 1px solid var(--rule);
  }
  td {
    padding: 4px 10px;
    border-bottom: 1px solid var(--rule);
  }
  td .field {
    width: 100%;
    height: 26px;
  }
  .col {
    font-weight: 600;
    white-space: nowrap;
  }
  .type {
    font-weight: 400;
    font-size: var(--text-xs);
    color: var(--faint);
  }
  .sample {
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  tr.skipped .col {
    color: var(--faint);
  }
  .danger-option {
    margin-top: 12px;
    padding: 8px 10px;
    border-radius: var(--radius-m);
  }
  .danger-option.on {
    background: var(--danger-soft);
    color: var(--danger);
    font-weight: 600;
  }
  .note {
    margin: 8px 0;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .warn {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--readonly);
  }
  .error {
    color: var(--danger);
    white-space: pre-wrap;
  }
  .spacer {
    flex: 1;
  }
</style>
