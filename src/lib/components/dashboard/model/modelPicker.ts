import type { LocalModel, ModelDefinition } from '../../../types.js';

export const WINDOW_THRESHOLD = 60;
export const SEARCH_THRESHOLD = 10;

/** Same normalization as domain.samePath (including its non-locale lowercase). */
export function pathKey(path: string): string {
  return path.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase();
}

export function groupModels(models: LocalModel[], definitions: ModelDefinition[]) {
  const byPath = new Map<string, ModelDefinition[]>();
  for (const d of definitions) {
    if (d.base.kind !== 'local' || !d.base.path) continue;
    const key = pathKey(d.base.path);
    const bucket = byPath.get(key);
    if (bucket) bucket.push(d);
    else byPath.set(key, [d]);
  }
  return models.filter((m) => m.kind !== 'mmproj' && m.kind !== 'adapter').map((model) => {
    const paths = new Set([model.path, ...model.shard_paths].filter(Boolean).map(pathKey));
    const defs = [...paths].flatMap((key) => byPath.get(key) ?? []);
    defs.sort((a, b) => a.alias.localeCompare(b.alias));
    return { model, defs };
  });
}

export interface PickerRow {
  key: string;
  groupIndex: number;
  value: string;
  model: LocalModel;
  definition: ModelDefinition | null;
  file: boolean;
  groupEnd: boolean;
  search: string;
}

export function flattenGroups(groups: ReturnType<typeof groupModels>): PickerRow[] {
  const seen = new Set<string>();
  return groups.flatMap(({ model, defs: candidates }, groupIndex) => {
    // A definition can match several library entries (or repeated definition records).
    const defs = candidates.filter((d) => {
      const value = `d:${d.id}`;
      if (seen.has(value)) return false;
      seen.add(value);
      return true;
    });
    const only = defs.length === 1 ? defs[0] : null;
    const fileSearch = [model.name, model.filename, model.quantization].join(' ');
    const row = (definition: ModelDefinition | null, file: boolean, groupEnd: boolean): PickerRow => ({
      key: JSON.stringify([groupIndex, model.id, definition ? `d:${definition.id}` : `f:${model.id}`]),
      groupIndex,
      value: definition ? `d:${definition.id}` : `f:${model.id}`,
      model, definition, file, groupEnd,
      search: `${fileSearch} ${definition?.alias ?? ''}`.toLocaleLowerCase(),
    });
    return [row(only, true, defs.length <= 1), ...(defs.length > 1 ? defs.map((d, i) => row(d, false, i === defs.length - 1)) : [])];
  });
}

export function filterRows(rows: PickerRow[], query: string): PickerRow[] {
  const words = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  return words.length ? rows.filter((r) => words.every((w) => r.search.includes(w))) : rows;
}

/** Prefix offsets use measured heights where available, estimates elsewhere. */
export function rowOffsets(rows: PickerRow[], heights: ReadonlyMap<string, number>): number[] {
  const offsets = [0];
  for (const row of rows) offsets.push(offsets[offsets.length - 1] + (heights.get(row.key) ?? ((row.file ? 48 : 24) + (row.groupEnd ? 4 : 0))));
  return offsets;
}

/** Apply a measurement batch without moving the visible anchor when rows above it resize. */
export function applyMeasurements(rows: PickerRow[], offsets: number[], heights: ReadonlyMap<string, number>, measurements: ReadonlyMap<string, number>, top: number) {
  const next = new Map(heights);
  let delta = 0;
  for (const [value, height] of measurements) {
    next.set(value, height);
    const index = rows.findIndex((r) => r.key === value);
    if (index >= 0 && offsets[index + 1] <= top) delta += height - (offsets[index + 1] - offsets[index]);
  }
  return { heights: next, top: top + delta };
}

export function indexAt(offsets: number[], top: number): number {
  let low = 0, high = offsets.length - 2;
  while (low < high) {
    const mid = Math.floor((low + high + 1) / 2);
    if (offsets[mid] <= top) low = mid;
    else high = mid - 1;
  }
  return Math.max(0, low);
}

export function rowWindow(offsets: number[], top: number, height: number, windowed: boolean) {
  const count = offsets.length - 1;
  const start = windowed ? Math.max(0, indexAt(offsets, top) - 5) : 0;
  const end = windowed ? Math.min(count, indexAt(offsets, top + height) + 6) : count;
  return { start, end, before: offsets[start], after: offsets[count] - offsets[end] };
}

export function scrollToRow(offsets: number[], index: number, top: number, height: number): number {
  if (offsets[index] < top) return offsets[index];
  if (offsets[index + 1] > top + height) return Math.max(0, offsets[index + 1] - height);
  return top;
}

export function navigationIndex(key: string, current: number, offsets: number[], height: number): number | null {
  const count = offsets.length - 1;
  if (!count) return null;
  const last = count - 1;
  if (key === 'Home') return 0;
  if (key === 'End') return last;
  if (key === 'ArrowDown') return Math.min(last, current + 1);
  if (key === 'ArrowUp') return Math.max(-1, current - 1);
  if (key === 'PageDown') return Math.min(last, Math.max(current + 1, indexAt(offsets, offsets[Math.max(0, current)] + height)));
  if (key === 'PageUp') return Math.max(0, Math.min(current - 1, indexAt(offsets, offsets[Math.max(0, current)] - height)));
  return null;
}

/** Enter in the filter picks the first result until a row has been activated. */
export function selectionIndex(current: number, count: number, fromFilter: boolean): number {
  const index = fromFilter && current < 0 ? 0 : current;
  return index >= 0 && index < count ? index : -1;
}

export interface PointerPosition { clientX: number; clientY: number }

/** Scroll-induced pointer events can target a new row without any mouse movement. */
export function pointerMoved(previous: PointerPosition | null, next: PointerPosition): boolean {
  return previous !== null && (previous.clientX !== next.clientX || previous.clientY !== next.clientY);
}
