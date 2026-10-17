/** Model data is inert text. Only these bounded, declarative shapes reach native views. */
export type Chart = { type: 'bar' | 'line' | 'pie'; labels: string[]; series: { name: string; values: number[] }[] };
export type UiBlock = { type: 'card' | 'callout'; title: string; text: string }
  | { type: 'list' | 'steps'; title: string; items: string[] }
  | { type: 'progress'; title: string; value: number }
  | { type: 'key-value'; title: string; entries: Record<string, string> };
export const isObject = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v);
const text = (v: unknown): v is string => typeof v === 'string' && v.length <= 4000;
const own = (v: Record<string, unknown>, keys: string[]) => Object.keys(v).every(k => keys.includes(k));

export function parseJson(source: string): unknown {
  if (source.length > 200_000) return undefined;
  try {
    return JSON.parse(source, (key, value) => {
      if (['__proto__', 'prototype', 'constructor'].includes(key)) throw new Error('Unsafe key');
      if (typeof value === 'number' && !Number.isFinite(value)) throw new Error('Non-finite number');
      return value;
    });
  } catch { return undefined; }
}

export function parseChart(source: string): Chart | null {
  const v = parseJson(source);
  if (!isObject(v) || !own(v, ['type', 'labels', 'series']) || typeof v.type !== 'string' || !['bar', 'line', 'pie'].includes(v.type)) return null;
  if (!Array.isArray(v.labels) || !v.labels.length || v.labels.length > 40 || !v.labels.every(text)) return null;
  if (!Array.isArray(v.series) || !v.series.length || v.series.length > 6) return null;
  const count = v.labels.length;
  if (!v.series.every(s => isObject(s) && own(s, ['name', 'values']) && text(s.name) && Array.isArray(s.values)
    && s.values.length === count && s.values.every(n => typeof n === 'number' && Number.isFinite(n) && Math.abs(n) <= 1e12))) return null;
  const chart = v as Chart;
  if (chart.type === 'pie' && (chart.series.length !== 1 || chart.series[0].values.some(n => n < 0) || !chart.series[0].values.some(n => n > 0))) return null;
  return chart;
}

export function parseUi(source: string): UiBlock | null {
  const v = parseJson(source);
  if (!isObject(v) || !text(v.title)) return null;
  switch (v.type) {
    case 'card': case 'callout':
      return own(v, ['type', 'title', 'text']) && text(v.text) ? v as UiBlock : null;
    case 'list': case 'steps':
      return own(v, ['type', 'title', 'items']) && Array.isArray(v.items) && v.items.length <= 100 && v.items.every(text) ? v as UiBlock : null;
    case 'progress':
      return own(v, ['type', 'title', 'value']) && typeof v.value === 'number' && Number.isFinite(v.value) && v.value >= 0 && v.value <= 100 ? v as UiBlock : null;
    case 'key-value':
      return own(v, ['type', 'title', 'entries']) && isObject(v.entries) && Object.keys(v.entries).length <= 100 && Object.values(v.entries).every(text) ? v as UiBlock : null;
    default: return null;
  }
}

export function structuredResult(source: string): Record<string, unknown> | Record<string, unknown>[] | null {
  const v = parseJson(source);
  if (isObject(v) && Object.keys(v).length <= 100) return v;
  if (Array.isArray(v) && v.length <= 500 && v.every(isObject) && new Set(v.flatMap(Object.keys)).size <= 50) return v;
  return null;
}
export function displayValue(v: unknown): string {
  return v === undefined ? '—' : typeof v === 'string' ? v : JSON.stringify(v);
}
/** Quote every cell and neutralize spreadsheet formulas, including leading control characters. */
export function toCsv(rows: string[][]): string {
  return rows.map(row => row.map(value => `"${(/^[\s]*[=+\-@]/.test(value) ? "'" : '') + value.replaceAll('"', '""')}"`).join(',')).join('\r\n');
}
export function sortRows(rows: string[][], column: number, direction: number): string[][] {
  return [...rows].sort((a, b) => {
    const x = a[column] ?? '', y = b[column] ?? '';
    const nx = Number(x), ny = Number(y);
    return direction * (x.trim() && y.trim() && Number.isFinite(nx) && Number.isFinite(ny) ? nx - ny : x.localeCompare(y));
  });
}
