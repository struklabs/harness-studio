import { invoke } from '#lib/ipc.js';
import { defaultLoadFit, estimateFit } from '#lib/fit.js';
import type { FitResult, HfModelSearchResult, HfModelVariant, ImageBundlePlan } from '#lib/types.js';
import { bestRepoFit, hasCompleteSize, repoUnavailable, type HubFit } from './hardwareRanking';
import { isCompanionVariant, pickProjectorVariant } from './recommend';

const TTL = 5 * 60_000;
const MAX = 120;
const metadata = new Map<string, { at: number; value: Promise<HfModelVariant[]> }>();

/** Shared by cards and drawer. Only Hub tree/config metadata; never downloads weights. */
export function fetchVariants(repoId: string): Promise<HfModelVariant[]> {
  const hit = metadata.get(repoId);
  if (hit && Date.now() - hit.at < TTL) return hit.value;
  const entry = { at: Date.now(), value: invoke<HfModelVariant[]>('get_hf_model_variants', { repoId }) };
  metadata.delete(repoId);
  metadata.set(repoId, entry);
  while (metadata.size > MAX) metadata.delete(metadata.keys().next().value!);
  void entry.value.catch(() => { if (metadata.get(repoId) === entry) metadata.delete(repoId); });
  return entry.value;
}

// Bound the number of repository tree requests and their fit batches while scrolling.
let active = 0;
const waiting: (() => void)[] = [];
const estimates = new Map<string, { at: number; value: Promise<HubFit> }>();
export function cachedRepoFit(repo: HfModelSearchResult, context: number, hardwareKey: string): Promise<HubFit> {
  const key = JSON.stringify([repo.id, repo.last_modified, repo.compatibility, context, hardwareKey]);
  const hit = estimates.get(key);
  if (hit && Date.now() - hit.at < TTL) return hit.value;
  const value = estimateRepoFit(repo, context);
  estimates.delete(key);
  estimates.set(key, { at: Date.now(), value });
  while (estimates.size > MAX) estimates.delete(estimates.keys().next().value!);
  return value;
}

async function estimateRepoFit(repo: HfModelSearchResult, context: number): Promise<HubFit> {
  if (repoUnavailable(repo)) return { tier: 'unavailable' };
  if (active >= 3) await new Promise<void>(resolve => waiting.push(resolve));
  else active++;
  try {
    const runtime = 'runtime' in repo.compatibility ? repo.compatibility.runtime : '';
    // These IPC estimators can route remote metadata without local headers. Other engines
    // need a local config/header: report unknown instead of applying the LLM estimator.
    if (!['llama.cpp', 'whisper.cpp', 'onnxruntime', 'stable-diffusion.cpp'].includes(runtime) || repo.adapter) return { tier: 'unknown' };
    const variants = await fetchVariants(repo.id);
    const sized = variants.filter(v => !v.not_runnable && !isCompanionVariant(v) && hasCompleteSize(v));
    const pairs = await Promise.all(sized.map(async v => {
      let result: FitResult;
      if (runtime === 'stable-diffusion.cpp') {
        const plan = await invoke<ImageBundlePlan | null>('get_image_bundle_plan', { file: v.shards[0]?.filename ?? '', repo: null });
        // Unresolved split bundles need their companion sizes before a reliable verdict.
        if (!plan && variants.some(isCompanionVariant)) return null;
        const size = v.total_size_bytes + (plan?.components ?? []).reduce((n, c) => n + c.size, 0);
        [result] = await invoke<FitResult[]>('estimate_image_fit', { sizes: [size] });
      } else {
        const file = v.shards.find(s => /\.(onnx|bin|gguf)$/i.test(s.filename))?.filename;
        if (runtime === 'onnxruntime' && !file?.endsWith('.onnx')) return null;
        const projector = pickProjectorVariant(variants, v);
        result = await defaultLoadFit({
          model_path: runtime === 'llama.cpp' ? null : file,
          model_size_bytes: v.total_size_bytes,
          mmproj_size_bytes: projector?.total_size_bytes ?? 0,
          context_size: context,
          gpu_layers: 999,
        });
        // Whisper is all-GPU or all-CPU. A failed GPU estimate can still run in RAM.
        if (runtime === 'whisper.cpp' && result.category === 'oom_risk') {
          const cpu = await estimateFit({ model_path: file, model_size_bytes: v.total_size_bytes, context_size: context, gpu_layers: 0 });
          if (cpu.category !== 'oom_risk') result = cpu;
        }
      }
      return [v.name, result] as const;
    }));
    return bestRepoFit(variants, new Map(pairs.filter(p => p !== null)));
  } catch {
    return { tier: 'unknown' };
  } finally {
    const next = waiting.shift();
    if (next) next();
    else active--;
  }
}
