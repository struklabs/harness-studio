import { tr, en, type Dictionary } from './locales';

export type Language = 'tr' | 'en';
export type TKey = keyof Dictionary;

const dictionaries: Record<Language, Record<TKey, string>> = { tr, en };

// Reactive: any template or $derived that calls t() re-renders when the language changes.
let current = $state<Language>('tr');

export function setLanguage(lang: Language) {
  current = lang;
  if (typeof document !== 'undefined') {
    document.documentElement.lang = lang;
  }
  // The start screen in app.html reads it before the app loads.
  try {
    localStorage.setItem('harness_lang', lang);
  } catch {
    // storage unavailable
  }
}

export function getLanguage(): Language {
  return current;
}

const isMac =
  typeof navigator !== 'undefined' && /Mac/i.test(`${navigator.userAgent} ${navigator.platform}`);

/** macOS wording: a `<key>_mac` text wins ("menu bar" for the tray), and "Ctrl+" shortcuts read "⌘". */
function platformText(key: TKey): string | undefined {
  if (!isMac) return dictionaries[current]?.[key] ?? en[key];
  const mac = `${key}_mac` as TKey;
  const text = dictionaries[current]?.[mac] ?? dictionaries[current]?.[key] ?? en[mac] ?? en[key];
  return text?.replace(/\bCtrl\+/g, '⌘');
}

/** Translates `key`, substituting `{name}` placeholders from `params`. Falls back to English, then the key. */
export function t(key: TKey, params?: Record<string, string | number>): string {
  const text = platformText(key) ?? key;
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (match, name) => (name in params ? String(params[name]) : match));
}

/** The text of `key` in every language (search keywords that work whatever the UI language is). */
export function inAllLanguages(key: TKey): string[] {
  const mac = `${key}_mac` as TKey;
  return (Object.keys(dictionaries) as Language[]).flatMap((l) =>
    [dictionaries[l][key] ?? key, ...(dictionaries[l][mac] ? [dictionaries[l][mac]] : [])]
  );
}

/** Locale for number/date formatting. */
export function locale(): string {
  return current === 'tr' ? 'tr-TR' : 'en-US';
}
