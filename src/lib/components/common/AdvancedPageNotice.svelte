<script lang="ts">
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { t } from '#lib/i18n.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  let { level }: { level?: 'advanced' } = $props();
</script>

{#if level === 'advanced' && !uiLevel.advanced}
  <div role="note" class="flex shrink-0 flex-wrap items-center gap-x-2 border-b border-border px-4 py-1 text-xs text-muted-foreground">
    <span>{t('uiLevel.page_notice')}</span>
    <Button variant="ghost" size="sm" class="h-7 text-xs" disabled={uiLevel.busy} onclick={() => uiLevel.set('advanced')}>{t('uiLevel.switch_advanced')}</Button>
  </div>
{/if}
