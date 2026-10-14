<script lang="ts">
  import * as ToggleGroup from '#lib/components/ui/toggle-group/index.js';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { t } from '#lib/i18n.svelte.js';
</script>

<ToggleGroup.Root type="single" size="sm" aria-label={t('uiLevel.label')} disabled={uiLevel.busy}
  value={uiLevel.level} onValueChange={value => { if (value === 'simple' || value === 'advanced') void uiLevel.set(value); }}>
  <ToggleGroup.Item value="simple" class="h-7 flex-1 px-2 text-xs">{t('uiLevel.simple')}</ToggleGroup.Item>
  <ToggleGroup.Item value="advanced" class="h-7 flex-1 px-2 text-xs">{t('uiLevel.advanced')}</ToggleGroup.Item>
</ToggleGroup.Root>
