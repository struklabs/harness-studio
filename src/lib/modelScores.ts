import type { FitCategory } from '#lib/types.js';

/** What a person wants to do with the model; picks which facts the task score weighs. */
export type ScoreTask = 'chat' | 'tools' | 'vision' | 'image' | 'speech';

/** One measured record of a model on a device (`list_model_perf`). */
export interface ModelPerfRecord {
  model_key: string;
  model_name: string;
  device: string;
  updated_at: number;
  load_ms: number | null;
  bench_pp_tps: number | null;
  bench_tg_tps: number | null;
  image_secs: number | null;
  audio_secs: number | null;
  tool_probe: { passed: number; total: number; failed: string[]; at: number } | null;
}

/** A reason behind a score: a locale key (`ms.r_*`) and its parameters. */
export interface ScoreReason {
  key: string;
  params?: Record<string, string | number>;
}

export interface Score {
  /** 0 to 100; null when there is nothing to judge from. */
  value: number | null;
  /** True when a measurement on this machine (not only an estimate) went into the value. */
  measured: boolean;
  reasons: ScoreReason[];
}

export interface Scores {
  hardware: Score;
  task: Score;
  quality: Score;
  speed: Score;
}

export interface ScoreInput {
  task: ScoreTask;
  /** Hardware fit of the file on this machine; null while unknown. */
  fit: FitCategory | null;
  /** `llm`, `embedding`, `reranker`, `asr`, `image`, `tts`, `pipeline` ... */
  kind: string | null;
  caps: { tools?: boolean; vision?: boolean; image?: boolean; asr?: boolean; tts?: boolean; chat?: boolean } | null;
  /** Parameters in billions; null when unknown. */
  paramsB: number | null;
  /** Quantization name ("Q4_K_M", "BF16"); null when unknown. */
  quantization: string | null;
  measured: ModelPerfRecord | null;
}

/** Same normalization as `model_perf::normalize_name` in Rust. */
export function normalizeModelName(name: string): string {
  const base = (name.split(/[/\\]/).pop() ?? name).trim().toLowerCase();
  return base.replace(/\.(gguf|bin|safetensors|onnx)$/, '');
}

/** Name the gateway serves a file under: no extension, split-set suffix (`-00001-of-00003`) removed. */
export function modelKeyOfFile(fileName: string): string {
  return normalizeModelName(fileName).replace(/-\d+-of-\d+$/, '');
}

/** Record of this model (by name) on the given device, newest first; any device when `device` is null. */
export function perfFor(records: ModelPerfRecord[], name: string, device: string | null = null): ModelPerfRecord | null {
  const key = modelKeyOfFile(name);
  return records.find((r) => r.model_key === key && (device === null || r.device === device)) ?? null;
}

/** Parameters in billions from a repo or file name ("Qwen3-4B-GGUF" -> 4, "gemma-3-270m" -> 0.27); null when none. */
export function paramsFromName(name: string): number | null {
  const base = name.split('/').pop() ?? name;
  const b = base.match(/(?:^|[-_. ])(\d+(?:\.\d+)?)\s*b(?=$|[-_. ])/i);
  if (b) return parseFloat(b[1]);
  const m = base.match(/(?:^|[-_. ])(\d+(?:\.\d+)?)\s*m(?=$|[-_. ])/i);
  return m ? parseFloat(m[1]) / 1000 : null;
}

const clamp = (n: number) => Math.max(0, Math.min(100, Math.round(n)));

/** Linear interpolation through `[x, y]` points sorted by x; flat outside the range. */
export function interp(x: number, points: [number, number][]): number {
  if (x <= points[0][0]) return points[0][1];
  for (let i = 1; i < points.length; i++) {
    const [x1, y1] = points[i];
    if (x <= x1) {
      const [x0, y0] = points[i - 1];
      return y0 + ((y1 - y0) * (x - x0)) / (x1 - x0);
    }
  }
  return points[points.length - 1][1];
}

const FIT_SCORE: Record<FitCategory, number> = { full_gpu: 100, tight: 75, hybrid: 55, cpu_heavy: 30, oom_risk: 5 };
const FIT_SPEED: Record<FitCategory, number> = { full_gpu: 85, tight: 70, hybrid: 45, cpu_heavy: 20, oom_risk: 5 };

const round1 = (n: number) => Math.round(n * 10) / 10;

export function hardwareScore(i: ScoreInput): Score {
  if (!i.fit) return { value: null, measured: false, reasons: [{ key: 'ms.r_fit_unknown' }] };
  let value = FIT_SCORE[i.fit];
  const reasons: ScoreReason[] = [{ key: `ms.r_fit_${i.fit}` }];
  let measured = false;
  const load = i.measured?.load_ms;
  if (load) {
    measured = true;
    const seconds = round1(load / 1000);
    if (load > 60_000) {
      value -= 10;
      reasons.push({ key: 'ms.r_load_slow', params: { seconds } });
    } else {
      reasons.push({ key: 'ms.r_load_ok', params: { seconds } });
    }
  }
  return { value: clamp(value), measured, reasons };
}

export function taskScore(i: ScoreInput): Score {
  const caps = i.caps ?? {};
  const kind = i.kind ?? 'llm';
  const probe = i.measured?.tool_probe ?? null;
  const match = (ok: boolean): Score => ok
    ? { value: 100, measured: false, reasons: [{ key: 'ms.r_task_match' }] }
    : { value: 0, measured: false, reasons: [{ key: 'ms.r_task_mismatch' }] };
  switch (i.task) {
    case 'image': return match(kind === 'image' || caps.image === true);
    case 'speech': return match(['asr', 'tts'].includes(kind) || caps.asr === true || caps.tts === true);
    case 'vision': {
      if (kind !== 'llm') return match(false);
      return caps.vision ? match(true) : { value: 0, measured: false, reasons: [{ key: 'ms.r_task_no_vision' }] };
    }
    case 'tools': {
      if (kind !== 'llm') return match(false);
      if (probe && probe.total > 0) {
        return {
          value: clamp(20 + (80 * probe.passed) / probe.total),
          measured: true,
          reasons: [{ key: 'ms.r_task_probe', params: { passed: probe.passed, total: probe.total } }],
        };
      }
      return caps.tools
        ? { value: 80, measured: false, reasons: [{ key: 'ms.r_task_tools_cap' }] }
        : { value: 20, measured: false, reasons: [{ key: 'ms.r_task_no_tools' }] };
    }
    default: return match(kind === 'llm' && caps.chat !== false);
  }
}

/** Quality factor of a quantization: how much of the full-precision model is left. */
function quantFactor(bits: number): number {
  return interp(bits, [[2, 0.55], [3, 0.75], [4, 0.88], [5, 0.94], [6, 0.97], [8, 1]]);
}

function bitsOf(q: string): number | null {
  const u = q.toUpperCase();
  if (/^(BF16|F16|FP16)/.test(u)) return 16;
  if (/^(F32|FP32)/.test(u)) return 32;
  const m = u.match(/(\d+)/);
  return m ? parseInt(m[1], 10) : null;
}

export function qualityScore(i: ScoreInput): Score {
  const reasons: ScoreReason[] = [];
  const bits = i.quantization ? bitsOf(i.quantization) : null;
  let value: number | null = null;
  if (i.paramsB && i.paramsB > 0) {
    // 1 B -> 30, 7 B -> 60, 30 B -> 82, 70 B and more -> 95.
    const size = interp(Math.log2(i.paramsB), [[0, 30], [Math.log2(3), 45], [Math.log2(7), 60], [Math.log2(13), 70], [Math.log2(30), 82], [Math.log2(70), 95]]);
    value = size * (bits ? quantFactor(Math.min(bits, 8)) : 0.9);
    reasons.push({ key: 'ms.r_q_size', params: { params: round1(i.paramsB) } });
    if (bits) reasons.push({ key: 'ms.r_q_quant', params: { bits } });
  } else {
    reasons.push({ key: 'ms.r_q_unknown' });
  }
  const probe = i.measured?.tool_probe;
  let measured = false;
  if (probe && probe.total > 0 && i.kind === 'llm') {
    // Reliable tool calls are evidence of a capable model: they move the estimate by up to 20 %.
    const evidence = (100 * probe.passed) / probe.total;
    value = value === null ? evidence : value * 0.8 + evidence * 0.2;
    measured = true;
    reasons.push({ key: 'ms.r_q_probe', params: { passed: probe.passed, total: probe.total } });
  }
  return { value: value === null ? null : clamp(value), measured, reasons };
}

export function speedScore(i: ScoreInput): Score {
  const m = i.measured;
  const reasons: ScoreReason[] = [];
  const parts: number[] = [];
  if (i.task === 'image') {
    if (m?.image_secs) {
      parts.push(interp(m.image_secs, [[5, 100], [15, 75], [60, 45], [180, 20], [600, 5]]));
      reasons.push({ key: 'ms.r_s_image', params: { seconds: round1(m.image_secs) } });
    }
  } else if (i.task === 'speech') {
    if (m?.audio_secs) {
      parts.push(interp(m.audio_secs, [[2, 100], [10, 70], [30, 40], [90, 15], [300, 5]]));
      reasons.push({ key: 'ms.r_s_audio', params: { seconds: round1(m.audio_secs) } });
    }
  } else {
    if (m?.bench_tg_tps) {
      parts.push(interp(m.bench_tg_tps, [[1, 5], [3, 20], [8, 40], [15, 60], [30, 80], [60, 100]]));
      reasons.push({ key: 'ms.r_s_tg', params: { tps: round1(m.bench_tg_tps) } });
    }
    if (m?.bench_pp_tps) {
      reasons.push({ key: 'ms.r_s_pp', params: { tps: Math.round(m.bench_pp_tps) } });
    }
  }
  if (parts.length) return { value: clamp(parts.reduce((a, b) => a + b, 0) / parts.length), measured: true, reasons };
  if (!i.fit) return { value: null, measured: false, reasons: [{ key: 'ms.r_s_unknown' }] };
  // No measurement: the fit decides (a model that does not fit fully is slow), and a large
  // model is slower than a small one on the same hardware.
  const big = i.paramsB && i.paramsB > 13 ? 10 : 0;
  return { value: clamp(FIT_SPEED[i.fit] - big), measured: false, reasons: [{ key: 'ms.r_s_estimate' }] };
}

export function scoreModel(i: ScoreInput): Scores {
  return { hardware: hardwareScore(i), task: taskScore(i), quality: qualityScore(i), speed: speedScore(i) };
}
