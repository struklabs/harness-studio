import { toast } from 'svelte-sonner';
import { t } from '#lib/i18n.svelte.js';
import { ACTION_TOAST_DURATION } from '#lib/toast.js';
import { firstNotice, redactDetail } from './toolRouting';

const warned = new Set<string>();

export function notifyAudioHistoryOmitted() {
  toast.warning(t('pg.audio_history_omitted'), { id: 'pg-audio-history-omitted' });
}

/** Concise localized warning, once per model; the redacted, bounded upstream detail is only a copy away. */
export function notifyRoutingFallback(error: string, model: unknown, withoutTools = false) {
  if (!firstNotice(warned, `${withoutTools ? 'audio:' : ''}${String(model ?? '')}`)) return;
  const detail = redactDetail(error);
  toast.warning(t(withoutTools ? 'w4.routing_fallback_audio' : 'w4.routing_fallback'), {
    description: detail ? redactDetail(detail, 160) : undefined,
    duration: detail ? ACTION_TOAST_DURATION : undefined,
    action: detail ? {
      label: t('w4.copy_details'),
      onClick: () => { void navigator.clipboard?.writeText(detail).then(() => toast.success(t('w4.copied')), () => {}); },
    } : undefined,
  });
}
