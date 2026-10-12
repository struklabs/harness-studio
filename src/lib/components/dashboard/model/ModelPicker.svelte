<script lang="ts">
  import FieldHelp from '$lib/components/common/FieldHelp.svelte';
  import type { LocalModel } from '$lib/types';
  import { formatBytes } from '$lib/format';
  import { t } from '$lib/i18n.svelte';
  import { sourceLabel, type LocalModelExt } from '$lib/import-types';
  import * as Select from '$lib/components/ui/select';
  import { Badge } from '$lib/components/ui/badge';

  interface Props {
    models: LocalModel[];
    selectedModelId: string;
    onSelect: (id: string) => void;
  }

  let { models, selectedModelId, onSelect }: Props = $props();

  let selected = $derived(models.find((m) => m.id === selectedModelId));
  const sourceOf = (m: LocalModel) => (m as LocalModelExt).source ?? null;
</script>

{#snippet row(model: LocalModel)}
  {@const src = sourceOf(model)}
  <span class="flex min-w-0 flex-1 items-center gap-2">
    <span class="min-w-0 flex-1 truncate text-left text-sm font-medium">{model.name}</span>
    {#if src}
      <span class="shrink-0 text-xs text-muted-foreground">{sourceLabel(src)}</span>
    {/if}
    <Badge variant="outline" class="shrink-0 font-mono text-xs">{model.quantization}</Badge>
    <span class="w-14 shrink-0 text-right font-num text-xs text-muted-foreground">{formatBytes(model.size_bytes)}</span>
  </span>
{/snippet}

<FieldHelp text={t('help.model_file')}><Select.Root type="single" value={selectedModelId} onValueChange={(v) => v && onSelect(v)}>
  <Select.Trigger class="h-9 w-full min-w-0" aria-label={t('dash.ms_title_tip')}>
    {#if selected}
      {@render row(selected)}
    {:else}
      <span class="text-muted-foreground">{t('dash.ms_pick_placeholder')}</span>
    {/if}
  </Select.Trigger>
  <Select.Content class="max-h-72 w-max min-w-(--bits-select-anchor-width) max-w-[min(36rem,calc(100vw-2rem))]">
    {#each models as model (model.id)}
      <Select.Item value={model.id} label={model.name} class="w-full">
        {@render row(model)}
      </Select.Item>
    {/each}
  </Select.Content>
</Select.Root></FieldHelp>
