// Backend error strings are i18n keys (harness_core::error_key):
//   `key`, `key: detail` (older form, one free parameter) or `key {"name":"value",...}`.
// This module turns them into localized text. Parameters carry technical detail (paths, OS
// error text) untranslated; a parameter that is itself a keyed error (a nested cause) is
// translated too.
//
// `installBackendErrors()` wraps Tauri's invoke so a rejected command throws a `BackendError`
// whose `message` / `toString()` is the localized text: components that show `String(e)` or
// `e.message` get the translation without knowing about keys. `errorCode(e)` gives the key for
// code that branches on it.
import { en } from '$lib/locales';
import { t, type TKey } from '$lib/i18n.svelte';

export type ErrorParams = Record<string, string | number>;

const KEY_RE = /^[a-z0-9_]+(\.[a-z0-9_]+)+$/;

function isKnown(key: string): key is TKey {
  return Object.prototype.hasOwnProperty.call(en, key);
}

/** Key and parameters of an error string; null for free text. */
export function parseError(text: string): { key: string; params: ErrorParams } | null {
  const space = text.indexOf(' ');
  if (space > 0 && text[space + 1] === '{' && KEY_RE.test(text.slice(0, space))) {
    try {
      const obj = JSON.parse(text.slice(space + 1));
      if (obj && typeof obj === 'object' && !Array.isArray(obj)) {
        const params: ErrorParams = {};
        for (const [k, v] of Object.entries(obj)) {
          params[k] = typeof v === 'number' ? v : typeof v === 'string' ? v : JSON.stringify(v);
        }
        return { key: text.slice(0, space), params };
      }
    } catch {
      // not the JSON form
    }
  }
  const colon = text.indexOf(': ');
  const key = colon >= 0 ? text.slice(0, colon) : text;
  if (!KEY_RE.test(key)) return null;
  return { key, params: colon >= 0 ? { detail: text.slice(colon + 2) } : {} };
}

/** Raw text of an error value (a `BackendError` keeps the backend string). */
export function rawError(e: unknown): string {
  return rawText(e);
}

function rawText(e: unknown): string {
  if (e instanceof BackendError) return e.raw;
  if (e instanceof Error) return e.message;
  return String(e ?? '');
}

/** The error key of an error value, or null for free text. */
export function errorCode(e: unknown): string | null {
  return parseError(rawText(e))?.key ?? null;
}

/** Localized text of a backend error string / error value. */
export function errorText(e: unknown, depth = 0): string {
  const text = rawText(e);
  const parsed = parseError(text);
  if (!parsed || !isKnown(parsed.key)) return depth > 3 ? text : replaceEmbeddedKeys(text);
  const params: ErrorParams = {};
  for (const [k, v] of Object.entries(parsed.params)) {
    params[k] = typeof v === 'string' && depth < 3 ? errorText(v, depth + 1) : v;
  }
  const template = t(parsed.key);
  let out = t(parsed.key, params);
  // Older `key: detail` strings: the detail follows the text when the template has no slot.
  if (params.detail !== undefined && params.detail !== '' && !template.includes('{detail}')) {
    out = `${out.replace(/[.:]\s*$/, '')}: ${params.detail}`;
  }
  return out;
}

/** Keys inside a longer free text (`... failed: models.in_use`) replaced with their text. */
function replaceEmbeddedKeys(text: string): string {
  return text.replace(/\b[a-z0-9_]+(?:\.[a-z0-9_]+)+\b/g, (m) => (isKnown(m) ? t(m) : m));
}

/** A rejected backend command. `message` and `toString()` are the localized text. */
export class BackendError extends Error {
  readonly raw: string;
  constructor(raw: string) {
    super();
    this.raw = raw;
    this.name = 'BackendError';
  }
  get code(): string | null {
    return parseError(this.raw)?.key ?? null;
  }
  // A getter (not the field Error sets) keeps the text in the current UI language.
  override get message(): string {
    return errorText(this.raw);
  }
  override toString(): string {
    return this.message;
  }
}
