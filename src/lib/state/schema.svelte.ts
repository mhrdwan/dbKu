// Lazily loaded schema tree, cached per connection.

import { untrack } from "svelte";
import { api, errorMessage } from "../api";
import type { ColumnInfo, DbObject, TableRef } from "../types";

const SEP = "\u0000";
export const key = (...parts: (string | null | undefined)[]) => parts.map((p) => p ?? "").join(SEP);

class SchemaCache {
  databases = $state<Record<string, string[]>>({});
  schemas = $state<Record<string, string[]>>({});
  objects = $state<Record<string, DbObject[]>>({});
  columns = $state<Record<string, ColumnInfo[]>>({});
  loading = $state<Record<string, boolean>>({});
  errors = $state<Record<string, string>>({});

  /** Requests in flight, shared so concurrent callers get the same answer. */
  private inflight = new Map<string, Promise<unknown>>();
  /** Bumped by forget(); answers to older requests are dropped. */
  private epoch = new Map<string, number>();

  // Loaders are called from effects; running untracked keeps those effects
  // from re-running on our own bookkeeping.
  private track<T>(k: string, connId: string, load: () => Promise<T>, store: (v: T) => void): Promise<T | undefined> {
    return untrack(() => {
      const pending = this.inflight.get(k) as Promise<T | undefined> | undefined;
      if (pending) return pending;
      const started = this.run(k, connId, load, store).finally(() => this.inflight.delete(k));
      this.inflight.set(k, started);
      return started;
    });
  }

  private async run<T>(k: string, connId: string, load: () => Promise<T>, store: (v: T) => void): Promise<T | undefined> {
    const epoch = this.epoch.get(connId) ?? 0;
    this.loading = { ...this.loading, [k]: true };
    try {
      const value = await load();
      if ((this.epoch.get(connId) ?? 0) !== epoch) return undefined;
      store(value);
      const { [k]: _e, ...errors } = this.errors;
      this.errors = errors;
      return value;
    } catch (e) {
      this.errors = { ...this.errors, [k]: errorMessage(e) };
      return undefined;
    } finally {
      const { [k]: _l, ...loading } = this.loading;
      this.loading = loading;
    }
  }

  loadDatabases(id: string) {
    const k = key("db", id);
    return this.track(k, id, () => api.listDatabases(id), (v) => (this.databases = { ...this.databases, [id]: v }));
  }

  loadSchemas(id: string, db: string) {
    const k = key(id, db);
    return this.track(key("sc", k), id, () => api.listSchemas(id, db), (v) => (this.schemas = { ...this.schemas, [k]: v }));
  }

  loadObjects(id: string, db: string, schema: string | null) {
    const k = key(id, db, schema);
    return this.track(
      key("ob", k),
      id,
      () => api.listObjects(id, db, schema),
      (v) => (this.objects = { ...this.objects, [k]: v }),
    );
  }

  /** Columns of a table. Unlike the tree loaders this rejects on error. */
  async loadColumns(id: string, t: TableRef): Promise<ColumnInfo[]> {
    const k = key(id, t.database, t.schema, t.table);
    const cached = untrack(() => this.columns[k]);
    if (cached) return cached;
    const loaded = await this.track(
      key("co", k),
      id,
      () => api.tableColumns(id, t),
      (v) => (this.columns = { ...this.columns, [k]: v }),
    );
    if (loaded) return loaded;
    const error = untrack(() => this.errors[key("co", k)]);
    if (error) throw new Error(error);
    return untrack(() => this.columns[k]) ?? [];
  }

  cachedColumns(id: string, t: TableRef): ColumnInfo[] | undefined {
    return this.columns[key(id, t.database, t.schema, t.table)];
  }

  objectsOf(id: string, db: string, schema: string | null): DbObject[] | undefined {
    return this.objects[key(id, db, schema)];
  }

  schemasOf(id: string, db: string): string[] | undefined {
    return this.schemas[key(id, db)];
  }

  isLoading(...parts: string[]): boolean {
    return !!this.loading[key(...parts)];
  }

  errorOf(...parts: string[]): string | undefined {
    return this.errors[key(...parts)];
  }

  /** Answers to requests already in flight for this connection are stale. */
  private invalidate(id: string) {
    this.epoch.set(id, (this.epoch.get(id) ?? 0) + 1);
    for (const k of [...this.inflight.keys()]) {
      if (k.split(SEP)[1] === id) this.inflight.delete(k);
    }
  }

  /** Drop everything cached for a connection. */
  forget(id: string) {
    this.invalidate(id);
    const keep = <T>(rec: Record<string, T>) =>
      Object.fromEntries(Object.entries(rec).filter(([k]) => k !== id && !k.startsWith(id + SEP)));
    this.databases = keep(this.databases);
    this.schemas = keep(this.schemas);
    this.objects = keep(this.objects);
    this.columns = keep(this.columns);
  }

  /** Drop cached objects/columns under one database (after DDL, refresh). */
  forgetDatabase(id: string, db: string) {
    this.invalidate(id);
    const prefix = key(id, db);
    const keep = <T>(rec: Record<string, T>) =>
      Object.fromEntries(Object.entries(rec).filter(([k]) => k !== prefix && !k.startsWith(prefix + SEP)));
    this.schemas = keep(this.schemas);
    this.objects = keep(this.objects);
    this.columns = keep(this.columns);
  }

  /** Every cached table/view, for the command palette. */
  allObjects(): { id: string; database: string; schema: string | null; object: DbObject }[] {
    return Object.entries(this.objects).flatMap(([k, objs]) => {
      const [id, database, schema] = k.split(SEP);
      return objs.map((object) => ({ id, database, schema: schema || null, object }));
    });
  }
}

export const schema = new SchemaCache();

/** The schema a fresh session lands in. */
export function defaultSchema(driver: string): string | null {
  if (driver === "postgres") return "public";
  if (driver === "mssql") return "dbo";
  return null;
}
