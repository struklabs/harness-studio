<script lang="ts">
  import { t } from '#lib/i18n.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import HelpPopover from '../common/HelpPopover.svelte';
  import { QUALITIES, type Quality } from './generationParameters';
  let { onselect, busy = false, seconds = null }: { onselect?: (quality: Quality) => void; busy?: boolean; seconds?: number | null } = $props();
</script>
<div class="space-y-2 rounded-md border border-border p-3">
  <div class="flex flex-wrap items-center gap-2" role="group" aria-label={t('gen.presets')}>
    <span class="text-sm font-medium">{t('gen.presets')}</span><HelpPopover label={t('gen.presets')} text={t(onselect ? 'gen.presets_hint' : 'gen.fixed_quality')} />
    {#each QUALITIES as quality}<Button size="sm" variant="outline" disabled={busy || !onselect} onclick={() => onselect?.(quality)}>{t(`gen.${quality}`)}</Button>{/each}
  </div>
  <p class="text-xs text-muted-foreground">{t(onselect ? 'gen.presets_hint' : 'gen.fixed_quality')}</p>
  <p class="text-xs text-muted-foreground" role="status">{seconds === null ? t('gen.estimate_unavailable') : t('gen.estimate', { seconds: Math.ceil(seconds) })}</p>
</div>
