<script lang="ts">
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  // Dashboard model selector: model definitions grouped under their GGUF file. A file without
  // any definition can still be picked (it launches with its file stem as model id).
  import type { LocalModel, ModelDefinition, ModelKind } from '#lib/types.js';
  import { formatBytes } from '#lib/format.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { definitionFile, samePath } from '#lib/stores/domain.svelte.js';
  import * as Select from '#lib/components/ui/select/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';

  interface Props {
    models: LocalModel[];
    definitions: ModelDefinition[];
    selectedModelId: string;
    selectedDefinitionId: string | null;
    onSelectDefinition: (id: string) => void;
    onSelectFile: (modelId: string) => void;
  }

  let { models, definitions, selectedModelId, selectedDefinitionId, onSelectDefinition, onSelectFile }: Props = $props();

  const KIND: Record<ModelKind, TKey> = {
    llm: 'def.kind_llm',
    embedding: 'def.kind_embedding',
    reranker: 'def.kind_reranker',
    asr: 'lib.kind_asr',
    image: 'lib.kind_image',
    tts: 'lib.kind_tts',
    mmproj: 'def.kind_mmproj',
    adapter: 'def.kind_adapter',
    unknown: 'def.kind_unknown',
  };

  /** Files that can be served on their own, each with its definitions. */
  let groups = $derived(
    models
      .filter((m) => !m.kind || !['mmproj', 'adapter'].includes(m.kind))
      .map((m) => ({
        model: m,
        defs: definitions
          .filter((d) => samePath(definitionFile(d), m.path) || m.shard_paths.some((p) => samePath(definitionFile(d), p)))
          .sort((a, b) => a.alias.localeCompare(b.alias)),
      }))
  );

  let value = $derived(selectedDefinitionId ? `d:${selectedDefinitionId}` : selectedModelId ? `f:${selectedModelId}` : '');
  let selectedDef = $derived(definitions.find((d) => d.id === selectedDefinitionId));
  let selectedModel = $derived(models.find((m) => m.id === selectedModelId));

  function change(v: string) {
    if (v.startsWith('d:')) onSelectDefinition(v.slice(2));
    else if (v.startsWith('f:')) onSelectFile(v.slice(2));
  }
</script>

{#snippet kindBadge(m: LocalModel)}
  {#if m.outside_scan}
    <Badge variant="outline" class="shrink-0 text-xs">{t('dash.ms_outside_scan')}</Badge>
  {/if}
  {#if m.kind && m.kind !== 'llm'}
    <Badge variant="outline" class="shrink-0 text-xs">{t(KIND[m.kind])}</Badge>
  {/if}
{/snippet}

<FieldHelp text={t('help.model_definition')}><Select.Root type="single" {value} onValueChange={(v) => v && change(v)}>
  <Select.Trigger class="h-auto min-h-9 w-full min-w-0 py-1" aria-label={t('def.picker_label')}>
    {#if selectedModel}
      <span class="flex min-w-0 flex-1 flex-col items-start text-left">
        <span class="flex w-full min-w-0 items-center gap-2">
          <span class="min-w-0 truncate font-mono text-sm font-medium">{selectedDef?.alias ?? selectedModel.name}</span>
          {#if !selectedDef}<span class="shrink-0 text-xs text-muted-foreground">{t('def.no_definition')}</span>{/if}
          {@render kindBadge(selectedModel)}
        </span>
        <span class="w-full truncate text-xs text-muted-foreground">
          {selectedModel.filename} · {selectedModel.quantization} · {formatBytes(selectedModel.size_bytes)}
        </span>
      </span>
    {:else}
      <span class="text-muted-foreground">{t('dash.ms_pick_placeholder')}</span>
    {/if}
  </Select.Trigger>
  <!-- As tall as the window allows (the default cap showed only about five files). -->
  <Select.Content class="max-h-[min(40rem,var(--bits-select-content-available-height))] w-max min-w-(--bits-select-anchor-width) max-w-[min(36rem,calc(100vw-2rem))]">
    {#each groups as g (g.model.id)}
      {@const only = g.defs.length === 1 ? g.defs[0] : null}
      <Select.Group>
        <!-- The file row itself is selectable: its only definition, else the file on its own. -->
        <Select.Item
          value={only ? `d:${only.id}` : `f:${g.model.id}`}
          label={only?.alias ?? g.model.name}
          class="mt-1 w-full *:[span]:last:min-w-0 *:[span]:last:shrink"
        >
          <span class="flex min-w-0 flex-1 flex-col">
            <span class="flex min-w-0 items-center gap-2 text-xs font-medium">
              <span class="min-w-0 truncate" title={g.model.path}>{g.model.name}</span>
              {@render kindBadge(g.model)}
              <span class="ml-auto shrink-0 font-num font-normal text-muted-foreground">
                {g.model.quantization} · {formatBytes(g.model.size_bytes)}
              </span>
            </span>
            {#if only}
              <span class="flex min-w-0 items-center gap-2">
                <span class="min-w-0 truncate font-mono text-xs">{only.alias}</span>
                {#if only.description}<span class="min-w-0 truncate text-xs text-muted-foreground">{only.description}</span>{/if}
              </span>
            {:else}
              <span class="min-w-0 truncate text-xs text-muted-foreground">{t('def.file_only_item')}</span>
            {/if}
          </span>
        </Select.Item>
        {#if g.defs.length > 1}
          {#each g.defs as d (d.id)}
            <Select.Item value={`d:${d.id}`} label={d.alias} class="w-full pl-5 *:[span]:last:min-w-0 *:[span]:last:shrink">
              <span class="flex min-w-0 flex-1 items-center gap-2">
                <span class="min-w-0 truncate font-mono text-xs">{d.alias}</span>
                {#if d.description}<span class="min-w-0 truncate text-xs text-muted-foreground">{d.description}</span>{/if}
              </span>
            </Select.Item>
          {/each}
        {/if}
      </Select.Group>
    {/each}
  </Select.Content>
</Select.Root></FieldHelp>
