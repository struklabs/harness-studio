import type { TrainingExportRequest, TrainingJob, TrainingQuant } from '#lib/training-types.js';

export const EXPORT_QUANTS: TrainingQuant[] = ['Q4_K_M', 'Q5_K_M', 'Q8_0', 'F16'];

/** Stable, deduplicated choices; reject an empty selection before starting an export. */
export function exportPlan(name: string, selected: TrainingQuant[]): TrainingExportRequest | null {
  const quants = EXPORT_QUANTS.filter(q => selected.includes(q));
  if (!quants.length) return null;
  return { name: name.trim() || null, gguf: true, quant: quants[0], quants, adapter_only: false };
}

export function comparePrompts(text: string): string[] | null {
  const prompts = text.split('\n').map(p => p.trim()).filter(Boolean);
  return prompts.length > 0 && prompts.length <= 8 && prompts.every(p => [...p].length <= 2000) ? prompts : null;
}

export function canCompare(job: TrainingJob): boolean {
  return job.status === 'done' || ((job.status === 'cancelled' || job.status === 'failed') && job.checkpoint_step !== null);
}

/** Freeze prompts before either side runs so edits cannot change the second side. */
export async function compareRun<T>(prompts: string[], run: (request: { prompts: string[] }) => Promise<T>): Promise<T> {
  return run({ prompts: [...prompts] });
}
