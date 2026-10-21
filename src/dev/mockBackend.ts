// Browser-only preview backend. `pnpm dev` opened in a normal browser has no
// Rust side, so this answers the IPC calls with demo data. It is imported
// only in development when not running inside Tauri (see main.ts).

import { mockIPC } from "@tauri-apps/api/mocks";
import type { Cell, ColumnInfo, ConnectionProfile, ExecOutcome, ResultSet, TableQuery } from "../lib/types";

const profiles: ConnectionProfile[] = [
  { id: "demo-pg", name: "Hotel PMS (production)", driver: "postgres", host: "10.0.4.12", port: 5432, user: "report", database: "pms", sslMode: "prefer", trustServerCert: false, legacyTls: false, readOnly: true, color: "#d64545", group: null, savePassword: true },
  { id: "demo-my", name: "Shop staging", driver: "mysql", host: "localhost", port: 3306, user: "root", database: "shop", sslMode: "prefer", trustServerCert: false, legacyTls: false, readOnly: false, color: "#2f9e5b", group: null, savePassword: true },
  { id: "demo-lt", name: "notes.db", driver: "sqlite", host: "", port: 0, user: "", database: "/Users/me/notes.db", sslMode: "disable", trustServerCert: false, legacyTls: false, readOnly: false, color: null, group: "Local", savePassword: true },
];

const names = ["Ayu Lestari", "Budi Santoso", "Citra Dewi", "Dimas Pratama", "Eka Putri", "Fajar Nugroho", "Gita Maharani", "Hendra Wijaya", "Intan Permata", "Joko Susilo"];
const cities = ["Jakarta", "Bandung", "Surabaya", "Yogyakarta", "Denpasar", "Medan"];

const guestCols: ColumnInfo[] = [
  { name: "id", dataType: "integer", nullable: false, defaultValue: "nextval('guests_id_seq'::regclass)", isPrimaryKey: true, isAutoIncrement: true, comment: null },
  { name: "full_name", dataType: "character varying(120)", nullable: false, defaultValue: null, isPrimaryKey: false, isAutoIncrement: false, comment: "As written on the ID card" },
  { name: "email", dataType: "character varying(200)", nullable: true, defaultValue: null, isPrimaryKey: false, isAutoIncrement: false, comment: null },
  { name: "city", dataType: "text", nullable: true, defaultValue: null, isPrimaryKey: false, isAutoIncrement: false, comment: null },
  { name: "nights", dataType: "integer", nullable: false, defaultValue: "0", isPrimaryKey: false, isAutoIncrement: false, comment: null },
  { name: "balance", dataType: "numeric(12,2)", nullable: false, defaultValue: "0", isPrimaryKey: false, isAutoIncrement: false, comment: null },
  { name: "notes", dataType: "jsonb", nullable: true, defaultValue: null, isPrimaryKey: false, isAutoIncrement: false, comment: null },
  { name: "created_at", dataType: "timestamp with time zone", nullable: false, defaultValue: "now()", isPrimaryKey: false, isAutoIncrement: false, comment: null },
];

const guests: Cell[][] = Array.from({ length: 1240 }, (_, i) => {
  const n = names[i % names.length];
  return [
    String(i + 1),
    `${n}${i >= names.length ? ` ${Math.floor(i / names.length)}` : ""}`,
    i % 7 === 3 ? null : `${n.split(" ")[0].toLowerCase()}${i}@mail.id`,
    cities[i % cities.length],
    String((i * 7) % 15),
    ((i * 137.5) % 9000).toFixed(2),
    i % 5 === 0 ? `{"vip": ${i % 10 === 0}, "pillow": "soft"}` : null,
    `2026-${String((i % 9) + 1).padStart(2, "0")}-${String((i % 27) + 1).padStart(2, "0")} 14:${String(i % 60).padStart(2, "0")}:00+07`,
  ];
});

const objects = {
  "demo-pg": [
    { name: "bookings", kind: "table" },
    { name: "guests", kind: "table" },
    { name: "invoices", kind: "table" },
    { name: "rooms", kind: "table" },
    { name: "daily_occupancy", kind: "view" },
  ],
  "demo-my": [
    { name: "customers", kind: "table" },
    { name: "orders", kind: "table" },
    { name: "products", kind: "table" },
  ],
  "demo-lt": [{ name: "notes", kind: "table" }],
} as const;

function set(columns: string[], rows: Cell[][], statement = ""): ResultSet {
  return { statement, columns: columns.map((name) => ({ name, typeName: "text" })), rows, rowsAffected: null, truncated: false, elapsedMs: 4 };
}

function guestsPage(q: TableQuery): ResultSet {
  let rows = guests;
  if (q.search) {
    const s = q.search.toLowerCase();
    rows = rows.filter((r) => r.some((v) => v?.toLowerCase().includes(s)));
  }
  const sort = q.sort[0];
  if (sort) {
    const i = guestCols.findIndex((c) => c.name === sort.column);
    rows = [...rows].sort((a, b) => (a[i] ?? "").localeCompare(b[i] ?? "", undefined, { numeric: true }) * (sort.dir === "asc" ? 1 : -1));
  }
  const page = rows.slice(q.offset, q.offset + q.limit);
  return { ...set(guestCols.map((c) => c.name), page), columns: guestCols.map((c) => ({ name: c.name, typeName: c.dataType })) };
}

/** Just enough SQL for screenshots: column list, `col >= n` and ORDER BY on guests. */
function demoQuery(sql: string): ResultSet {
  const all = guestsPage({ table: "guests", limit: 100_000, offset: 0, sort: [] });
  const names = guestCols.map((c) => c.name);
  const strip = (s: string) => s.trim().replace(/^\w+\./, "");
  const list = /select\s+([\s\S]+?)\s+from/i.exec(sql)?.[1] ?? "*";
  const picked = list.trim() === "*" ? names : list.split(",").map(strip).filter((c) => names.includes(c));
  let rows = all.rows;
  const min = /(\w+\.)?(\w+)\s*>=\s*(\d+)/i.exec(sql);
  if (min && names.includes(min[2])) {
    const i = names.indexOf(min[2]);
    rows = rows.filter((r) => Number(r[i]) >= Number(min[3]));
  }
  const order = /order\s+by\s+(\w+\.)?(\w+)(\s+desc)?/i.exec(sql);
  if (order && names.includes(order[2])) {
    const i = names.indexOf(order[2]);
    const dir = order[3] ? -1 : 1;
    rows = [...rows].sort((a, b) => (Number(a[i]) - Number(b[i])) * dir);
  }
  const idx = picked.map((c) => names.indexOf(c));
  return {
    ...all,
    columns: idx.map((i) => all.columns[i]),
    rows: rows.slice(0, 200).map((r) => idx.map((i) => r[i])),
  };
}

const delay = <T>(v: T, ms = 120) => new Promise<T>((r) => setTimeout(() => r(v), ms));

let saved: Record<string, unknown>[] = [
  { id: "q1", connId: "demo-pg", name: "Arrivals today", sql: "SELECT * FROM bookings WHERE arrival = CURRENT_DATE", database: "pms", schema: "public", updatedAt: 0 },
  { id: "q2", connId: "demo-pg", name: "Unpaid invoices", sql: "SELECT * FROM invoices WHERE paid = false", database: "pms", schema: "public", updatedAt: 0 },
];

export function installMockBackend() {
  mockIPC((cmd, payload) => {
    const args = (payload ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case "list_connections":
        return profiles;
      case "connect":
      case "test_connection":
        return delay({ version: "PostgreSQL 16.4 on aarch64-apple-darwin", defaultDatabase: args.id === "demo-my" ? "shop" : args.id === "demo-lt" ? "main" : "pms" }, 300);
      case "disconnect":
        return null;
      case "list_databases":
        return args.id === "demo-pg" ? ["pms", "pms_archive", "postgres"] : args.id === "demo-my" ? ["shop", "analytics"] : ["main"];
      case "list_schemas":
        return args.id === "demo-pg" ? ["public", "audit", "reporting"] : [];
      case "list_objects":
        return delay(objects[args.id as keyof typeof objects] ?? []);
      case "table_columns":
        return guestCols;
      case "table_indexes":
        return [
          { name: "guests_pkey", columns: ["id"], isUnique: true, isPrimary: true },
          { name: "guests_email_key", columns: ["email"], isUnique: true, isPrimary: false },
          { name: "guests_city_idx", columns: ["city", "created_at"], isUnique: false, isPrimary: false },
        ];
      case "table_ddl":
        return 'CREATE TABLE "public"."guests" (\n  "id" integer NOT NULL DEFAULT nextval(\'guests_id_seq\'::regclass),\n  "full_name" character varying(120) NOT NULL,\n  "email" character varying(200),\n  PRIMARY KEY ("id")\n);';
      case "fetch_table":
        return delay(guestsPage(args.query as TableQuery));
      case "count_rows": {
        const q = args.query as TableQuery;
        return delay(guestsPage({ ...q, offset: 0, limit: 1e9 }).rows.length, 400);
      }
      case "search_objects":
        return [
          { schema: "public", table: "guests", column: null, kind: "table" },
          { schema: "public", table: "guests", column: "guest_email", kind: "column" },
          { schema: "public", table: "bookings", column: "guest_id", kind: "column" },
        ];
      case "execute_query": {
        const sql = String(args.sql);
        if (/delete|update|insert|drop/i.test(sql)) {
          return Promise.reject({ kind: "read_only", message: "Blocked by read-only mode: `DELETE` is not allowed" });
        }
        const outcome: ExecOutcome = {
          results: [{ ...demoQuery(sql), statement: sql, elapsedMs: 38 }],
          notices: [],
          error: null,
          elapsedMs: 41,
        };
        return delay(outcome, 250);
      }
      case "cancel_query":
        return true;
      case "close_session":
        return null;
      case "preview_changes": {
        type Change = { type: string; key?: [string, Cell][]; values?: [string, Cell][] };
        const t = args.table as { database?: string; table: string };
        const name = `\`${t.database ?? "shop"}\`.\`${t.table}\``;
        const lit = (v: Cell) => (v === null ? "NULL" : `'${v.replace(/'/g, "''")}'`);
        const where = (key: [string, Cell][] = []) => key.map(([k, v]) => `\`${k}\` = ${lit(v)}`).join(" AND ");
        return (args.changes as Change[]).map((c) =>
          c.type === "update"
            ? `UPDATE ${name} SET ${(c.values ?? []).map(([k, v]) => `\`${k}\` = ${lit(v)}`).join(", ")} WHERE ${where(c.key)}`
            : c.type === "delete"
              ? `DELETE FROM ${name} WHERE ${where(c.key)}`
              : `INSERT INTO ${name} (${(c.values ?? []).map(([k]) => `\`${k}\``).join(", ")}) VALUES (${(c.values ?? []).map(([, v]) => lit(v)).join(", ")})`,
        );
      }
      case "apply_changes":
        return (args.changes as unknown[]).length;
      case "save_connection":
        return (args.input as ConnectionProfile & { id: string }).id ? args.input : { ...(args.input as object), id: crypto.randomUUID() };
      case "delete_connection":
        return null;
      case "pick_file":
        return delay({ token: "demo-token", name: args.purpose === "csv" ? "guests.csv" : "seed.sql", size: 48_213 });
      case "preview_csv":
        return {
          headers: ["Full Name", "E-mail", "City", "nights"],
          rows: [["Rina Kusuma", "rina@mail.id", "Malang", "2"], ["Agus Salim", "", "Bogor", "1"]],
          delimiter: ",",
          size: 48_213,
        };
      case "import_csv":
        return delay(1_240, 900);
      case "export_dump":
        return delay({ path: "/Users/me/Desktop/pms.sql", objects: 5, rows: 1_240 }, 900);
      case "run_sql_file":
        return delay({ total: 12, executed: 12, rowsAffected: 30, error: null, cancelled: false }, 900);
      case "cancel_task":
        return true;
      case "list_saved_queries":
        return saved;
      case "save_query": {
        const q = { ...(args.query as Record<string, unknown>) };
        if (!q.id) q.id = crypto.randomUUID();
        saved = [...saved.filter((x) => x.id !== q.id), q];
        return q;
      }
      case "delete_saved_query":
        saved = saved.filter((x) => x.id !== args.id);
        return null;
      default:
        if (cmd.startsWith("plugin:event")) return 1;
        if (cmd.startsWith("plugin:dialog")) return true;
        throw new Error(`mock backend: unhandled command ${cmd}`);
    }
  });
}
