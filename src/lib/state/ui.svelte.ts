// Transient UI services: toasts, context menu, query history, dialogs.

import type { Component } from "svelte";

export type ToastKind = "info" | "success" | "error";

interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
}

class Toasts {
  list = $state<Toast[]>([]);
  private seq = 0;

  show(message: string, kind: ToastKind = "info", ms = kind === "error" ? 7000 : 3200) {
    const id = ++this.seq;
    this.list = [...this.list, { id, kind, message }];
    setTimeout(() => this.dismiss(id), ms);
  }

  dismiss(id: number) {
    this.list = this.list.filter((t) => t.id !== id);
  }
}

export const toasts = new Toasts();

export interface MenuItem {
  label: string;
  icon?: Component;
  shortcut?: string;
  danger?: boolean;
  disabled?: boolean;
  action?: () => void;
  separator?: boolean;
}

class ContextMenu {
  open = $state(false);
  x = $state(0);
  y = $state(0);
  items = $state<MenuItem[]>([]);

  show(e: MouseEvent, items: MenuItem[]) {
    e.preventDefault();
    e.stopPropagation();
    this.items = items;
    this.x = e.clientX;
    this.y = e.clientY;
    this.open = true;
  }

  close() {
    this.open = false;
  }
}

export const menu = new ContextMenu();

export interface HistoryEntry {
  sql: string;
  connId: string;
  database: string | null;
  at: number;
  ok: boolean;
  ms: number;
}

const HISTORY_KEY = "dbku.history";
const HISTORY_MAX = 300;

class QueryHistory {
  entries = $state<HistoryEntry[]>(load());

  add(entry: HistoryEntry) {
    const sql = entry.sql.trim();
    if (!sql) return;
    const deduped = this.entries.filter((e) => !(e.sql === sql && e.connId === entry.connId));
    this.entries = [{ ...entry, sql }, ...deduped].slice(0, HISTORY_MAX);
    try {
      localStorage.setItem(HISTORY_KEY, JSON.stringify(this.entries));
    } catch {
      // History is a convenience; losing it is acceptable.
    }
  }

  forConnection(connId: string | null): HistoryEntry[] {
    return connId ? this.entries.filter((e) => e.connId === connId) : this.entries;
  }

  clear() {
    this.entries = [];
    try {
      localStorage.removeItem(HISTORY_KEY);
    } catch {
      // ignore
    }
  }
}

function load(): HistoryEntry[] {
  try {
    const raw = localStorage.getItem(HISTORY_KEY);
    return raw ? (JSON.parse(raw) as HistoryEntry[]) : [];
  } catch {
    return [];
  }
}

export const history = new QueryHistory();

export type TransferRequest =
  | { kind: "export"; connId: string; database: string | null; schema: string | null; only?: string[] }
  | { kind: "import"; connId: string; table: import("../types").TableRef }
  | { kind: "script"; connId: string; database: string | null; schema: string | null };

/** App-level dialogs any component can open. */
class Dialogs {
  transfer = $state<TransferRequest | null>(null);
  connectionEditor = $state<{ open: boolean; id: string | null; duplicateOf?: string }>({ open: false, id: null });
  passwordFor = $state<string | null>(null);
  palette = $state(false);
  settings = $state(false);

  editConnection(id: string | null) {
    this.connectionEditor = { open: true, id };
  }
  duplicateConnection(id: string) {
    this.connectionEditor = { open: true, id: null, duplicateOf: id };
  }
  closeConnectionEditor() {
    this.connectionEditor = { open: false, id: null };
  }
}

export const dialogs = new Dialogs();

/** Bumped when data changes outside a table tab (e.g. a CSV import), so open
 *  tabs showing that table reload. Keyed by connId/database/schema/table. */
class DataVersions {
  versions = $state<Record<string, number>>({});

  key(connId: string, t: import("../types").TableRef): string {
    return [connId, t.database ?? "", t.schema ?? "", t.table].join("/");
  }

  bump(connId: string, t: import("../types").TableRef) {
    const k = this.key(connId, t);
    this.versions = { ...this.versions, [k]: (this.versions[k] ?? 0) + 1 };
  }

  of(connId: string, t: import("../types").TableRef): number {
    return this.versions[this.key(connId, t)] ?? 0;
  }
}

export const dataVersions = new DataVersions();

