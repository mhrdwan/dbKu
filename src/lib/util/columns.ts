export interface ColumnMatch {
  index: number;
  /** Where the query sits in the name, or -1 when it only matched with spacing removed. */
  start: number;
  length: number;
}

const squash = (s: string) => s.toLowerCase().replace(/[\s_-]+/g, "");

/**
 * Columns whose name contains `query`, ignoring case (and, as a fallback, spaces,
 * underscores and dashes). Exact names come first, then prefixes, then the rest,
 * each in table order. An empty query lists every column.
 */
export function findColumns(names: readonly string[], query: string): ColumnMatch[] {
  const q = query.trim().toLowerCase();
  if (!q) return names.map((_, index) => ({ index, start: -1, length: 0 }));
  const sq = squash(q);
  const ranked = names.flatMap((name, index) => {
    const lower = name.toLowerCase();
    const at = lower.indexOf(q);
    if (at >= 0) return [{ rank: lower === q ? 0 : at === 0 ? 1 : 2, match: { index, start: at, length: q.length } }];
    const squashed = squash(name);
    if (sq && squashed.includes(sq)) return [{ rank: squashed === sq ? 0 : squashed.startsWith(sq) ? 1 : 2, match: { index, start: -1, length: 0 } }];
    return [];
  });
  return ranked.sort((a, b) => a.rank - b.rank || a.match.index - b.match.index).map((r) => r.match);
}
