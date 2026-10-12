// Types mirroring `src-tauri/src/benchmark.rs` plus pure helpers used by the
// benchmark UI. Kept dependency-free (no imports) so the helpers are easy to test.

export type FlashAttnMode = 'on' | 'off' | 'auto';

export const CACHE_TYPES = ['f32', 'f16', 'bf16', 'q8_0', 'q4_0', 'q4_1', 'iq4_nl', 'q5_0', 'q5_1'] as const;
export type CacheType = (typeof CACHE_TYPES)[number];

export interface BenchmarkRequest {
  model_path: string;
  n_prompt: number[];
  n_gen: number[];
  n_depth: number[];
  /** 99 = all layers on the GPU. */
  gpu_layers: number[];
  flash_attn: FlashAttnMode[];
  cache_type_k: string[];
  cache_type_v: string[];
  threads?: number | null;
  batch_size?: number | null;
  ubatch_size?: number | null;
  repetitions?: number | null;
  /** Servers that held GPU memory when the run started (the app fills this in). */
  concurrent_servers?: number;
}

/** Running servers a benchmark would share the GPU with (`benchmark_preflight`). */
export interface BenchPreflight {
  servers: { id: string; name: string }[];
  vram_free_bytes: number | null;
  vram_total_bytes: number | null;
  contended: boolean;
}

/** How many servers ran next to a saved run; its numbers are not comparable with an idle GPU when above 0. */
export function concurrentServers(run: { request?: { concurrent_servers?: number | null } }): number {
  return Math.max(0, run.request?.concurrent_servers ?? 0);
}

export interface BenchResult {
  model_type: string;
  model_size: number;
  model_n_params: number;
  backends: string;
  gpu_info: string;
  cpu_info: string;
  build_commit: string;
  build_number: number | null;
  n_prompt: number;
  n_gen: number;
  n_depth: number;
  n_gpu_layers: number;
  n_threads: number;
  n_batch: number;
  n_ubatch: number;
  type_k: string;
  type_v: string;
  /** -1 auto / 0 off / 1 on (older builds report a bool). */
  flash_attn: number | boolean | null;
  avg_ts: number;
  stddev_ts: number;
}

export type RunStatus = 'completed' | 'cancelled' | 'error';

export interface BenchmarkRun {
  id: string;
  /** Unix seconds. */
  started_at: number;
  finished_at: number;
  model_name: string;
  request: BenchmarkRequest;
  results: BenchResult[];
  status: RunStatus;
  error: string | null;
}

export interface BenchmarkStatus {
  /** Id of the current (or latest) run; the saved run gets the same id. */
  run_id?: string | null;
  running: boolean;
  current: number;
  total: number;
  phase: string;
  results: BenchResult[];
  log_tail: string[];
  last_run: BenchmarkRun | null;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

export type TestKind = 'pp' | 'tg' | 'pg';

export function testKind(r: Pick<BenchResult, 'n_prompt' | 'n_gen'>): TestKind {
  if (r.n_prompt > 0 && r.n_gen > 0) return 'pg';
  return r.n_gen > 0 ? 'tg' : 'pp';
}

/** "pp512", "tg128", "pp512+tg128", with " @ d4096" when a depth is set. */
export function testLabel(r: Pick<BenchResult, 'n_prompt' | 'n_gen' | 'n_depth'>): string {
  const parts: string[] = [];
  if (r.n_prompt > 0) parts.push(`pp${r.n_prompt}`);
  if (r.n_gen > 0) parts.push(`tg${r.n_gen}`);
  let label = parts.join('+') || 'pp0';
  if (r.n_depth > 0) label += ` @ d${r.n_depth}`;
  return label;
}

export function flashAttnLabel(v: BenchResult['flash_attn']): string {
  if (v === true || v === 1) return 'on';
  if (v === false || v === 0) return 'off';
  if (v === -1) return 'auto';
  return '?';
}

export type ParsedList = { values: number[]; error: boolean };

/**
 * Parses "512, 1024" into positive integers (or non-negative with `allowZero`).
 * Empty input is valid and yields []. Duplicates are dropped, order kept.
 */
export function parseIntList(input: string, allowZero = false): ParsedList {
  const values: number[] = [];
  const parts = input
    .split(/[,\s;]+/)
    .map((s) => s.trim())
    .filter(Boolean);
  for (const p of parts) {
    if (!/^\d+$/.test(p)) return { values: [], error: true };
    const n = Number(p);
    if (!Number.isSafeInteger(n) || (!allowZero && n <= 0) || n > 1_000_000) return { values: [], error: true };
    if (!values.includes(n)) values.push(n);
  }
  return { values, error: false };
}

/** Parses a single optional positive integer ("" → null). */
export function parseOptionalInt(input: string): { value: number | null; error: boolean } {
  const s = input.trim();
  if (!s) return { value: null, error: false };
  if (!/^\d+$/.test(s) || Number(s) <= 0) return { value: null, error: true };
  return { value: Number(s), error: false };
}

/**
 * Approximate number of llama-bench tests: parameter combinations × test
 * sizes. Each n_prompt value and each n_gen value is its own test (llama-bench
 * runs pp and tg separately), and empty lists count as llama-bench defaults (1).
 */
export function testCount(req: Pick<BenchmarkRequest, 'n_prompt' | 'n_gen' | 'n_depth' | 'gpu_layers' | 'flash_attn' | 'cache_type_k' | 'cache_type_v'>): number {
  const n = (a: unknown[]) => Math.max(1, a.length);
  const sizes = req.n_prompt.filter((v) => v > 0).length + req.n_gen.filter((v) => v > 0).length;
  const fa = new Set(req.flash_attn).size;
  return (
    Math.max(1, sizes) *
    n(req.n_depth) *
    n(req.gpu_layers) *
    Math.max(1, fa) *
    n(req.cache_type_k) *
    n(req.cache_type_v)
  );
}

/** Evenly spread -ngl values for a layer sweep. */
export function gpuLayerSweep(blockCount: number | null | undefined): number[] {
  if (!blockCount || blockCount <= 0) return [0, 16, 32, 99];
  const vals = [0, 0.25, 0.5, 0.75].map((f) => Math.round(blockCount * f));
  vals.push(99);
  return [...new Set(vals)];
}

/** Index of the fastest row per test label (e.g. best pp512, best tg128). */
export function bestByTest(results: BenchResult[]): Set<number> {
  const best = new Map<string, number>();
  results.forEach((r, i) => {
    const key = testLabel(r);
    const cur = best.get(key);
    if (cur === undefined || r.avg_ts > results[cur].avg_ts) best.set(key, i);
  });
  return new Set(best.values());
}

/** Highest t/s among rows of the given kind, or null. */
export function bestTs(results: BenchResult[], kind: TestKind): number | null {
  let best: number | null = null;
  for (const r of results) if (testKind(r) === kind && (best === null || r.avg_ts > best)) best = r.avg_ts;
  return best;
}

/** Max t/s per test label, used to scale the bar widths. */
export function maxByTest(results: BenchResult[]): Map<string, number> {
  const m = new Map<string, number>();
  for (const r of results) {
    const k = testLabel(r);
    m.set(k, Math.max(m.get(k) ?? 0, r.avg_ts));
  }
  return m;
}

/** Row identity used when comparing runs: every parameter except the timing. */
export function configKey(r: BenchResult): string {
  return [testLabel(r), r.n_gpu_layers, flashAttnLabel(r.flash_attn), r.type_k, r.type_v, r.n_threads, r.n_batch, r.n_ubatch].join('|');
}

export interface CompareRow {
  key: string;
  test: string;
  kind: TestKind;
  config: string;
  /** One entry per compared run (null = run has no such row). */
  values: (number | null)[];
}

/** Aligns rows across runs by config, sorted by test kind then label. */
export function compareRuns(runs: BenchmarkRun[]): CompareRow[] {
  const rows = new Map<string, CompareRow>();
  runs.forEach((run, idx) => {
    for (const r of run.results) {
      const key = configKey(r);
      let row = rows.get(key);
      if (!row) {
        row = {
          key,
          test: testLabel(r),
          kind: testKind(r),
          config: `ngl ${r.n_gpu_layers} · fa ${flashAttnLabel(r.flash_attn)} · ${r.type_k}/${r.type_v}`,
          values: runs.map(() => null),
        };
        rows.set(key, row);
      }
      row.values[idx] = r.avg_ts;
    }
  });
  const order: Record<TestKind, number> = { pp: 0, tg: 1, pg: 2 };
  return [...rows.values()].sort((a, b) => order[a.kind] - order[b.kind] || a.test.localeCompare(b.test, 'en', { numeric: true }) || a.config.localeCompare(b.config));
}

/** Percentage difference of `value` vs `base` (null when not computable). */
export function pctDiff(value: number | null, base: number | null): number | null {
  if (value === null || base === null || base === 0) return null;
  return ((value - base) / base) * 100;
}

/** Markdown table for a run (GitHub style, same shape as llama-bench's md output). */
export function runToMarkdown(run: Pick<BenchmarkRun, 'model_name' | 'results'> & { request?: { concurrent_servers?: number | null } }): string {
  const lines: string[] = [];
  const first = run.results[0];
  lines.push(`### ${run.model_name}`);
  if (first) {
    const build = first.build_number != null ? `${first.build_commit} (${first.build_number})` : first.build_commit;
    lines.push('');
    lines.push(`- model: ${first.model_type} · ${(first.model_size / 1024 ** 3).toFixed(2)} GiB`);
    lines.push(`- backends: ${first.backends}${first.gpu_info ? ` · ${first.gpu_info}` : ''}`);
    lines.push(`- build: ${build}`);
  }
  const busy = concurrentServers(run);
  if (busy > 0) lines.push(`- measured while ${busy} server(s) were running (numbers are lower than on an idle GPU)`);
  lines.push('');
  lines.push('| test | ngl | fa | type_k | type_v | threads | t/s |');
  lines.push('| --- | --: | --: | --- | --- | --: | --: |');
  for (const r of run.results) {
    lines.push(
      `| ${testLabel(r)} | ${r.n_gpu_layers} | ${flashAttnLabel(r.flash_attn)} | ${r.type_k} | ${r.type_v} | ${r.n_threads} | ${r.avg_ts.toFixed(2)} ± ${r.stddev_ts.toFixed(2)} |`,
    );
  }
  return lines.join('\n');
}
