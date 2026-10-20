// State logic of the uninstall window (src/routes/uninstall). No UI or IPC imports, so it can be
// tested with plain Node (scripts/test-uninstall.mjs).

export type CategoryId =
  | 'models'
  | 'conversations'
  | 'mcp_servers'
  | 'skills'
  | 'tools'
  | 'agents'
  | 'engines'
  | 'python_envs'
  | 'caches'
  | 'settings';

/** Display order; the backend keeps the same order. */
export const CATEGORY_ORDER: readonly CategoryId[] = [
  'models',
  'conversations',
  'mcp_servers',
  'skills',
  'tools',
  'agents',
  'engines',
  'python_envs',
  'caches',
  'settings',
];

export interface CategoryInfo {
  category: CategoryId;
  /** `null` while the backend is still measuring. */
  bytes: number | null;
  present: boolean;
  default_selected: boolean;
}

export interface Estimate {
  categories: CategoryInfo[];
  external_models: string[];
  complete: boolean;
}

export type Mode = 'everything' | 'choose';

export interface SelectionPayload {
  everything: boolean;
  categories: CategoryId[];
  external_models: string[];
}

export type StepState = 'pending' | 'running' | 'done' | 'failed';
export interface Step {
  id: string;
  state: StepState;
  error?: string;
}
export type StepEvent =
  | { state: 'started'; step: string }
  | { state: 'done'; step: string }
  | { state: 'failed'; step: string; error: string };

/** Safe defaults: user content stays, re-downloadable things go. Only what exists is ticked. */
export function initialSelection(infos: CategoryInfo[]): Set<CategoryId> {
  return new Set(infos.filter((i) => i.present && i.default_selected).map((i) => i.category));
}

/** Categories that will be removed in this mode (present ones only; absent ones are no-ops). */
export function removedCategories(mode: Mode, infos: CategoryInfo[], selected: Set<CategoryId>): CategoryId[] {
  return CATEGORY_ORDER.filter((id) => {
    const info = infos.find((i) => i.category === id);
    return info?.present && (mode === 'everything' || selected.has(id));
  });
}

export function keptCategories(mode: Mode, infos: CategoryInfo[], selected: Set<CategoryId>): CategoryId[] {
  if (mode === 'everything') return [];
  return CATEGORY_ORDER.filter((id) => infos.find((i) => i.category === id)?.present && !selected.has(id));
}

/** Sum of known sizes; `pending` while any included size is still being measured. */
export function totalBytes(infos: CategoryInfo[], ids: CategoryId[]): { bytes: number; pending: boolean } {
  let bytes = 0;
  let pending = false;
  for (const id of ids) {
    const info = infos.find((i) => i.category === id);
    if (!info) continue;
    if (info.bytes === null) pending = true;
    else bytes += info.bytes;
  }
  return { bytes, pending };
}

/** External model folders can only go together with the Models category. */
export function externalAllowed(mode: Mode, selected: Set<CategoryId>): boolean {
  return mode === 'everything' || selected.has('models');
}

export function buildSelection(mode: Mode, selected: Set<CategoryId>, external: string[]): SelectionPayload {
  const external_models = externalAllowed(mode, selected) ? [...external] : [];
  if (mode === 'everything') return { everything: true, categories: [], external_models };
  return { everything: false, categories: CATEGORY_ORDER.filter((id) => selected.has(id)), external_models };
}

/** The steps the progress screen lists, in execution order. */
export function stepIds(mode: Mode, removed: CategoryId[]): string[] {
  const ids: string[] = ['stop', ...removed];
  if (mode === 'everything' || removed.includes('settings')) ids.push('credentials');
  if (mode === 'everything') ids.push('leftovers');
  return ids;
}

export function initialSteps(ids: string[]): Step[] {
  return ids.map((id) => ({ id, state: 'pending' as StepState }));
}

export function applyStepEvent(steps: Step[], event: StepEvent): Step[] {
  const state: StepState = event.state === 'started' ? 'running' : event.state;
  const error = event.state === 'failed' ? event.error : undefined;
  if (!steps.some((s) => s.id === event.step)) return [...steps, { id: event.step, state, error }];
  return steps.map((s) => (s.id === event.step ? { ...s, state, error } : s));
}

/** 0..1 for the progress bar. */
export function progressOf(steps: Step[]): number {
  if (!steps.length) return 0;
  const finished = steps.filter((s) => s.state === 'done' || s.state === 'failed').length;
  const running = steps.some((s) => s.state === 'running') ? 0.5 : 0;
  return Math.min(1, (finished + running) / steps.length);
}

export function failedSteps(steps: Step[]): Step[] {
  return steps.filter((s) => s.state === 'failed');
}
