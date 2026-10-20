// Playground > Image > ComfyUI: the user's own ComfyUI (core `comfyui`; Harness never runs it).
import { invoke } from '#lib/ipc.js';

export const COMFYUI_DEFAULT_URL = 'http://127.0.0.1:8188';

export interface ComfyStatus {
  url: string;
  version: string | null;
  python_version: string | null;
  os: string | null;
  devices: { name: string; kind: string; vram_total: number; vram_free: number }[];
}

export interface ComfyTemplate {
  source: string;
  name: string;
  title: string;
  description: string;
  category: string;
}

export interface ComfyCatalog {
  workflows: { path: string; size: number | null; modified: number | null }[];
  templates: ComfyTemplate[];
  checkpoints: string[];
}

export type WorkflowSource =
  | { kind: 'builtin' }
  | { kind: 'saved'; path: string }
  | { kind: 'template'; source: string; name: string };

export interface Txt2ImgParams {
  prompt: string;
  negative: string;
  seed: number | null;
  width: number | null;
  height: number | null;
  steps: number | null;
  cfg: number | null;
  checkpoint: string | null;
}

export interface RunEvent {
  prompt_id: string;
  stage: 'queued' | 'running' | 'progress';
  value: number;
  max: number;
  node: string | null;
}

export interface RunResult {
  prompt_id: string;
  seed: number;
  images: { path: string; format: string; data: string }[];
}

export const comfyApi = {
  settings: () => invoke<{ url: string }>('comfyui_get_settings'),
  save: (url: string) => invoke<{ url: string }>('comfyui_save_settings', { url }),
  status: (url: string | null) => invoke<ComfyStatus>('comfyui_status', { url }),
  catalog: () => invoke<ComfyCatalog>('comfyui_catalog'),
  run: (source: WorkflowSource, params: Txt2ImgParams) => invoke<RunResult>('comfyui_run', { source, params }),
  cancel: (promptId: string) => invoke<void>('comfyui_cancel', { promptId }),
  open: (path: string) => invoke<void>('comfyui_open_output', { path }),
  reveal: (path: string) => invoke<void>('comfyui_reveal_output', { path }),
};

/** `768x512` -> [768, 512]: multiples of 8, 64..8192 (core `workflow::SIDE_RANGE`); else null. */
export function parseComfySize(size: string): [number, number] | null {
  const m = size.trim().match(/^(\d+)\s*[x×]\s*(\d+)$/i);
  if (!m) return null;
  const sides = [Number(m[1]), Number(m[2])] as [number, number];
  return sides.every((s) => s >= 64 && s <= 8192 && s % 8 === 0) ? sides : null;
}

/** One string per workflow choice, for a `<select>`: `builtin`, `saved:<path>`, `template:<source>/<name>`. */
export function sourceKey(s: WorkflowSource): string {
  if (s.kind === 'saved') return `saved:${s.path}`;
  if (s.kind === 'template') return `template:${s.source}/${s.name}`;
  return 'builtin';
}

export function parseSourceKey(key: string): WorkflowSource {
  if (key.startsWith('saved:') && key.length > 6) return { kind: 'saved', path: key.slice(6) };
  const m = key.match(/^template:([^/]+)\/(.+)$/);
  if (m) return { kind: 'template', source: m[1], name: m[2] };
  return { kind: 'builtin' };
}

/** A whole number from a form field; empty or not a number = `null` (the workflow's own). */
function wholeNumber(text: string): number | null {
  const v = text.trim();
  return /^\d+$/.test(v) ? Number(v) : null;
}

/** The Image tab's form as run parameters. Seed empty or -1 = random; count is the workflow's. */
export function paramsFromForm(
  form: { prompt: string; negative: string; size: string; steps: string; cfgScale: string; seed: string },
  checkpoint: string | null,
): Txt2ImgParams {
  const size = form.size.trim() ? parseComfySize(form.size) : null;
  const cfg = form.cfgScale.trim() === '' ? NaN : Number(form.cfgScale.trim());
  return {
    prompt: form.prompt.trim(),
    negative: form.negative.trim(),
    seed: wholeNumber(form.seed),
    width: size?.[0] ?? null,
    height: size?.[1] ?? null,
    steps: wholeNumber(form.steps),
    cfg: Number.isFinite(cfg) ? cfg : null,
    checkpoint: checkpoint || null,
  };
}

/** Step progress as a percentage; `null` while the steps are unknown (queued, polling only). */
export function progressPercent(e: RunEvent | null): number | null {
  if (!e || e.stage !== 'progress' || e.max <= 0) return null;
  return Math.max(0, Math.min(100, Math.round((e.value / e.max) * 100)));
}

/** Templates grouped by category (custom-node ones under their module), in list order. */
export function groupTemplates(templates: ComfyTemplate[]): [string, ComfyTemplate[]][] {
  const groups = new Map<string, ComfyTemplate[]>();
  for (const t of templates) {
    const key = t.category || t.source;
    groups.set(key, [...(groups.get(key) ?? []), t]);
  }
  return [...groups];
}
