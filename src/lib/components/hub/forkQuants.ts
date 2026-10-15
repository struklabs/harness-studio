/** Quant types only the PrismML llama.cpp fork can load (mirror of `FORK_TYPES` in runtimes/quant.rs). */
const PRISM_FORK_QUANT = /(?:^|[^A-Z0-9])(?:PQ2_0|PTQ1_0)(?![A-Z0-9])/i;

/** Whether a quant label or GGUF file name is a PrismML-fork type (the repo stays runnable). */
export function needsPrismFork(...names: (string | null | undefined)[]): boolean {
  return names.some((n) => !!n && PRISM_FORK_QUANT.test(n));
}
