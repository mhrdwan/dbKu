// Mirrors src-tauri/src/model.rs (camelCase over IPC).

export type Driver = "postgres" | "mysql" | "sqlite" | "mssql";
export type SslMode = "disable" | "prefer" | "require";

export interface ConnectionProfile {
  id: string;
  name: string;
  driver: Driver;
  host: string;
  port: number;
  user: string;
  database: string;
  sslMode: SslMode;
  trustServerCert: boolean;
  legacyTls: boolean;
  readOnly: boolean;
  color: string | null;
  group: string | null;
  savePassword: boolean;
}

export interface ConnectionInput extends ConnectionProfile {
  password?: string | null;
}

export interface ServerInfo {
  version: string;
  defaultDatabase: string | null;
}

export interface QueryContext {
  database?: string | null;
  schema?: string | null;
}

export interface TableRef {
  database?: string | null;
  schema?: string | null;
  table: string;
}

export type ObjectKind = "table" | "view" | "materialized_view" | "foreign_table";

export interface DbObject {
  name: string;
  kind: ObjectKind;
}

export interface ColumnInfo {
  name: string;
  dataType: string;
  nullable: boolean;
  defaultValue: string | null;
  isPrimaryKey: boolean;
  isAutoIncrement: boolean;
  comment: string | null;
}

export interface IndexInfo {
  name: string;
  columns: string[];
  isUnique: boolean;
  isPrimary: boolean;
}

export interface ColumnMeta {
  name: string;
  typeName: string;
}

export type Cell = string | null;

export interface ResultSet {
  statement: string;
  columns: ColumnMeta[];
  rows: Cell[][];
  rowsAffected: number | null;
  truncated: boolean;
  elapsedMs: number;
}

export interface StatementError {
  index: number;
  statement: string;
  message: string;
}

export interface ExecOutcome {
  results: ResultSet[];
  notices: string[];
  error: StatementError | null;
  elapsedMs: number;
}

export interface ExecOptions {
  maxRows?: number;
  runId?: string;
  /** Query-tab key: runs with the same key share one dedicated connection. */
  session?: string;
}

export type SortDir = "asc" | "desc";

export interface SortSpec {
  column: string;
  dir: SortDir;
}

export interface TableQuery extends TableRef {
  limit: number;
  offset: number;
  sort: SortSpec[];
  filter?: string | null;
  search?: string | null;
  searchColumns?: string[];
}

export type KeyValues = [string, Cell][];

export type RowChange =
  | { type: "update"; key: KeyValues; values: KeyValues }
  | { type: "insert"; values: KeyValues }
  | { type: "delete"; key: KeyValues };

export interface SearchHit {
  schema: string | null;
  table: string;
  column: string | null;
  kind: string;
}

export interface AppError {
  kind: "database" | "not_connected" | "invalid" | "read_only" | "storage" | "secret";
  message: string;
}

export interface SavedQuery {
  id: string;
  connId: string | null;
  name: string;
  sql: string;
  database: string | null;
  schema: string | null;
  updatedAt: number;
}

export interface PickedFile {
  token: string;
  name: string;
  size: number | null;
}

export interface CsvPreview {
  headers: string[];
  rows: string[][];
  delimiter: string;
  size: number;
}

export interface ColumnMapping {
  column: string;
  source: number;
}

export interface CsvImportOptions {
  delimiter: string;
  hasHeader: boolean;
  emptyAsNull: boolean;
  truncateFirst: boolean;
  mapping: ColumnMapping[];
}

export interface DumpOptions {
  structure: boolean;
  data: boolean;
  dropExisting: boolean;
  rowsPerInsert: number;
}

export interface DumpSummary {
  path: string;
  objects: number;
  rows: number;
}

export interface ScriptOutcome {
  total: number;
  executed: number;
  rowsAffected: number;
  error: StatementError | null;
  cancelled: boolean;
}

export interface TransferProgress {
  taskId: string;
  done: number;
  total: number | null;
  label: string;
}

