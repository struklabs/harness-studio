import { locale } from '#lib/i18n.svelte.js';

function byteParts(bytes: number | null | undefined): { value: number; unit: string } {
  if (!bytes || bytes <= 0 || !Number.isFinite(bytes)) return { value: 0, unit: 'B' };
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), sizes.length - 1);
  if (i < sizes.length - 1 && bytes / Math.pow(k, i) >= 1000) i++;
  return { value: bytes / Math.pow(k, i), unit: sizes[i] };
}

/** Human-readable byte size (1024-based), e.g. 1536 → "1.5 KB" ("1,5 KB" in Turkish). */
export function formatBytes(bytes: number | null | undefined, decimals = 2): string {
  const { value, unit } = byteParts(bytes);
  return `${value.toLocaleString(locale(), { maximumFractionDigits: decimals })} ${unit}`;
}

/** Fixed precision on both sides; omit the used unit only when it matches the total unit. */
export function formatBytesPair(used: number, total: number, decimals = 1): { used: string; rest: string } {
  const u = byteParts(used);
  const t = byteParts(total);
  // Whole bytes have no fraction ("0 B", not "0,0 B").
  const fmt = (p: { value: number; unit: string }) => {
    const digits = p.unit === 'B' ? 0 : decimals;
    return p.value.toLocaleString(locale(), { minimumFractionDigits: digits, maximumFractionDigits: digits });
  };
  return {
    used: `${fmt(u)}${u.unit === t.unit ? '' : ` ${u.unit}`}`,
    rest: ` / ${fmt(t)} ${t.unit}`,
  };
}

/** Single-string form of the shared byte pair. */
export function formatBytesPairText(used: number, total: number, decimals = 1): string {
  const pair = formatBytesPair(used, total, decimals);
  return pair.used + pair.rest;
}

/** Readable model name: a file path id ("C:\\AI\\model-q4.gguf") becomes "model-q4". */
export function modelLabel(id: string): string {
  const base = id.split(/[\\/]/).pop() || id;
  return base.replace(/\.gguf$/i, '') || id;
}
