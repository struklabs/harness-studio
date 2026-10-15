import { t } from '#lib/i18n.svelte.js';
import { contextExceededError, gatewayBusyError, gatewayMemoryError, type GatewayBusy } from '#lib/gateway-error.js';
import { formatBytes } from '#lib/format.js';
import { describeErrorBody } from '#lib/components/playground/sse.js';
import { errorText } from '#lib/errors.js';

/** Localized sentence for a busy-model 503: which model is blocked, by whom, and when to retry. */
export function busyText(busy: GatewayBusy): string {
  const wait = busy.retryAfter != null && busy.retryAfter > 0
    ? t('pg.busy_wait_secs', { seconds: busy.retryAfter })
    : t('pg.busy_wait_finish');
  return t('pg.busy_model', { model: busy.model, busy: busy.busy.join(', '), wait });
}

/** Localized sentence for a not-enough-GPU-memory 503: the model, how much it needs and how much
 *  is free (`error.harness`), and what keeps it from fitting. Null for any other body. */
export function memoryText(body: string): string | null {
  const memory = gatewayMemoryError(body);
  if (!memory) return null;
  let harness: { need_bytes?: unknown; available_bytes?: unknown } | null = null;
  try {
    harness = JSON.parse(body)?.error?.harness ?? null;
  } catch {
    // plain text body
  }
  const need = typeof harness?.need_bytes === 'number' ? formatBytes(harness.need_bytes, 1) : null;
  const free = typeof harness?.available_bytes === 'number' ? formatBytes(harness.available_bytes, 1) : null;
  const parts = [need && free ? t('pg.memory_model', { model: memory.model, need, free }) : t('pg.memory_model_short', { model: memory.model })];
  if (/even with the idle models unloaded/i.test(memory.raw)) parts.push(t('pg.memory_no_fit'));
  if (memory.blockers.length) parts.push(t('pg.memory_blocking', { models: memory.blockers.join(', ') }));
  return parts.join(' ');
}

/** Error body text for the UI: a localized busy-model or GPU-memory sentence, otherwise `describeErrorBody`. */
export function describeError(body: string): string {
  const busy = gatewayBusyError(body);
  if (busy) return busyText(busy);
  const exceeded = contextExceededError(body);
  if (exceeded) return t('pg.context_exceeded', exceeded);
  return memoryText(body) ?? keyedText(body) ?? describeErrorBody(body);
}

/** A gateway error that carries a backend error key (`error.harness.error_key`), localized. */
function keyedText(body: string): string | null {
  let key: unknown;
  try {
    key = JSON.parse(body)?.error?.harness?.error_key;
  } catch {
    return null;
  }
  return typeof key === 'string' ? errorText(key) : null;
}
