<script lang="ts">
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { t } from '#lib/i18n.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  let { level, discoverable = false }: { level?: 'advanced'; discoverable?: boolean } = $props();
</script>

{#if discoverable && !uiLevel.advanced}
  <div class="min-w-0 text-xs text-muted-foreground">
    <Button variant="ghost" size="sm" class="h-auto w-full justify-start whitespace-normal px-1 py-1.5 text-left text-xs" title={t('uiLevel.more_hint')} disabled={uiLevel.busy} onclick={() => uiLevel.set('advanced')}>{t('uiLevel.more')}</Button>
  </div>
{:else if level === 'advanced' && !uiLevel.advanced}
  <div role="note" class="flex shrink-0 flex-wrap items-center gap-x-2 border-b border-border px-4 py-1 text-xs text-muted-foreground">
    <span>{t('uiLevel.page_notice')}</span>
    <Button variant="ghost" size="sm" class="h-7 text-xs" disabled={uiLevel.busy} onclick={() => uiLevel.set('advanced')}>{t('uiLevel.switch_advanced')}</Button>
  </div>
{/if}
