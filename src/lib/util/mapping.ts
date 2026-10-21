// Matching CSV headers to table columns.

const normalize = (s: string) => s.toLowerCase().replace(/[^a-z0-9]/g, "");

/**
 * For each table column, the index of the CSV field that fills it, or -1.
 * With a header row, fields match by name (case, spaces and punctuation
 * ignored). Without one, fields map to columns in order.
 */
export function autoMap(columns: readonly string[], headers: readonly string[], hasHeader: boolean): number[] {
  if (!hasHeader) return columns.map((_, i) => (i < headers.length ? i : -1));
  const byName = new Map<string, number>();
  headers.forEach((h, i) => {
    const key = normalize(h);
    if (key && !byName.has(key)) byName.set(key, i);
  });
  return columns.map((c) => byName.get(normalize(c)) ?? -1);
}
