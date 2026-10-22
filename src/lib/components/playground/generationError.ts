import { t } from '#lib/i18n.svelte.js';
import { errorText, parseError, rawError } from '#lib/errors.js';
/** Retain translated structured errors, replace unlocalized HTTP/model internals with an actionable message. */
export function generationError(error: unknown): string {
  const raw = rawError(error);
  const parsed = parseError(raw);
  const localized = errorText(error);
  if (parsed && localized !== raw && !/HTTP \d|<html|Traceback/i.test(localized)) return localized;
  if (/memory|out.of.memory|oom|VRAM/i.test(raw)) return t('gen.error_memory');
  if (/fetch|network|connection|offline/i.test(raw)) return t('gen.error_connection');
  return t('gen.error_run');
}
