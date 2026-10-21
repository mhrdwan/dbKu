// Client-side quoting, used only to pre-fill editable text (filters, snippets)
// that the user sees before it runs. The backend does its own quoting.

import type { Cell, Driver } from "../types";

export function quoteIdent(driver: Driver | undefined, name: string): string {
  if (driver === "mysql") return "`" + name.replace(/`/g, "``") + "`";
  if (driver === "mssql") return "[" + name.replace(/]/g, "]]") + "]";
  return '"' + name.replace(/"/g, '""') + '"';
}

export function quoteLiteral(driver: Driver | undefined, value: string): string {
  if (driver === "mysql") return "'" + value.replace(/\\/g, "\\\\").replace(/'/g, "''") + "'";
  if (driver === "mssql") return "N'" + value.replace(/'/g, "''") + "'";
  return "'" + value.replace(/'/g, "''") + "'";
}

export function equalsPredicate(driver: Driver | undefined, column: string, value: Cell): string {
  const col = quoteIdent(driver, column);
  return value === null ? `${col} IS NULL` : `${col} = ${quoteLiteral(driver, value)}`;
}
