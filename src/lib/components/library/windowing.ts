export interface WindowRow {
  start: number;
  end: number;
  height: number;
  offset: number;
}

export function columnCount(width: number, layout: 'grid' | 'list', gap = 8): number {
  return layout === 'list' ? 1 : Math.max(1, Math.floor((width + gap) / (340 + gap)));
}

export function groupRows(ids: readonly string[], columns: number, heights: ReadonlyMap<string, number>, estimate = 220, gap = 8): WindowRow[] {
  const rows: WindowRow[] = [];
  let offset = 0;
  for (let start = 0; start < ids.length; start += columns) {
    const end = Math.min(start + columns, ids.length);
    let height = 0;
    for (let i = start; i < end; i++) height = Math.max(height, heights.get(ids[i]) ?? estimate);
    rows.push({ start, end, height, offset });
    offset += height + gap;
  }
  return rows;
}

export function rowAt(rows: readonly WindowRow[], offset: number): number {
  let low = 0;
  let high = rows.length;
  while (low < high) {
    const mid = (low + high) >>> 1;
    if (rows[mid].offset + rows[mid].height <= offset) low = mid + 1;
    else high = mid;
  }
  return Math.min(low, Math.max(0, rows.length - 1));
}

export function visibleRange(rows: readonly WindowRow[], scrollTop: number, viewport: number): [number, number] {
  if (!rows.length) return [0, 0];
  const start = rowAt(rows, Math.max(0, scrollTop - viewport));
  const end = Math.min(rows.length, rowAt(rows, scrollTop + viewport * 2) + 1);
  // Keep adjacent rows available even for a single unusually tall card.
  return [Math.max(0, start - 1), Math.min(rows.length, end + 1)];
}

export function renderedRows(rows: readonly WindowRow[], scrollTop: number, viewport: number, pinnedIndex = -1): Set<number> {
  const result = new Set<number>();
  const add = ([start, end]: [number, number]) => {
    for (let i = start; i < end; i++) result.add(i);
  };
  add(visibleRange(rows, scrollTop, viewport));
  if (pinnedIndex >= 0) {
    const row = rows.findIndex(r => r.start <= pinnedIndex && pinnedIndex < r.end);
    if (row >= 0) add(visibleRange(rows, rows[row].offset, viewport));
  }
  return result;
}

export function totalHeight(rows: readonly WindowRow[]): number {
  const last = rows.at(-1);
  return last ? last.offset + last.height : 0;
}

export function anchoredOffset(previous: readonly WindowRow[], previousIds: readonly string[], next: readonly WindowRow[], nextIds: readonly string[], scrollTop: number): number {
  if (!previous.length || !next.length) return scrollTop;
  const row = previous[rowAt(previous, scrollTop)];
  const index = nextIds.indexOf(previousIds[row.start]);
  const target = next.find(r => r.start <= index && index < r.end);
  return target ? Math.max(0, target.offset + scrollTop - row.offset) : scrollTop;
}
