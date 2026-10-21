import type { ObjectKind, SavedQuery, TableRef } from "../types";

export type TableView = "data" | "structure" | "ddl";

interface TabBase {
  id: string;
  title: string;
  connId: string | null;
}

export interface QueryTab extends TabBase {
  kind: "query";
  database: string | null;
  schema: string | null;
  initialSql: string;
  /** Set when the tab edits a saved query; ⌘S updates it. */
  savedId?: string;
}

export interface TableTab extends TabBase {
  kind: "table";
  connId: string;
  table: TableRef;
  objectKind: ObjectKind;
  view: TableView;
  /** Pre-filled quick search, e.g. when opened from "Find in database". */
  search?: string;
}

export interface SearchTab extends TabBase {
  kind: "search";
  database: string | null;
  schema: string | null;
}

export type Tab = QueryTab | TableTab | SearchTab;

let counter = 0;
const nextId = () => `tab-${Date.now().toString(36)}-${(counter++).toString(36)}`;

function tableKey(connId: string, t: TableRef) {
  return [connId, t.database ?? "", t.schema ?? "", t.table].join("/");
}

class Tabs {
  list = $state<Tab[]>([]);
  activeId = $state<string | null>(null);
  /** Tabs holding unsaved grid edits. */
  dirty = $state<Set<string>>(new Set());
  private queryCount = 0;

  get active(): Tab | undefined {
    return this.list.find((t) => t.id === this.activeId);
  }

  activate(id: string) {
    this.activeId = id;
  }

  private add(tab: Tab) {
    const index = this.list.findIndex((t) => t.id === this.activeId);
    const at = index < 0 ? this.list.length : index + 1;
    this.list = [...this.list.slice(0, at), tab, ...this.list.slice(at)];
    this.activeId = tab.id;
  }

  newQuery(connId: string | null, database: string | null, schema: string | null, sql = "") {
    this.queryCount += 1;
    this.add({
      kind: "query",
      id: nextId(),
      title: `Query ${this.queryCount}`,
      connId,
      database,
      schema,
      initialSql: sql,
    });
  }

  openSaved(q: SavedQuery, connId: string | null) {
    const existing = this.list.find((t) => t.kind === "query" && t.savedId === q.id);
    if (existing) {
      this.activeId = existing.id;
      return;
    }
    this.add({
      kind: "query",
      id: nextId(),
      title: q.name,
      connId: q.connId ?? connId,
      database: q.database,
      schema: q.schema,
      initialSql: q.sql,
      savedId: q.id,
    });
  }

  openTable(connId: string, table: TableRef, objectKind: ObjectKind, view: TableView = "data", search?: string) {
    const k = tableKey(connId, table);
    const existing = this.list.find((t) => t.kind === "table" && tableKey(t.connId, t.table) === k);
    if (existing && existing.kind === "table") {
      this.update(existing.id, { view, ...(search !== undefined ? { search } : {}) });
      this.activeId = existing.id;
      return;
    }
    this.add({ kind: "table", id: nextId(), title: table.table, connId, table, objectKind, view, search });
  }

  openSearch(connId: string | null, database: string | null, schema: string | null) {
    this.add({ kind: "search", id: nextId(), title: "Find in database", connId, database, schema });
  }

  setDirty(id: string, isDirty: boolean) {
    if (this.dirty.has(id) === isDirty) return;
    const next = new Set(this.dirty);
    if (isDirty) next.add(id);
    else next.delete(id);
    this.dirty = next;
  }

  update(id: string, patch: Partial<Tab>) {
    this.list = this.list.map((t) => (t.id === id ? ({ ...t, ...patch } as Tab) : t));
  }

  close(id: string) {
    const index = this.list.findIndex((t) => t.id === id);
    if (index < 0) return;
    this.list = this.list.filter((t) => t.id !== id);
    this.setDirty(id, false);
    if (this.activeId === id) {
      const next = this.list[Math.min(index, this.list.length - 1)];
      this.activeId = next?.id ?? null;
    }
  }

  closeOthers(id: string) {
    this.list = this.list.filter((t) => t.id === id);
    this.activeId = id;
  }

  closeForConnection(connId: string) {
    const remaining = this.list.filter((t) => !(t.kind === "table" && t.connId === connId));
    this.list = remaining;
    if (!remaining.some((t) => t.id === this.activeId)) this.activeId = remaining.at(-1)?.id ?? null;
  }

  cycle(step: 1 | -1) {
    if (this.list.length === 0) return;
    const index = this.list.findIndex((t) => t.id === this.activeId);
    const next = (index + step + this.list.length) % this.list.length;
    this.activeId = this.list[next].id;
  }
}

export const tabs = new Tabs();
