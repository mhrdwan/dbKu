// Typed wrappers around the Rust commands in src-tauri/src/commands.rs.

import { invoke } from "@tauri-apps/api/core";
import type {
  AppError,
  ColumnInfo,
  CsvImportOptions,
  CsvPreview,
  DumpOptions,
  DumpSummary,
  PickedFile,
  SavedQuery,
  ScriptOutcome,
  ConnectionInput,
  ConnectionProfile,
  DbObject,
  ExecOptions,
  ExecOutcome,
  IndexInfo,
  QueryContext,
  ResultSet,
  RowChange,
  SearchHit,
  ServerInfo,
  TableQuery,
  TableRef,
} from "./types";

export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as AppError).message);
  return String(e);
}

export function isAppError(e: unknown, kind: AppError["kind"]): boolean {
  return !!e && typeof e === "object" && (e as AppError).kind === kind;
}

function toQuery(q: TableQuery) {
  return { ...q, searchColumns: q.searchColumns ?? [] };
}

export const api = {
  listConnections: () => invoke<ConnectionProfile[]>("list_connections"),
  saveConnection: (input: ConnectionInput) => invoke<ConnectionProfile>("save_connection", { input }),
  deleteConnection: (id: string) => invoke<void>("delete_connection", { id }),
  testConnection: (input: ConnectionInput) => invoke<ServerInfo>("test_connection", { input }),
  connect: (id: string, password?: string) => invoke<ServerInfo>("connect", { id, password: password ?? null }),
  disconnect: (id: string) => invoke<void>("disconnect", { id }),

  listDatabases: (id: string) => invoke<string[]>("list_databases", { id }),
  listSchemas: (id: string, database: string) => invoke<string[]>("list_schemas", { id, database }),
  listObjects: (id: string, database: string, schema: string | null) =>
    invoke<DbObject[]>("list_objects", { id, database, schema }),
  tableColumns: (id: string, table: TableRef) => invoke<ColumnInfo[]>("table_columns", { id, table }),
  tableIndexes: (id: string, table: TableRef) => invoke<IndexInfo[]>("table_indexes", { id, table }),
  tableDdl: (id: string, table: TableRef) => invoke<string>("table_ddl", { id, table }),
  searchObjects: (id: string, database: string, term: string) =>
    invoke<SearchHit[]>("search_objects", { id, database, term }),

  executeQuery: (id: string, context: QueryContext, sql: string, options?: ExecOptions) =>
    invoke<ExecOutcome>("execute_query", { id, context, sql, options: options ?? null }),
  cancelQuery: (id: string, runId: string) => invoke<boolean>("cancel_query", { id, runId }),
  closeSession: (id: string, session: string) => invoke<void>("close_session", { id, session }),
  fetchTable: (id: string, query: TableQuery) => invoke<ResultSet>("fetch_table", { id, query: toQuery(query) }),
  countRows: (id: string, query: TableQuery) => invoke<number>("count_rows", { id, query: toQuery(query) }),
  previewChanges: (id: string, table: TableRef, changes: RowChange[]) =>
    invoke<string[]>("preview_changes", { id, table, changes }),
  applyChanges: (id: string, table: TableRef, changes: RowChange[]) =>
    invoke<number>("apply_changes", { id, table, changes }),

  // Files are chosen in native dialogs on the Rust side; only a token comes back.
  pickFile: (purpose: "dump" | "csv" | "sql", suggestedName?: string) =>
    invoke<PickedFile | null>("pick_file", { purpose, suggestedName: suggestedName ?? null }),
  previewCsv: (token: string, delimiter: string | null, hasHeader: boolean) =>
    invoke<CsvPreview>("preview_csv", { token, delimiter, hasHeader }),
  exportDump: (args: {
    id: string;
    token: string;
    taskId: string;
    database: string | null;
    schema: string | null;
    objects: DbObject[];
    options: DumpOptions;
  }) => invoke<DumpSummary>("export_dump", args),
  importCsv: (id: string, token: string, taskId: string, table: TableRef, options: CsvImportOptions) =>
    invoke<number>("import_csv", { id, token, taskId, table, options }),
  runSqlFile: (id: string, token: string, taskId: string, context: QueryContext) =>
    invoke<ScriptOutcome>("run_sql_file", { id, token, taskId, context }),
  cancelTask: (taskId: string) => invoke<boolean>("cancel_task", { taskId }),

  listSavedQueries: () => invoke<SavedQuery[]>("list_saved_queries"),
  saveQuery: (query: Partial<SavedQuery> & { name: string; sql: string }) =>
    invoke<SavedQuery>("save_query", { query: { id: "", connId: null, database: null, schema: null, ...query } }),
  deleteSavedQuery: (id: string) => invoke<void>("delete_saved_query", { id }),
};
