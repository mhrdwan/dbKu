// Cross-component workflows.

import { connections } from "./state/connections.svelte";
import { defaultSchema, schema } from "./state/schema.svelte";
import { ask } from "@tauri-apps/plugin-dialog";
import { tabs } from "./state/tabs.svelte";
import { dialogs, toasts } from "./state/ui.svelte";
import type { TableRef } from "./types";

/** Connect (asking for a password when none is stored) and warm the tree. */
export async function openConnection(id: string, password?: string): Promise<boolean> {
  const profile = connections.byId(id);
  if (!profile) return false;
  if (connections.isConnected(id)) return true;
  if (password === undefined && connections.needsPassword(profile)) {
    dialogs.passwordFor = id;
    return false;
  }
  const error = await connections.connect(id, password);
  if (error) {
    toasts.show(`Couldn't connect to ${profile.name}. ${error}`, "error");
    return false;
  }
  await schema.loadDatabases(id);
  const db = defaultDatabase(id);
  if (db) {
    await schema.loadSchemas(id, db);
    const sc = defaultSchema(profile.driver);
    void schema.loadObjects(id, db, (schema.schemasOf(id, db) ?? []).includes(sc ?? "") ? sc : null);
  }
  return true;
}

export function defaultDatabase(id: string): string | null {
  const info = connections.info[id];
  const dbs = schema.databases[id] ?? [];
  return info?.defaultDatabase ?? dbs[0] ?? null;
}

async function confirmLoseEdits(tabIds: string[]): Promise<boolean> {
  const dirty = tabIds.filter((t) => tabs.dirty.has(t));
  if (dirty.length === 0) return true;
  const names = tabs.list.filter((t) => dirty.includes(t.id)).map((t) => t.title).join(", ");
  return ask(`Unsaved changes in ${names} will be lost.`, { title: "Discard unsaved changes?", kind: "warning" });
}

/** Close a tab, asking first when it holds unsaved grid edits. */
export async function requestCloseTab(id: string) {
  if (await confirmLoseEdits([id])) tabs.close(id);
}

export async function requestCloseOtherTabs(id: string) {
  const others = tabs.list.filter((t) => t.id !== id).map((t) => t.id);
  if (await confirmLoseEdits(others)) tabs.closeOthers(id);
}

/** Returns false when the user chose to keep unsaved edits. */
export async function closeConnection(id: string): Promise<boolean> {
  const affected = tabs.list.filter((t) => t.kind === "table" && t.connId === id).map((t) => t.id);
  if (!(await confirmLoseEdits(affected))) return false;
  await connections.disconnect(id);
  tabs.closeForConnection(id);
  return true;
}

export function newQueryFor(id: string | null, database?: string | null, sc?: string | null, sql = "") {
  if (!id) {
    tabs.newQuery(null, null, null, sql);
    return;
  }
  const profile = connections.byId(id);
  const db = database ?? defaultDatabase(id);
  const chosenSchema = sc !== undefined ? sc : profile ? defaultSchema(profile.driver) : null;
  tabs.newQuery(id, db, chosenSchema, sql);
}

export function qualifiedName(driver: string | undefined, t: TableRef): string {
  const q = (s: string) => {
    if (driver === "mysql") return "`" + s.replace(/`/g, "``") + "`";
    if (driver === "mssql") return "[" + s.replace(/]/g, "]]") + "]";
    return /^[a-z_][a-z0-9_]*$/.test(s) ? s : '"' + s.replace(/"/g, '""') + '"';
  };
  const parts = driver === "mysql" || driver === "sqlite" ? [t.table] : [t.schema, t.table];
  return parts.filter((p): p is string => !!p).map(q).join(".");
}

export function selectStatement(driver: string | undefined, t: TableRef, limit = 100): string {
  const name = qualifiedName(driver, t);
  return driver === "mssql" ? `SELECT TOP ${limit} *\nFROM ${name};` : `SELECT *\nFROM ${name}\nLIMIT ${limit};`;
}

export async function copyText(text: string, what = "Copied") {
  try {
    await navigator.clipboard.writeText(text);
    toasts.show(what, "success", 1600);
  } catch {
    toasts.show("Clipboard is not available", "error");
  }
}
