<script lang="ts">
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  // Dashboard model selector: model definitions grouped under their GGUF file. A file without
  // any definition can still be picked (it launches with its file stem as model id).
  import type { LocalModel, ModelDefinition, ModelKind } from '#lib/types.js';
  import { formatBytes } from '#lib/format.js';
  import { speechKindKey } from '#lib/libraryDefinitions.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { tick, onDestroy } from 'svelte';
  import * as Popover from '#lib/components/ui/popover/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Check, ChevronDown } from '@lucide/svelte';
  import {
    groupModels, flattenGroups, filterRows, rowOffsets, rowWindow, applyMeasurements,
    scrollToRow, navigationIndex, WINDOW_THRESHOLD, SEARCH_THRESHOLD, type PickerRow,
  } from './modelPicker.js';
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
    pipeline: 'lib.kind_pipeline',
    mmproj: 'def.kind_mmproj',
    adapter: 'def.kind_adapter',
    unknown: 'def.kind_unknown',
  };

  let groups = $derived(groupModels(models, definitions));
  let rows = $derived(flattenGroups(groups));
  let searchable = $derived(groups.length > SEARCH_THRESHOLD || rows.length > WINDOW_THRESHOLD);
  let windowed = $derived(rows.length > WINDOW_THRESHOLD);
  let open = $state(false);
  let query = $state('');
  let active = $state(-1);
  let scrollTop = $state(0);
  let scrollReady = false;
  let viewportHeight = $state(400);
  let list: HTMLDivElement | undefined = $state();
  let filter: HTMLInputElement | null = $state(null);
  let heights = $state<ReadonlyMap<string, number>>(new Map());
  const uid = $props.id();
  let filtered = $derived(filterRows(rows, query));
  let offsets = $derived(rowOffsets(filtered, heights));
  let visibleWindow = $derived(rowWindow(offsets, scrollTop, viewportHeight, windowed));
  // Never point aria-activedescendant at an unmounted option after a mouse scroll.
  let activeId = $derived(active >= visibleWindow.start && active < visibleWindow.end ? `${uid}-option-${active}` : undefined);

  const pendingHeights = new Map<string, number>();
  let measurementFrame: number | null = null;
  onDestroy(() => {
    if (measurementFrame !== null) cancelAnimationFrame(measurementFrame);
  });

  function measure(node: HTMLElement, value: string) {
    const observer = new ResizeObserver(([entry]) => {
      const height = entry.borderBoxSize[0]?.blockSize ?? node.getBoundingClientRect().height;
      if (height <= 0 || heights.get(value) === height) return;
      pendingHeights.set(value, height);
      if (measurementFrame !== null) return;
      // Batch cache/spacer writes outside ResizeObserver delivery to avoid resize loops.
      measurementFrame = requestAnimationFrame(async () => {
        measurementFrame = null;
        const measured = applyMeasurements(filtered, offsets, heights, pendingHeights, scrollTop);
        pendingHeights.clear();
        heights = measured.heights;
        if (measured.top !== scrollTop && list && open) {
          scrollTop = measured.top;
          await tick();
          if (list && open) list.scrollTop = scrollTop;
        }
      });
    });
    observer.observe(node);
    return { destroy: () => observer.disconnect() };
  }

  async function move(index: number) {
    active = index;
    scrollTop = scrollToRow(offsets, index, scrollTop, viewportHeight);
    // Update the spacers before setting scrollTop; the old DOM may be a short search result.
    await tick();
    if (!open || !list) return;
    list.scrollTop = scrollTop;
    scrollTop = list.scrollTop;
    list.focus({ preventScroll: true });
  }

  function prepareOpen(next: boolean) {
    scrollReady = false;
    if (!next) return;
    query = '';
    active = rows.findIndex((r) => r.value === value);
    scrollTop = active >= 0 ? offsets[active] : 0;
  }

  async function opened(event: Event) {
    event.preventDefault();
    await tick();
    if (!open || !list) return;
    viewportHeight = list.clientHeight;
    scrollTop = active >= 0 ? offsets[active] : 0;
    await tick();
    if (!open || !list) return;
    list.scrollTop = scrollTop;
    scrollTop = list.scrollTop;
    scrollReady = true;
    filter?.focus({ preventScroll: true });
  }

  function search() {
    active = -1;
    scrollTop = 0;
    if (list) list.scrollTop = 0;
  }

  function keydown(event: KeyboardEvent, fromFilter = false) {
    if (event.isComposing) return;
    // Escape bubbles to Popover, whose default close autofocus restores the trigger.
    if (event.key === 'Escape' || event.key === 'Tab') return;
    // Keep text editing keys (Home/End/ArrowUp) in the filter.
    if (fromFilter && event.key !== 'ArrowDown' && event.key !== 'Enter') return;
    const next = fromFilter && event.key === 'ArrowDown' && filtered.length
      ? Math.max(0, active)
      : navigationIndex(event.key, active, offsets, viewportHeight);
    if (next !== null) {
      event.preventDefault();
      event.stopPropagation();
      void move(next);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      event.stopPropagation();
      if (active >= 0 && filtered[active]) pick(filtered[active].value);
    }
  }

  function pick(next: string) {
    open = false;
    change(next);
  }

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
    <Badge variant="outline" class="shrink-0 text-xs">{t(m.kind === 'asr' ? speechKindKey(m) : KIND[m.kind])}</Badge>
  {/if}
{/snippet}

{#snippet triggerLabel()}
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
{/snippet}

{#snippet rowLabel(row: PickerRow)}
  {#if row.file}
    <span class="flex min-w-0 flex-1 flex-col">
      <span class="flex min-w-0 items-center gap-2 text-xs font-medium">
        <span class="min-w-0 truncate" title={row.model.path}>{row.model.name}</span>
        {@render kindBadge(row.model)}
        <span class="ml-auto shrink-0 font-num font-normal text-muted-foreground">
          {row.model.quantization} · {formatBytes(row.model.size_bytes)}
        </span>
      </span>
      {#if row.definition}
        <span class="flex min-w-0 items-center gap-2">
          <span class="min-w-0 truncate font-mono text-xs">{row.definition.alias}</span>
          {#if row.definition.description}<span class="min-w-0 truncate text-xs text-muted-foreground">{row.definition.description}</span>{/if}
        </span>
      {:else}
        <span class="min-w-0 truncate text-xs text-muted-foreground">{t('def.file_only_item')}</span>
      {/if}
    </span>
  {:else if row.definition}
    <span class="flex min-w-0 flex-1 items-center gap-2">
      <span class="min-w-0 truncate font-mono text-xs">{row.definition.alias}</span>
      {#if row.definition.description}<span class="min-w-0 truncate text-xs text-muted-foreground">{row.definition.description}</span>{/if}
    </span>
  {/if}
{/snippet}

<FieldHelp text={t('help.model_definition')}>
{#if searchable}
  <Popover.Root bind:open onOpenChange={prepareOpen}>
    <Popover.Trigger
      aria-label={t('def.picker_label')}
      onkeydown={(e) => {
        if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
          e.preventDefault();
          prepareOpen(true);
          open = true;
        }
      }}
      class="border-input dark:bg-input/30 dark:hover:bg-input/50 focus-visible:border-ring focus-visible:ring-ring/50 flex h-auto min-h-9 w-full min-w-0 items-center justify-between gap-1.5 rounded-lg border bg-transparent py-1 pr-2 pl-2.5 text-sm outline-none focus-visible:ring-3">
      {@render triggerLabel()}
      <ChevronDown class="size-4 shrink-0 text-muted-foreground" />
    </Popover.Trigger>
    <Popover.Content align="start" onOpenAutoFocus={opened}
      class={[windowed ? 'w-[min(36rem,calc(100vw-2rem))]' : 'w-max', 'min-w-(--bits-popover-anchor-width) max-w-[min(36rem,calc(100vw-2rem))] gap-0 overflow-hidden p-0']}>
      <div class="shrink-0 border-b border-border p-1">
        <Input bind:ref={filter} bind:value={query} oninput={search} onkeydown={(e) => keydown(e, true)}
            aria-label={t('def.picker_filter')} placeholder={t('def.picker_filter')} aria-controls="{uid}-listbox" />
      </div>
      <div bind:this={list} bind:clientHeight={viewportHeight}
        id="{uid}-listbox" role="listbox" tabindex="0" aria-label={t('def.picker_label')}
        aria-activedescendant={activeId} onkeydown={(e) => keydown(e)}
        onscroll={() => { if (scrollReady && list) scrollTop = list.scrollTop; }}
        class="max-h-[max(2rem,min(36rem,calc(var(--bits-popover-content-available-height)_-_3rem)))] overflow-y-auto overscroll-contain px-1 outline-none [overflow-anchor:none]">
        <div role="presentation" style:height="{visibleWindow.before}px"></div>
        {#each filtered.slice(visibleWindow.start, visibleWindow.end) as row, i (row.value)}
          {@const index = visibleWindow.start + i}
          <div use:measure={row.value} role="presentation" class:pt-1={row.file} class:pb-1={row.groupEnd}>
          <div id="{uid}-option-{index}" role="option" tabindex="-1"
            aria-selected={row.value === value} aria-setsize={windowed ? filtered.length : undefined} aria-posinset={windowed ? index + 1 : undefined}
            data-highlighted={active === index || undefined}
            class="relative flex w-full cursor-default items-center gap-1.5 rounded-md py-1 pr-8 text-sm select-none data-highlighted:bg-accent data-highlighted:text-accent-foreground"
            class:mt-1={row.file} class:pl-5={!row.file} class:pl-1.5={row.file}
            onkeydown={(e) => keydown(e)} onpointermove={() => { active = index; }} onclick={() => pick(row.value)}>
            <span class="flex min-w-0 flex-1 items-center gap-2 whitespace-nowrap">
              {@render rowLabel(row)}
            </span>
            {#if row.value === value}<Check class="pointer-events-none absolute right-2 size-4" />{/if}
          </div>
          </div>
        {/each}
        <div role="presentation" style:height="{visibleWindow.after}px"></div>
        {#if filtered.length === 0}<p role="status" class="px-3 py-4 text-xs text-muted-foreground">{t('def.picker_no_matches')}</p>{/if}
      </div>
    </Popover.Content>
  </Popover.Root>
{:else}
  <Select.Root type="single" {value} onValueChange={(v) => v && change(v)}>
    <Select.Trigger class="h-auto min-h-9 w-full min-w-0 py-1" aria-label={t('def.picker_label')}>
      {@render triggerLabel()}
    </Select.Trigger>
    <!-- As tall as the window allows (the default cap showed only about five files). -->
    <Select.Content class="max-h-[min(40rem,var(--bits-select-content-available-height))] w-max min-w-(--bits-select-anchor-width) max-w-[min(36rem,calc(100vw-2rem))]">
    {#each groups as group (group.model.id)}
      <Select.Group>
      {#each flattenGroups([group]) as row (row.value)}
      <Select.Item value={row.value} label={row.definition?.alias ?? row.model.name}
        class={row.file ? 'mt-1 w-full *:[span]:last:min-w-0 *:[span]:last:shrink' : 'w-full pl-5 *:[span]:last:min-w-0 *:[span]:last:shrink'}>
        {@render rowLabel(row)}
      </Select.Item>
      {/each}
      </Select.Group>
    {/each}
    </Select.Content>
  </Select.Root>

{/if}
</FieldHelp>
