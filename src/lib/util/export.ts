// Result exports. NULL is written as an empty CSV field / JSON null.

import type { Cell } from "../types";

function csvField(value: Cell, delimiter: string): string {
  if (value === null) return "";
  const needsQuotes = value.includes(delimiter) || /["\r\n]/.test(value) || /^\s|\s$/.test(value);
  return needsQuotes ? `"${value.replace(/"/g, '""')}"` : value;
}

export function toDelimited(columns: readonly string[], rows: readonly (readonly Cell[])[], delimiter = ","): string {
  const lines = [columns, ...rows].map((r) => r.map((v) => csvField(v, delimiter)).join(delimiter));
  return lines.join("\r\n") + "\r\n";
}

/** Tab-separated without a trailing newline: pastes cleanly into spreadsheets. */
export function toClipboardTsv(rows: readonly (readonly Cell[])[]): string {
  return rows.map((r) => r.map((v) => (v ?? "").replace(/[\t\r\n]+/g, " ")).join("\t")).join("\n");
}

export function toJson(columns: readonly string[], rows: readonly (readonly Cell[])[]): string {
  const objects = rows.map((r) => Object.fromEntries(columns.map((c, i) => [c, r[i] ?? null])));
  return JSON.stringify(objects, null, 2) + "\n";
}

export function toMarkdown(columns: readonly string[], rows: readonly (readonly Cell[])[]): string {
  const esc = (v: Cell) => (v === null ? "NULL" : v.replace(/\|/g, "\\|").replace(/\r?\n/g, " "));
  const head = `| ${columns.map((c) => esc(c)).join(" | ")} |`;
  const rule = `| ${columns.map(() => "---").join(" | ")} |`;
  const body = rows.map((r) => `| ${r.map(esc).join(" | ")} |`);
  return [head, rule, ...body].join("\n") + "\n";
}
