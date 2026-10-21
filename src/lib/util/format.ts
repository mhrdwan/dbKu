import type { Cell } from "../types";

const NUMERIC = /^(u?int\d*|integer|bigint|smallint|tinyint|mediumint|numeric|decimal|float\d*|double|real|money|smallmoney|serial|bigserial|smallserial|number|oid|bit|i16|i32|i64|u8|f32|f64|int[248]|float[48])\b/i;

export function isNumericType(typeName: string): boolean {
  return NUMERIC.test(typeName.trim());
}

const MAX_DISPLAY = 400;

/** One-line preview of a value for a grid cell. */
export function displayValue(v: Cell): string {
  if (v === null) return "NULL";
  const short = v.length > MAX_DISPLAY ? v.slice(0, MAX_DISPLAY) + "…" : v;
  return short.replace(/\r?\n/g, " ⏎ ").replace(/\t/g, " ");
}

export function formatMs(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(ms < 10_000 ? 2 : 1)} s`;
  const m = Math.floor(ms / 60_000);
  return `${m} min ${Math.round((ms % 60_000) / 1000)} s`;
}

export function plural(n: number, one: string, many = one + "s"): string {
  return `${n.toLocaleString()} ${n === 1 ? one : many}`;
}

/** Rough pixel width for a column, from its header and a sample of values. */
export function estimateWidth(name: string, values: Cell[]): number {
  const sample = values.slice(0, 200);
  const longest = sample.reduce((m, v) => Math.max(m, Math.min(displayValue(v).length, 48)), 0);
  const px = Math.max(name.length * 7.4 + 34, longest * 7.1 + 22);
  return Math.round(Math.min(360, Math.max(72, px)));
}

export function tryPrettyJson(v: string): string | null {
  const t = v.trim();
  if (!(t.startsWith("{") || t.startsWith("["))) return null;
  try {
    return JSON.stringify(JSON.parse(t), null, 2);
  } catch {
    return null;
  }
}
