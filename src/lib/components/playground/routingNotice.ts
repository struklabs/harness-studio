import { toast } from 'svelte-sonner';
import { t } from '#lib/i18n.svelte.js';
import { ACTION_TOAST_DURATION } from '#lib/toast.js';
import { redactDetail } from './toolRouting';

/** Concise localized warning; the redacted, bounded upstream detail is only a copy away. */
export function notifyRoutingFallback(error: string) {
  const detail = redactDetail(error);
  toast.warning(t('w4.routing_fallback'), {
    description: detail ? redactDetail(detail, 160) : undefined,
    duration: detail ? ACTION_TOAST_DURATION : undefined,
    action: detail ? {
      label: t('w4.copy_details'),
      onClick: () => { void navigator.clipboard?.writeText(detail).then(() => toast.success(t('w4.copied')), () => {}); },
    } : undefined,
  });
}
