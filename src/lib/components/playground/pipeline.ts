// Requests and responses of the gateway's /v1/hf (Hugging Face Inference API shapes). Pure helpers.

export interface LabelScore {
  label: string;
  score: number;
}

export interface EntitySpan {
  entity: string;
  word: string;
  score: number;
  start: number;
  end: number;
}

export interface FillResult {
  token: string;
  score: number;
  sequence: string;
}

export interface AnswerResult {
  answer: string;
  score: number;
  start: number;
  end: number;
}

export interface Detection {
  label: string;
  score: number;
  box: { xmin: number; ymin: number; xmax: number; ymax: number };
}

export interface MaskResult {
  label: string;
  /** `data:image/png;base64,...` of the 8-bit mask (255 = this label). */
  mask: string;
}

export interface DepthResult {
  /** `data:image/png;base64,...` of the depth map (brighter = nearer). */
  image: string;
  width: number;
  height: number;
}

export type PipelineResult =
  | { kind: 'labels'; items: LabelScore[] }
  | { kind: 'entities'; items: EntitySpan[] }
  | { kind: 'fills'; items: FillResult[] }
  | { kind: 'answers'; items: AnswerResult[] }
  | { kind: 'detections'; items: Detection[] }
  | { kind: 'masks'; items: MaskResult[] }
  | { kind: 'depth'; value: DepthResult }
  | { kind: 'raw'; value: unknown };

/** Tasks that take one image (harness-onnx vision heads, the Python worker's image tasks). */
export const IMAGE_TASKS = ['image-classification', 'object-detection', 'image-segmentation', 'depth-estimation'] as const;

export function isImageTask(task: string | undefined): boolean {
  return (IMAGE_TASKS as readonly string[]).includes(task ?? '');
}

/** The request of an image task: the image as a data URL. */
export function buildImageRequest(task: string, image: string): Record<string, unknown> | null {
  if (!image.startsWith('data:')) return null;
  if (task === 'image-classification') return { inputs: image, parameters: { top_k: 5 } };
  if (task === 'object-detection') return { inputs: image, parameters: { threshold: 0.5 } };
  return { inputs: image };
}

const png = (b64: string) => (b64.startsWith('data:') ? b64 : `data:image/png;base64,${b64}`);

/** Tasks with their own form; any other task gets a JSON body editor. */
export const FORM_TASKS = [
  'text-classification',
  'zero-shot-classification',
  'token-classification',
  'fill-mask',
  'question-answering',
] as const;

export type FormTask = (typeof FORM_TASKS)[number];

export function isFormTask(task: string | undefined): task is FormTask {
  return (FORM_TASKS as readonly string[]).includes(task ?? '');
}

export interface PipelineForm {
  text: string;
  labels: string;
  multiLabel: boolean;
  question: string;
  context: string;
}

/** Comma separated labels, trimmed, empty ones dropped. */
export function splitLabels(labels: string): string[] {
  return labels
    .split(',')
    .map((l) => l.trim())
    .filter(Boolean);
}

/** The request body for `task`, or null while the form is incomplete. */
export function buildRequest(task: FormTask, form: PipelineForm): Record<string, unknown> | null {
  const text = form.text.trim();
  switch (task) {
    case 'zero-shot-classification': {
      const labels = splitLabels(form.labels);
      if (!text || labels.length < 2) return null;
      return { inputs: text, parameters: { candidate_labels: labels, multi_label: form.multiLabel } };
    }
    case 'token-classification':
      return text ? { inputs: text, parameters: { aggregation_strategy: 'simple' } } : null;
    case 'fill-mask':
      return text ? { inputs: text, parameters: { top_k: 5 } } : null;
    case 'question-answering': {
      const question = form.question.trim();
      const context = form.context.trim();
      return question && context ? { inputs: { question, context }, parameters: { top_k: 3 } } : null;
    }
    default:
      return text ? { inputs: text } : null;
  }
}

const isNum = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);
const isStr = (v: unknown): v is string => typeof v === 'string';
const obj = (v: unknown): Record<string, unknown> | null =>
  v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : null;
/** A single input is answered with a list holding one list in some tasks (`[[...]]`). */
const flat = (v: unknown): unknown[] => (Array.isArray(v) ? (v.length === 1 && Array.isArray(v[0]) ? v[0] : v) : []);

/** Reads a response into the result view of `task`; anything unexpected is shown raw. */
export function parseResponse(task: string | undefined, json: unknown): PipelineResult {
  const depth = obj(json);
  if (depth && isStr(depth.depth)) {
    return {
      kind: 'depth',
      value: { image: png(depth.depth), width: isNum(depth.width) ? depth.width : 0, height: isNum(depth.height) ? depth.height : 0 },
    };
  }
  const zeroShot = obj(json);
  if (zeroShot && Array.isArray(zeroShot.labels) && Array.isArray(zeroShot.scores)) {
    const scores = zeroShot.scores;
    const items = zeroShot.labels.flatMap((label, i) => (isStr(label) && isNum(scores[i]) ? [{ label, score: scores[i] }] : []));
    return { kind: 'labels', items };
  }
  const one = obj(json);
  if (one && isStr(one.answer) && isNum(one.score)) {
    return { kind: 'answers', items: [answerOf(one)] };
  }
  const list = flat(json).map(obj);
  if (list.length === 0 || list.some((r) => r === null)) {
    if (Array.isArray(json) && json.length === 0) {
      if (task === 'token-classification') return { kind: 'entities', items: [] };
      if (task === 'object-detection') return { kind: 'detections', items: [] };
      if (task === 'image-segmentation') return { kind: 'masks', items: [] };
    }
    return { kind: 'raw', value: json };
  }
  const rows = list as Record<string, unknown>[];
  if (rows.every((r) => isStr(r.mask) && isStr(r.label))) {
    return { kind: 'masks', items: rows.map((r) => ({ label: (r.label as string).trim(), mask: png(r.mask as string) })) };
  }
  if (rows.every((r) => obj(r.box) && isStr(r.label) && isNum(r.score))) {
    return {
      kind: 'detections',
      items: rows.map((r) => {
        const b = obj(r.box) as Record<string, unknown>;
        const n = (k: string) => (isNum(b[k]) ? (b[k] as number) : 0);
        return { label: r.label as string, score: r.score as number, box: { xmin: n('xmin'), ymin: n('ymin'), xmax: n('xmax'), ymax: n('ymax') } };
      }),
    };
  }
  if (rows.every((r) => isStr(r.answer) && isNum(r.score))) {
    return { kind: 'answers', items: rows.map(answerOf) };
  }
  if (rows.every((r) => isStr(r.token_str) && isNum(r.score))) {
    return {
      kind: 'fills',
      items: rows.map((r) => ({ token: (r.token_str as string).trim(), score: r.score as number, sequence: isStr(r.sequence) ? r.sequence : '' })),
    };
  }
  if (rows.every((r) => (isStr(r.entity_group) || isStr(r.entity)) && isNum(r.score))) {
    return {
      kind: 'entities',
      items: rows.map((r) => ({
        entity: (isStr(r.entity_group) ? r.entity_group : r.entity) as string,
        word: isStr(r.word) ? r.word : '',
        score: r.score as number,
        start: isNum(r.start) ? r.start : 0,
        end: isNum(r.end) ? r.end : 0,
      })),
    };
  }
  if (rows.every((r) => isStr(r.label) && isNum(r.score))) {
    return { kind: 'labels', items: rows.map((r) => ({ label: r.label as string, score: r.score as number })) };
  }
  return { kind: 'raw', value: json };
}

function answerOf(r: Record<string, unknown>): AnswerResult {
  return {
    answer: r.answer as string,
    score: r.score as number,
    start: isNum(r.start) ? r.start : 0,
    end: isNum(r.end) ? r.end : 0,
  };
}

/** `text` cut into plain and entity pieces (spans sorted, overlaps skipped) for highlighting. */
export function highlight(text: string, spans: EntitySpan[]): { text: string; entity?: string }[] {
  const out: { text: string; entity?: string }[] = [];
  let at = 0;
  for (const s of [...spans].sort((a, b) => a.start - b.start)) {
    if (s.start < at || s.end <= s.start || s.end > text.length) continue;
    if (s.start > at) out.push({ text: text.slice(at, s.start) });
    out.push({ text: text.slice(s.start, s.end), entity: s.entity });
    at = s.end;
  }
  if (at < text.length) out.push({ text: text.slice(at) });
  return out;
}

/** POST {baseUrl}/hf/{model}. */
export async function runPipeline(
  baseUrl: string,
  model: string,
  body: unknown,
  headers: Record<string, string>,
  signal?: AbortSignal,
): Promise<{ json: unknown; ms: number }> {
  const started = performance.now();
  const res = await fetch(`${baseUrl}/hf/${encodeURIComponent(model)}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', ...headers },
    body: JSON.stringify(body),
    signal,
  });
  const text = await res.text();
  const ms = performance.now() - started;
  if (!res.ok) throw new Error(`HTTP ${res.status}: ${errorText(text) || res.statusText}`);
  return { json: JSON.parse(text), ms };
}

function errorText(text: string): string {
  try {
    const j = JSON.parse(text) as { error?: unknown };
    const e = obj(j.error);
    return isStr(j.error) ? j.error : e && isStr(e.message) ? e.message : text;
  } catch {
    return text;
  }
}
