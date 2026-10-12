import { locale as appLocale } from '#lib/i18n.svelte.js';

export function formatTokens(n: number | null | undefined, locale: string = appLocale()): string {
  if (n == null || !Number.isFinite(n) || n < 0) return '0';
  
  if (n < 1000) {
    return new Intl.NumberFormat(locale).format(n);
  }
  
  return new Intl.NumberFormat(locale, {
    notation: 'compact',
    compactDisplay: 'short',
    maximumFractionDigits: 1
  }).format(n);
}

export function formatTokenNumber(n: number | null | undefined, locale: string = appLocale()): string {
  if (n == null || !Number.isFinite(n) || n < 0) return '0';
  return new Intl.NumberFormat(locale).format(n);
}
