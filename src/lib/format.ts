import { locale } from '#lib/i18n.svelte.js';

/** Human-readable byte size (1024-based), e.g. 1536 → "1.5 KB" ("1,5 KB" in Turkish). */
export function formatBytes(bytes: number | null | undefined, decimals = 2): string {
  if (!bytes || bytes <= 0 || !Number.isFinite(bytes)) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), sizes.length - 1);
  if (i < sizes.length - 1 && bytes / Math.pow(k, i) >= 1000) i++;
  return `${(bytes / Math.pow(k, i)).toLocaleString(locale(), { maximumFractionDigits: decimals })} ${sizes[i]}`;
}

/** Readable model name: a file path id ("C:\\AI\\model-q4.gguf") becomes "model-q4". */
export function modelLabel(id: string): string {
  const base = id.split(/[\\/]/).pop() || id;
  return base.replace(/\.gguf$/i, '') || id;
}
