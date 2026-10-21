<script lang="ts">
  import { Bookmark, FileDown, FileUp, History, Lock, Play, Square } from "@lucide/svelte";
  import type { SQLNamespace } from "@codemirror/lang-sql";
  import { untrack } from "svelte";
  import { api, errorMessage } from "../../lib/api";
  import { defaultDatabase, openConnection } from "../../lib/actions";
  import { connections } from "../../lib/state/connections.svelte";
  import { savedQueries } from "../../lib/state/saved.svelte";
  import { defaultSchema, schema } from "../../lib/state/schema.svelte";
  import { settings } from "../../lib/state/settings.svelte";
  import { tabs, type QueryTab } from "../../lib/state/tabs.svelte";
  import { history, menu, toasts } from "../../lib/state/ui.svelte";
  import type { ExecOutcome } from "../../lib/types";
  import { openTextFile, saveTextFile } from "../../lib/util/files";
  import PromptDialog from "../ui/PromptDialog.svelte";
  import Split from "../ui/Split.svelte";
  import ResultPanel from "./ResultPanel.svelte";
  import SqlEditor from "./SqlEditor.svelte";

  interface Props {
    tab: QueryTab;
  }

  let { tab }: Props = $props();

  let editor = $state<SqlEditor>();
  let outcome = $state<ExecOutcome | null>(null);
  let running = $state(false);
  let runError = $state<string | null>(null);
  let runId = $state<string | null>(null);
  /** Connection the current run started on (the picker may change meanwhile). */
  let runConn: string | null = null;
  let ratio = $state(0.46);
  let filePath = $state<string | null>(null);

  const profile = $derived(connections.byId(tab.connId));
  const databases = $derived(tab.connId ? (schema.databases[tab.connId] ?? []) : []);
  const schemas = $derived(tab.connId && tab.database ? (schema.schemasOf(tab.connId, tab.database) ?? []) : []);
  const canCancel = $derived(profile?.driver === "postgres" || profile?.driver === "mysql");

  // Load the pickers' options once connected.
  $effect(() => {
    const id = tab.connId;
    if (!id || !connections.isConnected(id)) return;
    if (!schema.databases[id]) void schema.loadDatabases(id);
    if (tab.database && !schema.schemasOf(id, tab.database)) void schema.loadSchemas(id, tab.database);
    const sc = schemas.length > 0 ? tab.schema : null;
    if (tab.database && !schema.objectsOf(id, tab.database, sc)) void schema.loadObjects(id, tab.database, sc);
  });

  // Table and column names for autocompletion.
  const completion = $derived.by((): SQLNamespace => {
    const id = tab.connId;
    if (!id || !tab.database) return {};
    const sc = schemas.length > 0 ? tab.schema : null;
    const objects = schema.objectsOf(id, tab.database, sc) ?? [];
    return Object.fromEntries(
      objects.map((o) => {
        const cols = schema.cachedColumns(id, { database: tab.database, schema: sc, table: o.name }) ?? [];
        return [o.name, cols.map((c) => ({ label: c.name, type: "property", detail: c.dataType }))];
      }),
    );
  });

  // A fresh query tab is for typing: put the cursor in the editor.
  $effect(() => {
    if (editor && tabs.activeId === tab.id) queueMicrotask(() => editor?.focus());
  });

  // The tab owns a dedicated server connection (its session); give it back
  // when the tab closes or moves to another connection.
  function releaseSession(connId: string | null) {
    if (connId && connections.isConnected(connId)) void api.closeSession(connId, tab.id).catch(() => undefined);
  }

  $effect(() => () => releaseSession(untrack(() => tab.connId)));

  function changeConnection(id: string) {
    releaseSession(tab.connId);
    const p = connections.byId(id);
    tabs.update(tab.id, { connId: id || null, database: null, schema: p ? defaultSchema(p.driver) : null });
    if (id && !connections.isConnected(id)) {
      void openConnection(id).then((ok) => ok && tabs.update(tab.id, { database: defaultDatabase(id) }));
    } else if (id) {
      tabs.update(tab.id, { database: defaultDatabase(id) });
    }
  }

  function changeDatabase(db: string) {
    const p = profile;
    tabs.update(tab.id, { database: db, schema: p ? defaultSchema(p.driver) : null });
  }

  export async function run(sqlText = editor?.currentSql() ?? "") {
    if (running || !sqlText.trim()) return;
    const id = tab.connId;
    if (!id) {
      toasts.show("Choose a connection for this query first.", "info");
      return;
    }
    running = true; // before the first await, so a double ⌘↵ runs once
    if (!(await openConnection(id))) {
      running = false;
      return;
    }
    const database = tab.database ?? defaultDatabase(id);
    if (database !== tab.database) tabs.update(tab.id, { database });
    const thisRun = crypto.randomUUID();
    runId = thisRun;
    runConn = id;
    runError = null;
    const started = performance.now();
    try {
      const result = await api.executeQuery(id, { database, schema: schemas.length > 0 ? tab.schema : null }, sqlText, {
        maxRows: settings.value.maxRows,
        runId: thisRun,
        session: tab.id,
      });
      outcome = result;
      history.add({ sql: sqlText, connId: id, database, at: Date.now(), ok: !result.error, ms: result.elapsedMs });
      if (/\b(create|drop|alter|rename)\b/i.test(sqlText) && database) schema.forgetDatabase(id, database);
    } catch (e) {
      outcome = null;
      runError = errorMessage(e);
      history.add({ sql: sqlText, connId: id, database, at: Date.now(), ok: false, ms: Math.round(performance.now() - started) });
    } finally {
      running = false;
      runId = null;
    }
  }

  async function cancel() {
    if (!runId || !runConn) return;
    try {
      const ok = await api.cancelQuery(runConn, runId);
      if (!ok) toasts.show("The query already finished.", "info");
    } catch (e) {
      toasts.show(`Couldn't stop the query. ${errorMessage(e)}`, "error");
    }
  }

  function showHistory(e: MouseEvent) {
    const entries = history.forConnection(tab.connId).slice(0, 25);
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const items = entries.length
      ? entries.map((h) => ({
          label: (h.ok ? "" : "✕ ") + h.sql.replace(/\s+/g, " ").slice(0, 70),
          action: () => editor?.setValue(h.sql),
        }))
      : [{ label: "No queries yet for this connection", disabled: true }];
    menu.show(new MouseEvent("contextmenu", { clientX: rect.left, clientY: rect.bottom + 4 }), items);
  }

  async function openFile() {
    try {
      const file = await openTextFile([{ name: "SQL", extensions: ["sql", "txt"] }]);
      if (!file) return;
      editor?.setValue(file.content);
      filePath = file.path;
      tabs.update(tab.id, { title: file.path.split(/[\\/]/).pop() ?? tab.title });
    } catch (e) {
      toasts.show(`Couldn't open the file. ${errorMessage(e)}`, "error");
    }
  }

  let naming = $state(false);
  const savedQuery = $derived(savedQueries.byId(tab.savedId));

  /** ⌘S: update the saved query, or ask for a name the first time. */
  async function saveQuery(name?: string) {
    const existing = savedQuery;
    if (!existing && !name) {
      naming = true;
      return;
    }
    try {
      const saved = await savedQueries.save({
        ...(existing ?? {}),
        name: name ?? existing!.name,
        sql: editor?.getValue() ?? "",
        connId: tab.connId,
        database: tab.database,
        schema: tab.schema,
      });
      tabs.update(tab.id, { savedId: saved.id, title: saved.name });
      naming = false;
      toasts.show(`Saved “${saved.name}”`, "success", 1500);
    } catch (e) {
      toasts.show(`Couldn't save the query. ${errorMessage(e)}`, "error");
    }
  }

  async function saveFile() {
    try {
      const path = await saveTextFile(filePath ?? `${tab.title}.sql`, editor?.getValue() ?? "", [
        { name: "SQL", extensions: ["sql"] },
      ]);
      if (!path) return;
      filePath = path;
      tabs.update(tab.id, { title: path.split(/[\\/]/).pop() ?? tab.title });
      toasts.show("Saved", "success", 1500);
    } catch (e) {
      toasts.show(`Couldn't save the file. ${errorMessage(e)}`, "error");
    }
  }
</script>

<div class="query">
  <div class="toolbar">
    <select class="field conn" disabled={running} value={tab.connId ?? ""} onchange={(e) => changeConnection(e.currentTarget.value)} aria-label="Connection">
      <option value="">Choose connection…</option>
      {#each connections.profiles as p (p.id)}
        <option value={p.id}>{p.name}</option>
      {/each}
    </select>
    {#if databases.length > 0}
      <select class="field" value={tab.database ?? ""} onchange={(e) => changeDatabase(e.currentTarget.value)} aria-label="Database">
        {#each databases as db (db)}<option value={db}>{db}</option>{/each}
      </select>
    {/if}
    {#if schemas.length > 0}
      <select class="field" value={tab.schema ?? ""} onchange={(e) => tabs.update(tab.id, { schema: e.currentTarget.value })} aria-label="Schema">
        {#each schemas as s (s)}<option value={s}>{s}</option>{/each}
      </select>
    {/if}

    {#if running && canCancel}
      <button class="btn danger" onclick={cancel}><Square size={12} fill="currentColor" /> Stop</button>
    {:else}
      <button class="btn primary" onclick={() => run()} disabled={running} title="Run selection or whole script (⌘↵)">
        <Play size={13} fill="currentColor" /> Run
      </button>
    {/if}

    {#if profile?.readOnly}
      <span class="ro" title="Only reading statements are allowed on this connection"><Lock size={12} /> Read-only</span>
    {/if}

    <span class="spacer"></span>
    <button class="btn ghost small" onclick={showHistory} title="Recent queries"><History size={14} /> History</button>
    <button class="btn ghost small" onclick={() => saveQuery()} title={savedQuery ? `Update “${savedQuery.name}” (⌘S)` : "Save to Saved queries (⌘S)"}>
      <Bookmark size={14} fill={savedQuery ? "currentColor" : "none"} /> {savedQuery ? "Saved" : "Save"}
    </button>
    <button class="btn ghost small icon" onclick={openFile} title="Open .sql file" aria-label="Open SQL file"><FileUp size={14} /></button>
    <button class="btn ghost small icon" onclick={saveFile} title="Save as .sql file" aria-label="Save as SQL file"><FileDown size={14} /></button>
  </div>

  <div class="work">
    <Split bind:ratio>
      {#snippet first()}
        <SqlEditor
          bind:this={editor}
          value={tab.initialSql}
          driver={profile?.driver ?? null}
          schema={completion}
          fontSize={settings.value.editorFontSize}
          placeholder="SELECT * FROM …"
          onrun={(sql) => run(sql)}
          onsave={() => saveQuery()}
        />
      {/snippet}
      {#snippet second()}
        <ResultPanel {outcome} {running} {runError} />
      {/snippet}
    </Split>
  </div>
</div>

{#if naming}
  <PromptDialog
    title="Save query"
    label="Name"
    value={tab.title.startsWith("Query ") ? "" : tab.title}
    confirmLabel="Save"
    onconfirm={(name) => saveQuery(name)}
    onclose={() => (naming = false)}
  />
{/if}

<style>
  .query {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 7px 10px;
    border-bottom: 1px solid var(--rule);
    background: var(--surface);
  }
  .toolbar .field {
    max-width: 180px;
  }
  .toolbar .conn {
    max-width: 220px;
    font-weight: 500;
  }
  .spacer {
    flex: 1;
  }
  .ro {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 22px;
    padding: 0 8px;
    border-radius: 11px;
    background: var(--readonly-soft);
    color: var(--readonly);
    font-size: var(--text-sm);
    font-weight: 600;
  }
  .work {
    flex: 1;
    min-height: 0;
  }
</style>
