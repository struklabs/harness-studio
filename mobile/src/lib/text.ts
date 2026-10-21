// Translations come from the desktop's dictionaries (`mobile.*` and shared namespaces such as
// `jobs.*`). Errors arrive as keys from the native side and the API helpers.
import { t, type TKey } from '#lib/i18n.svelte.js';

/** The text of an error key, or the key itself when it is not one of ours. */
export function errorText(key: string | null | undefined): string {
  if (!key) return '';
  const text = t(key as TKey);
  return text === key ? t('mobile.err.request') : text;
}

export function bytes(n: number | null | undefined): string {
  if (n === null || n === undefined) return '–';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v < 10 && i > 0 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
}

export function usd(n: number | null | undefined): string {
  if (n === null || n === undefined) return '–';
  return n < 0.01 ? `$${n.toPrecision(2)}` : `$${n.toFixed(2)}`;
}
