// Pure helpers of the training wizard: the settings state, applying a hardware preset, building
// the job request, the dataset length histogram and run recipes (export / import). No Svelte
// here so the logic is unit-tested on its own.
import type {
  DatasetReport,
  TrainingConfig,
  TrainingCreateRequest,
  TrainingDevice,
  TrainingLrScheduler,
  TrainingMethod,
  TrainingPreset,
} from '#lib/training-types.js';

export interface WizardState {
  name: string;
  model: string;
  dataset: string;
  method: TrainingMethod;
  device: TrainingDevice;
  steps: number;
  learningRate: number;
  rank: number;
  loraAlpha: number;
  loraDropout: number;
  batchSize: number;
  gradAccum: number;
  sequenceLength: number;
  warmupRatio: number;
  lrScheduler: TrainingLrScheduler;
  weightDecay: number;
  validationFraction: number;
  checkpointSteps: number | null;
  evalSteps: number | null;
}

/** Fields a preset decides; everything else (name, model, dataset, split) is the user's. */
export const PRESET_FIELDS = [
  'method',
  'device',
  'steps',
  'learningRate',
  'rank',
  'loraAlpha',
  'loraDropout',
  'batchSize',
  'gradAccum',
  'sequenceLength',
  'warmupRatio',
  'lrScheduler',
  'weightDecay',
] as const satisfies readonly (keyof WizardState)[];

export function defaultState(): WizardState {
  return {
    name: '',
    model: '',
    dataset: '',
    method: 'lora',
    device: 'cpu',
    steps: 100,
    learningRate: 0.0002,
    rank: 16,
    loraAlpha: 32,
    loraDropout: 0.05,
    batchSize: 1,
    gradAccum: 1,
    sequenceLength: 512,
    warmupRatio: 0.05,
    lrScheduler: 'cosine',
    weightDecay: 0.01,
    validationFraction: 0.1,
    checkpointSteps: null,
    evalSteps: null,
  };
}

/** The preset's values on top of the state (name, model, dataset and split stay). */
export function applyPreset(state: WizardState, preset: TrainingPreset): WizardState {
  return {
    ...state,
    method: preset.method,
    device: preset.device,
    steps: preset.steps,
    learningRate: preset.learning_rate,
    rank: preset.rank,
    loraAlpha: preset.lora_alpha,
    loraDropout: preset.lora_dropout,
    batchSize: preset.batch_size,
    gradAccum: preset.grad_accum,
    sequenceLength: preset.sequence_length,
    warmupRatio: preset.warmup_ratio,
    lrScheduler: preset.lr_scheduler,
    weightDecay: preset.weight_decay,
  };
}

/** The preset's values as a state, for comparing against what the user has now. */
export function presetFields(preset: TrainingPreset): Pick<WizardState, (typeof PRESET_FIELDS)[number]> {
  const s = applyPreset(defaultState(), preset);
  return Object.fromEntries(PRESET_FIELDS.map((k) => [k, s[k]])) as Pick<WizardState, (typeof PRESET_FIELDS)[number]>;
}

/** Fields whose value differs from the preset (shown as "changed from recommended"). */
export function changedFields(state: WizardState, preset: TrainingPreset): (typeof PRESET_FIELDS)[number][] {
  const base = presetFields(preset);
  return PRESET_FIELDS.filter((k) => state[k] !== base[k]);
}

/** Rows seen by the optimizer per update. */
export function effectiveBatch(state: Pick<WizardState, 'batchSize' | 'gradAccum'>): number {
  return Math.max(1, state.batchSize) * Math.max(1, state.gradAccum);
}

/** The body of `training_preview` / `training_create`; blank optional values are left out. */
export function buildRequest(state: WizardState): TrainingCreateRequest {
  return {
    name: state.name.trim() || undefined,
    model: state.model.trim(),
    dataset: state.dataset.trim(),
    method: state.method,
    device: state.device,
    steps: state.steps,
    learning_rate: state.learningRate,
    rank: state.rank,
    lora_alpha: state.loraAlpha,
    lora_dropout: state.loraDropout,
    batch_size: state.batchSize,
    grad_accum: state.gradAccum,
    sequence_length: state.sequenceLength,
    warmup_ratio: state.warmupRatio,
    lr_scheduler: state.lrScheduler,
    weight_decay: state.weightDecay,
    validation_fraction: state.validationFraction,
    checkpoint_steps: state.checkpointSteps || undefined,
    eval_steps: state.evalSteps || undefined,
  };
}

/** Why a settings page cannot continue (`null` when it can). Mirrors the backend's limits. */
export function settingsError(s: WizardState): string | null {
  const within = (v: number, lo: number, hi: number) => Number.isFinite(v) && v >= lo && v <= hi;
  if (!within(s.steps, 1, 100000)) return 'steps';
  if (!(s.learningRate > 0 && s.learningRate < 1)) return 'learning_rate';
  if (!within(s.rank, 1, 256)) return 'rank';
  if (!within(s.loraAlpha, 1, 1024)) return 'lora_alpha';
  if (!within(s.loraDropout, 0, 0.5)) return 'lora_dropout';
  if (!within(s.batchSize, 1, 64)) return 'batch_size';
  if (!within(s.gradAccum, 1, 256)) return 'grad_accum';
  if (!within(s.sequenceLength, 32, 8192)) return 'sequence_length';
  if (!within(s.warmupRatio, 0, 0.5)) return 'warmup_ratio';
  if (!within(s.weightDecay, 0, 1)) return 'weight_decay';
  if (!within(s.validationFraction, 0.01, 0.5)) return 'validation_fraction';
  return null;
}

// --- dataset length histogram ------------------------------------------------------------------

export const HISTOGRAM_EDGES = [32, 64, 128, 256, 512, 1024, 2048, 4096, 8192] as const;

export interface HistogramBar {
  label: string;
  count: number;
  /** Share of the tallest bar, 0..1. */
  height: number;
  /** The bar is entirely beyond the sequence length: those rows get cut. */
  truncated: boolean;
}

export function histogramBars(report: DatasetReport, sequenceLength: number): HistogramBar[] {
  const counts = report.histogram ?? [];
  const max = Math.max(1, ...counts);
  return counts.map((count, i) => {
    const lower = i === 0 ? 0 : HISTOGRAM_EDGES[i - 1];
    const label = i < HISTOGRAM_EDGES.length ? `≤${HISTOGRAM_EDGES[i]}` : `>${HISTOGRAM_EDGES[HISTOGRAM_EDGES.length - 1]}`;
    return { label, count, height: count / max, truncated: lower >= sequenceLength };
  });
}

/** At least this share of rows is longer than `sequenceLength` (whole buckets only). */
export function truncatedShare(report: DatasetReport, sequenceLength: number): number {
  const bars = histogramBars(report, sequenceLength);
  const total = bars.reduce((a, b) => a + b.count, 0);
  if (total === 0) return 0;
  return bars.filter((b) => b.truncated).reduce((a, b) => a + b.count, 0) / total;
}

// --- recipes -----------------------------------------------------------------------------------

export const RECIPE_VERSION = 1;

/** A run's settings as JSON text that can be saved, shared and imported again. */
export function recipeFromConfig(config: TrainingConfig): string {
  return JSON.stringify(
    {
      recipe: RECIPE_VERSION,
      name: config.name,
      model: config.model,
      dataset: config.dataset,
      method: config.method,
      device: config.device,
      steps: config.steps,
      learning_rate: config.learning_rate,
      rank: config.rank,
      lora_alpha: config.lora_alpha || config.rank * 2,
      lora_dropout: config.lora_dropout,
      batch_size: config.batch_size,
      grad_accum: config.grad_accum,
      sequence_length: config.sequence_length,
      warmup_ratio: config.warmup_ratio,
      lr_scheduler: config.lr_scheduler,
      weight_decay: config.weight_decay,
      validation_fraction: config.validation_fraction,
      checkpoint_steps: config.checkpoint_steps,
      eval_steps: config.eval_steps,
    },
    null,
    2,
  );
}

export type RecipeResult = { ok: true; state: WizardState } | { ok: false; error: 'json' | 'version' | 'field'; field?: string };

const METHODS: TrainingMethod[] = ['lora', 'qlora'];
const DEVICES: TrainingDevice[] = ['cpu', 'cuda', 'mps', 'mlx'];
const SCHEDULERS: TrainingLrScheduler[] = ['linear', 'cosine', 'constant'];

/** Reads recipe text on top of the defaults. Unknown keys are ignored; bad values are named. */
export function parseRecipe(text: string): RecipeResult {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return { ok: false, error: 'json' };
  }
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return { ok: false, error: 'json' };
  const r = raw as Record<string, unknown>;
  if (r.recipe !== RECIPE_VERSION) return { ok: false, error: 'version' };
  const state = defaultState();
  const num = (key: string, set: (v: number) => void): string | null => {
    if (!(key in r)) return null;
    const v = r[key];
    if (typeof v !== 'number' || !Number.isFinite(v)) return key;
    set(v);
    return null;
  };
  const str = (key: string, set: (v: string) => void): string | null => {
    if (!(key in r)) return null;
    if (typeof r[key] !== 'string') return key;
    set(r[key] as string);
    return null;
  };
  const oneOf = <T extends string>(key: string, allowed: T[], set: (v: T) => void): string | null => {
    if (!(key in r)) return null;
    if (!allowed.includes(r[key] as T)) return key;
    set(r[key] as T);
    return null;
  };
  const checks = [
    str('name', (v) => (state.name = v)),
    str('model', (v) => (state.model = v)),
    str('dataset', (v) => (state.dataset = v)),
    oneOf('method', METHODS, (v) => (state.method = v)),
    oneOf('device', DEVICES, (v) => (state.device = v)),
    oneOf('lr_scheduler', SCHEDULERS, (v) => (state.lrScheduler = v)),
    num('steps', (v) => (state.steps = v)),
    num('learning_rate', (v) => (state.learningRate = v)),
    num('rank', (v) => {
      state.rank = v;
      state.loraAlpha = v * 2;
    }),
    num('lora_alpha', (v) => (state.loraAlpha = v)),
    num('lora_dropout', (v) => (state.loraDropout = v)),
    num('batch_size', (v) => (state.batchSize = v)),
    num('grad_accum', (v) => (state.gradAccum = v)),
    num('sequence_length', (v) => (state.sequenceLength = v)),
    num('warmup_ratio', (v) => (state.warmupRatio = v)),
    num('weight_decay', (v) => (state.weightDecay = v)),
    num('validation_fraction', (v) => (state.validationFraction = v)),
    num('checkpoint_steps', (v) => (state.checkpointSteps = v)),
    num('eval_steps', (v) => (state.evalSteps = v)),
  ];
  const bad = checks.find((c) => c !== null);
  if (bad) return { ok: false, error: 'field', field: bad };
  const range = settingsError(state);
  if (range) return { ok: false, error: 'field', field: range };
  return { ok: true, state };
}
