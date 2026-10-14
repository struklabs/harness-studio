<script lang="ts">
  import Field from '../dashboard/parts/Field.svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { errorText } from '#lib/errors.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import * as Select from '#lib/components/ui/select/index.js';
  import * as ToggleGroup from '#lib/components/ui/toggle-group/index.js';
  import { EMBED_NONE, resolveEmbedModel, type RagModels } from './rag';
  import type { PgSettings } from './types';
  let { settings = $bindable(), models, onEmbedChange, onGoToHub }: {
    settings: PgSettings; models: RagModels | null; onEmbedChange: () => void; onGoToHub: () => void;
  } = $props();
  let embedList = $derived(models?.embedding ?? []);
  let rerankList = $derived(models?.rerank ?? []);
  let effectiveEmbed = $derived(resolveEmbedModel(settings.ragEmbedModel, embedList));
  let embedValue = $derived(effectiveEmbed ?? EMBED_NONE);
  function setEmbed(v: string) { settings.ragEmbedModel = v; onEmbedChange(); }
</script>
<div class="flex min-w-0 flex-col gap-3">
  <Field label={t('doc.embed_label')} hint={t('doc.embed_desc')} note={embedList.length ? t('doc.embed_effect') : undefined}>
    {#if embedList.length === 0}
      <p class="m-0 text-xs text-muted-foreground">{t('doc.embed_none_desc')}</p>
      <Button variant="outline" size="sm" class="self-start" onclick={onGoToHub}>{t('doc.open_hub')}</Button>
    {:else}
      <Select.Root type="single" value={embedValue} onValueChange={(v) => v && setEmbed(v)}>
        <Select.Trigger class="w-full font-mono text-xs" aria-label={t('doc.embed_label')}>
          <span class="truncate">{effectiveEmbed ?? t('doc.embed_off')}</span>
        </Select.Trigger>
        <Select.Content>
          {#each embedList as m (m.alias)}
            <Select.Item value={m.alias} label={m.alias} class="font-mono text-xs">
              <span class="min-w-0 flex-1 truncate">{m.alias}</span>
              {#if m.dims}<span class="shrink-0 font-sans text-muted-foreground">{t('doc.dims', { n: m.dims })}</span>{/if}
            </Select.Item>
          {/each}
          <Select.Item value={EMBED_NONE} label={t('doc.embed_off')} class="text-xs">{t('doc.embed_off')}</Select.Item>
        </Select.Content>
      </Select.Root>
      {#if !models?.gateway}
        <p class="m-0 text-xs text-warning">{errorText('rag.gateway_required')}</p>
      {/if}
    {/if}
  </Field>

  <Field label={t('doc.rerank_label')} hint={t('doc.rerank_desc')} note={rerankList.length === 0 ? t('doc.rerank_none_desc') : t('doc.rerank_effect')}>
    <Select.Root
      type="single"
      value={settings.ragRerankModel || 'off'}
      disabled={rerankList.length === 0}
      onValueChange={(v) => (settings.ragRerankModel = v === 'off' ? '' : v)}
    >
      <Select.Trigger class="w-full font-mono text-xs" aria-label={t('doc.rerank_label')}>
        <span class="truncate">{settings.ragRerankModel || t('doc.rerank_off')}</span>
      </Select.Trigger>
      <Select.Content>
        <Select.Item value="off" label={t('doc.rerank_off')} class="text-xs">{t('doc.rerank_off')}</Select.Item>
        {#each rerankList as m (m.alias)}
          <Select.Item value={m.alias} label={m.alias} class="font-mono text-xs">{m.alias}</Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>
  </Field>

  <Field label={t('doc.topk_label')} hint={t('doc.topk_desc')}>
    <ToggleGroup.Root
      type="single"
      variant="outline"
      size="sm"
      value={String(settings.ragTopK)}
      onValueChange={(v) => {
        if (v) settings.ragTopK = Number(v);
      }}
      class="w-full"
      aria-label={t('doc.topk_label')}
    >
      {#each [3, 5, 8, 12] as k (k)}
        <ToggleGroup.Item value={String(k)} class="h-7 flex-1 font-num text-xs">{k}</ToggleGroup.Item>
      {/each}
    </ToggleGroup.Root>
  </Field>
</div>
