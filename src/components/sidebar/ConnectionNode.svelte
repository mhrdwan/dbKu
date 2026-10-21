<script lang="ts">
  import { ask } from "@tauri-apps/plugin-dialog";
  import {
    Bookmark,
    Copy,
    Database,
    Download,
    FileCode2,
    FileUp,
    Eye,
    FileCode,
    Folder,
    Layers,
    Lock,
    Pencil,
    Plug,
    RefreshCw,
    Search,
    SquareTerminal,
    Table2,
    TableProperties,
    Trash2,
    Unplug,
  } from "@lucide/svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { closeConnection, copyText, newQueryFor, openConnection, selectStatement } from "../../lib/actions";
  import { errorMessage } from "../../lib/api";
  import { connections } from "../../lib/state/connections.svelte";
  import { savedQueries } from "../../lib/state/saved.svelte";
  import { key, schema } from "../../lib/state/schema.svelte";
  import { tabs } from "../../lib/state/tabs.svelte";
  import { dialogs, menu, toasts } from "../../lib/state/ui.svelte";
  import type { ConnectionProfile, DbObject, SavedQuery } from "../../lib/types";
  import PromptDialog from "../ui/PromptDialog.svelte";
  import EngineMark from "../ui/EngineMark.svelte";
  import TreeRow from "./TreeRow.svelte";

  interface Props {
    profile: ConnectionProfile;
    filter: string;
  }

  let { profile, filter }: Props = $props();

  const expanded = new SvelteSet<string>();
  const id = $derived(profile.id);
  const status = $derived(connections.status[id] ?? "idle");
  const usesSchemas = $derived(profile.driver === "postgres" || profile.driver === "mssql");
  const needle = $derived(filter.trim().toLowerCase());
  const activeTab = $derived(tabs.active);
  const queries = $derived(savedQueries.forConnection(profile.id));
  let renaming = $state<SavedQuery | null>(null);

  function defaultDb(): string | null {
    return connections.info[id]?.defaultDatabase ?? schema.databases[id]?.[0] ?? null;
  }

  function exportSql(database: string | null, sc: string | null, only?: string[]) {
    dialogs.transfer = { kind: "export", connId: id, database, schema: sc, only };
  }

  function runFile(database: string | null, sc: string | null) {
    dialogs.transfer = { kind: "script", connId: id, database, schema: sc };
  }

  function queryMenu(e: MouseEvent, q: SavedQuery) {
    menu.show(e, [
      { label: "Open", icon: SquareTerminal, action: () => tabs.openSaved(q, id) },
      { label: "Rename…", icon: Pencil, action: () => (renaming = q) },
      { label: "Copy SQL", icon: Copy, action: () => void copyText(q.sql, "SQL copied") },
      { separator: true, label: "" },
      {
        label: "Delete",
        icon: Trash2,
        danger: true,
        action: async () => {
          const ok = await ask(`Delete the saved query “${q.name}”?`, { title: "Delete saved query", kind: "warning" });
          if (ok) await savedQueries.remove(q.id).catch((err) => toasts.show(errorMessage(err), "error"));
        },
      },
    ]);
  }

  function isOpen(k: string) {
    return expanded.has(k);
  }

  function toggle(k: string, onOpen?: () => void) {
    if (expanded.has(k)) expanded.delete(k);
    else {
      expanded.add(k);
      onOpen?.();
    }
  }

  async function toggleConnection() {
    const k = key(id);
    if (expanded.has(k)) {
      expanded.delete(k);
      return;
    }
    if (await openConnection(id)) {
      expanded.add(k);
      const dbs = schema.databases[id] ?? [];
      // Open the default database straight away; that's where people work.
      const db = connections.info[id]?.defaultDatabase ?? (dbs.length === 1 ? dbs[0] : null);
      if (db) expandDatabase(db, true);
    }
  }

  function expandDatabase(db: string, force = false) {
    const k = key(id, db);
    if (expanded.has(k) && !force) {
      expanded.delete(k);
      return;
    }
    expanded.add(k);
    if (usesSchemas) {
      void schema.loadSchemas(id, db).then(() => {
        const preferred = profile.driver === "postgres" ? "public" : "dbo";
        if ((schema.schemasOf(id, db) ?? []).includes(preferred)) expandSchema(db, preferred, true);
      });
    } else {
      void schema.loadObjects(id, db, null);
    }
  }

  function expandSchema(db: string, sc: string, force = false) {
    const k = key(id, db, sc);
    if (expanded.has(k) && !force) {
      expanded.delete(k);
      return;
    }
    expanded.add(k);
    void schema.loadObjects(id, db, sc);
  }

  function visible(objects: DbObject[] | undefined): DbObject[] {
    const list = objects ?? [];
    return needle ? list.filter((o) => o.name.toLowerCase().includes(needle)) : list;
  }

  function isActiveObject(db: string, sc: string | null, name: string) {
    const t = activeTab;
    return t?.kind === "table" && t.connId === id && t.table.database === db && (t.table.schema ?? null) === sc && t.table.table === name;
  }

  function openObject(db: string, sc: string | null, o: DbObject, view: "data" | "structure" | "ddl" = "data") {
    tabs.openTable(id, { database: db, schema: sc, table: o.name }, o.kind, view);
  }

  async function remove() {
    const ok = await ask(`Delete the connection “${profile.name}”? Its saved password is removed from the keychain too.`, {
      title: "Delete connection",
      kind: "warning",
    });
    if (!ok) return;
    try {
      if (connections.isConnected(id) && !(await closeConnection(id))) return;
      await connections.remove(id);
    } catch (e) {
      toasts.show(errorMessage(e), "error");
    }
  }

  function refresh() {
    schema.forget(id);
    void schema.loadDatabases(id);
    for (const k of expanded) {
      const [, db, sc] = k.split("\u0000");
      if (!db || db.startsWith("\u0001")) continue; // UI folders, not databases
      if (sc) void schema.loadObjects(id, db, sc);
      else if (db) void (usesSchemas ? schema.loadSchemas(id, db) : schema.loadObjects(id, db, null));
    }
  }

  function refreshDatabase(db: string) {
    schema.forgetDatabase(id, db);
    if (!usesSchemas) {
      void schema.loadObjects(id, db, null);
      return;
    }
    void schema.loadSchemas(id, db).then(() => {
      for (const k of expanded) {
        const [, kdb, sc, extra] = k.split("\u0000");
        if (kdb === db && sc && extra === undefined) void schema.loadObjects(id, db, sc);
      }
    });
  }

  function connectionMenu(e: MouseEvent) {
    const connected = status === "connected";
    menu.show(e, [
      connected
        ? { label: "Disconnect", icon: Unplug, action: () => void closeConnection(id) }
        : { label: "Connect", icon: Plug, action: () => void toggleConnection() },
      { label: "New query", icon: SquareTerminal, shortcut: "⌘T", action: () => newQueryFor(id) },
      { label: "Find in database", icon: Search, action: () => tabs.openSearch(id, null, null) },
      { label: "Refresh", icon: RefreshCw, disabled: !connected, action: refresh },
      { separator: true, label: "" },
      { label: "Export as SQL…", icon: Download, disabled: !connected, action: () => exportSql(defaultDb(), usesSchemas ? (profile.driver === "postgres" ? "public" : "dbo") : null) },
      { label: "Run SQL file…", icon: FileCode2, disabled: !connected, action: () => runFile(defaultDb(), null) },
      { separator: true, label: "" },
      { label: "Edit connection…", icon: Pencil, action: () => dialogs.editConnection(id) },
      { label: "Duplicate", icon: Copy, action: () => dialogs.duplicateConnection(id) },
      { label: "Delete", icon: Trash2, danger: true, action: () => void remove() },
    ]);
  }

  function databaseMenu(e: MouseEvent, db: string) {
    menu.show(e, [
      { label: "New query here", icon: SquareTerminal, action: () => newQueryFor(id, db) },
      { label: "Find in this database", icon: Search, action: () => tabs.openSearch(id, db, null) },
      { label: "Export as SQL…", icon: Download, disabled: usesSchemas, action: () => exportSql(db, null) },
      { label: "Run SQL file…", icon: FileCode2, action: () => runFile(db, null) },
      { label: "Refresh", icon: RefreshCw, action: () => refreshDatabase(db) },
      { label: "Copy name", icon: Copy, action: () => void copyText(db) },
    ]);
  }

  function schemaMenu(e: MouseEvent, db: string, sc: string) {
    menu.show(e, [
      { label: "New query here", icon: SquareTerminal, action: () => newQueryFor(id, db, sc) },
      { label: "Find in this schema", icon: Search, action: () => tabs.openSearch(id, db, sc) },
      { label: "Export as SQL…", icon: Download, action: () => exportSql(db, sc) },
      { label: "Run SQL file…", icon: FileCode2, action: () => runFile(db, sc) },
      { label: "Refresh", icon: RefreshCw, action: () => void schema.loadObjects(id, db, sc) },
    ]);
  }

  function objectMenu(e: MouseEvent, db: string, sc: string | null, o: DbObject) {
    const t = { database: db, schema: sc, table: o.name };
    menu.show(e, [
      { label: "Open data", icon: Table2, action: () => openObject(db, sc, o) },
      { label: "Structure", icon: TableProperties, action: () => openObject(db, sc, o, "structure") },
      { label: "DDL", icon: FileCode, action: () => openObject(db, sc, o, "ddl") },
      { separator: true, label: "" },
      { label: "New query with SELECT", icon: SquareTerminal, action: () => newQueryFor(id, db, sc, selectStatement(profile.driver, t)) },
      {
        label: "Import CSV…",
        icon: FileUp,
        disabled: o.kind !== "table" || profile.readOnly,
        action: () => (dialogs.transfer = { kind: "import", connId: id, table: t }),
      },
      { label: "Export as SQL…", icon: Download, action: () => exportSql(db, sc, [o.name]) },
      { label: "Copy name", icon: Copy, action: () => void copyText(o.name) },
    ]);
  }
</script>

{#snippet objectRows(db: string, sc: string | null, depth: number)}
  {@const all = schema.objectsOf(id, db, sc)}
  {@const list = visible(all)}
  {@const tables = list.filter((o) => o.kind === "table" || o.kind === "foreign_table")}
  {@const views = list.filter((o) => o.kind === "view" || o.kind === "materialized_view")}
  {@const err = schema.errorOf("ob", id, db, sc ?? "")}
  {#if err}
    <p class="tree-error selectable" style:padding-left="{depth * 14 + 26}px">{err}</p>
  {:else if all && all.length === 0}
    <p class="tree-empty" style:padding-left="{depth * 14 + 26}px">No tables</p>
  {/if}
  {#each [{ label: "Tables", items: tables, icon: Table2 }, { label: "Views", items: views, icon: Eye }] as group (group.label)}
    {#if group.items.length > 0}
      {@const gk = key(id, db, sc, group.label)}
      <TreeRow
        depth={depth}
        label={group.label}
        expandable
        expanded={!isOpen(gk + "\u0000closed")}
        ontoggle={() => toggle(gk + "\u0000closed")}
      >
        {#snippet icon()}<Folder size={13} class="ico" />{/snippet}
        {#snippet trailing()}<span class="count">{group.items.length}</span>{/snippet}
      </TreeRow>
      {#if !isOpen(gk + "\u0000closed")}
        {#each group.items as o (o.name)}
          <TreeRow
            depth={depth + 1}
            label={o.name}
            title={o.kind === "materialized_view" ? `${o.name} (materialized view)` : o.name}
            active={isActiveObject(db, sc, o.name)}
            onclick={() => openObject(db, sc, o)}
            oncontextmenu={(e) => objectMenu(e, db, sc, o)}
          >
            {#snippet icon()}<group.icon size={13} class="ico" />{/snippet}
          </TreeRow>
        {/each}
      {/if}
    {/if}
  {/each}
{/snippet}

<div role="group">
  <TreeRow
    depth={0}
    label={profile.name}
    title={connections.errors[id] ?? `${profile.name}${profile.host ? ` — ${profile.host}` : ""}`}
    expandable
    expanded={isOpen(key(id))}
    loading={status === "connecting"}
    onclick={toggleConnection}
    ontoggle={toggleConnection}
    oncontextmenu={connectionMenu}
  >
    {#snippet icon()}<EngineMark driver={profile.driver} color={profile.color} size={18} />{/snippet}
    {#snippet trailing()}
      {#if profile.readOnly}<span title="Read-only connection"><Lock size={11} class="ro" /></span>{/if}
      <span class="dot {status}" title={status}></span>
    {/snippet}
  </TreeRow>

  {#if isOpen(key(id)) && queries.length > 0}
    {@const qk = key(id, "\u0001queries")}
    <TreeRow depth={1} label="Saved queries" expandable expanded={!isOpen(qk)} ontoggle={() => toggle(qk)}>
      {#snippet icon()}<Bookmark size={13} class="ico" />{/snippet}
      {#snippet trailing()}<span class="count">{queries.length}</span>{/snippet}
    </TreeRow>
    {#if !isOpen(qk)}
      {#each queries as q (q.id)}
        <TreeRow
          depth={2}
          label={q.name}
          title={q.sql.slice(0, 300)}
          active={activeTab?.kind === "query" && activeTab.savedId === q.id}
          onclick={() => tabs.openSaved(q, id)}
          oncontextmenu={(e) => queryMenu(e, q)}
        >
          {#snippet icon()}<SquareTerminal size={13} class="ico" />{/snippet}
        </TreeRow>
      {/each}
    {/if}
  {/if}

  {#if isOpen(key(id)) && status === "connected"}
    {#each schema.databases[id] ?? [] as db (db)}
      {@const dbKey = key(id, db)}
      <TreeRow
        depth={1}
        label={db}
        expandable
        expanded={isOpen(dbKey)}
        loading={schema.isLoading("sc", id, db) || schema.isLoading("ob", id, db, "")}
        active={activeTab?.kind !== "table" && activeTab?.connId === id && "database" in activeTab && activeTab.database === db}
        onclick={() => expandDatabase(db)}
        ondblclick={() => newQueryFor(id, db)}
        oncontextmenu={(e) => databaseMenu(e, db)}
      >
        {#snippet icon()}<Database size={13} class="ico" />{/snippet}
      </TreeRow>
      {#if isOpen(dbKey)}
        {#if usesSchemas}
          {#each schema.schemasOf(id, db) ?? [] as sc (sc)}
            {@const scKey = key(id, db, sc)}
            <TreeRow
              depth={2}
              label={sc}
              expandable
              expanded={isOpen(scKey)}
              loading={schema.isLoading("ob", id, db, sc)}
              onclick={() => expandSchema(db, sc)}
              oncontextmenu={(e) => schemaMenu(e, db, sc)}
            >
              {#snippet icon()}<Layers size={13} class="ico" />{/snippet}
            </TreeRow>
            {#if isOpen(scKey)}
              {@render objectRows(db, sc, 3)}
            {/if}
          {/each}
        {:else}
          {@render objectRows(db, null, 2)}
        {/if}
      {/if}
    {/each}
  {/if}
</div>

{#if renaming}
  {@const q = renaming}
  <PromptDialog
    title="Rename saved query"
    label="Name"
    value={q.name}
    confirmLabel="Rename"
    onconfirm={async (name) => {
      await savedQueries.save({ ...q, name }).catch((err) => toasts.show(errorMessage(err), "error"));
      tabs.list.filter((t) => t.kind === "query" && t.savedId === q.id).forEach((t) => tabs.update(t.id, { title: name }));
      renaming = null;
    }}
    onclose={() => (renaming = null)}
  />
{/if}

<style>
  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: transparent;
  }
  .dot.connected {
    background: var(--ok);
    box-shadow: 0 0 0 3px var(--ok-soft);
  }
  .dot.error {
    background: var(--danger);
  }
  :global(.ro) {
    color: var(--readonly);
  }
  .count {
    font-size: var(--text-xs);
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }
  .tree-empty,
  .tree-error {
    margin: 2px 0 4px;
    font-size: var(--text-sm);
    color: var(--faint);
  }
  .tree-error {
    color: var(--danger);
    white-space: normal;
    padding-right: 10px;
  }
</style>
