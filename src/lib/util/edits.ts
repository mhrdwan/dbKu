// Pending grid edits, kept as immutable values: every function returns a
// new PendingEdits and never touches its input.

import type { Cell, KeyValues, RowChange } from "../types";

export interface PendingInsert {
  id: string;
  values: Readonly<Record<string, Cell>>;
}

export interface PendingEdits {
  /** Edited cells of existing rows, by row index then column name. */
  readonly updates: Readonly<Record<number, Readonly<Record<string, Cell>>>>;
  readonly inserts: readonly PendingInsert[];
  readonly deletes: readonly number[];
}

export const emptyEdits = (): PendingEdits => ({ updates: {}, inserts: [], deletes: [] });

export function setCell(
  edits: PendingEdits,
  row: number,
  column: string,
  value: Cell,
  original: Cell,
): PendingEdits {
  const { [column]: _dropped, ...rest } = edits.updates[row] ?? {};
  const rowEdits = value === original ? rest : { ...rest, [column]: value };
  const { [row]: _old, ...otherRows } = edits.updates;
  const updates = Object.keys(rowEdits).length > 0 ? { ...otherRows, [row]: rowEdits } : otherRows;
  return { ...edits, updates };
}

export function toggleDelete(edits: PendingEdits, row: number): PendingEdits {
  const deletes = edits.deletes.includes(row)
    ? edits.deletes.filter((r) => r !== row)
    : [...edits.deletes, row];
  return { ...edits, deletes };
}

export function addInsert(edits: PendingEdits, id: string): PendingEdits {
  return { ...edits, inserts: [...edits.inserts, { id, values: {} }] };
}

export function setInsertCell(edits: PendingEdits, id: string, column: string, value: Cell): PendingEdits {
  const inserts = edits.inserts.map((ins) =>
    ins.id === id ? { ...ins, values: { ...ins.values, [column]: value } } : ins,
  );
  return { ...edits, inserts };
}

export function removeInsert(edits: PendingEdits, id: string): PendingEdits {
  return { ...edits, inserts: edits.inserts.filter((ins) => ins.id !== id) };
}

export function editCount(edits: PendingEdits): number {
  const updated = Object.keys(edits.updates).filter((r) => !edits.deletes.includes(Number(r))).length;
  return updated + edits.inserts.length + edits.deletes.length;
}

export function cellValue(edits: PendingEdits, row: number, column: string, original: Cell): Cell {
  const rowEdits = edits.updates[row];
  return rowEdits && column in rowEdits ? rowEdits[column] : original;
}

export function isCellEdited(edits: PendingEdits, row: number, column: string): boolean {
  const rowEdits = edits.updates[row];
  return !!rowEdits && column in rowEdits;
}

/**
 * Turn pending edits into backend changes. Rows are identified by the
 * ORIGINAL values of their key columns, so editing a key column works.
 */
export function toRowChanges(
  edits: PendingEdits,
  rows: readonly (readonly Cell[])[],
  columns: readonly string[],
  keyColumns: readonly string[],
): RowChange[] {
  const keyOf = (row: number): KeyValues =>
    keyColumns.map((k) => [k, rows[row]?.[columns.indexOf(k)] ?? null]);

  const deletes: RowChange[] = [...edits.deletes]
    .sort((a, b) => a - b)
    .map((row) => ({ type: "delete", key: keyOf(row) }));

  const updates: RowChange[] = Object.entries(edits.updates)
    .map(([row, values]) => [Number(row), values] as const)
    .filter(([row]) => !edits.deletes.includes(row))
    .sort(([a], [b]) => a - b)
    .map(([row, values]) => ({ type: "update", key: keyOf(row), values: Object.entries(values) }));

  const inserts: RowChange[] = edits.inserts.map((ins) => ({
    type: "insert",
    values: Object.entries(ins.values),
  }));

  return [...updates, ...deletes, ...inserts];
}
