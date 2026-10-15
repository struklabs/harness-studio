<script lang="ts">
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import AdvancedOnly from '#lib/components/common/AdvancedOnly.svelte';
  import SectionLoading from '#lib/components/common/SectionLoading.svelte';
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  import type { Snippet } from 'svelte';
  import type { HardwareFitResult, LocalModel, ModelDefinition } from '#lib/types.js';
  import { t } from '#lib/i18n.svelte.js';
  import { RefreshCw, FolderOpen, Download, CircleHelp, MoreHorizontal, Plus, Copy, Pencil, Trash2, FileDown, FileUp } from '@lucide/svelte';
  import * as DropdownMenu from '#lib/components/ui/dropdown-menu/index.js';
  import { buttonVariants } from '#lib/components/ui/button/index.js';
  import { cn } from '#lib/utils.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Switch } from '#lib/components/ui/switch/index.js';
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import * as Empty from '#lib/components/ui/empty/index.js';
  import SectionLabel from './parts/SectionLabel.svelte';
  import DefinitionPicker from './model/DefinitionPicker.svelte';
  import type { DefinitionAction } from '#lib/stores/domain.svelte.js';
  import ModelDetails from './model/ModelDetails.svelte';
  import RouterInfo from './model/RouterInfo.svelte';

  interface Props {
    models: LocalModel[];
    selectedModelId: string;
    selectedModel: LocalModel | undefined;
    isScanning: boolean;
    routerMode: boolean;
    modelsDir: string;
    fitResult: HardwareFitResult | null;
    recommendedNgl: number | null;
    gpuLayers: number;
    onSelectModel: (id: string) => void;
    onRefresh: () => void;
    onGoToHub: () => void;
    onOpenTour: () => void;
    onApplyRecommendedNgl: () => void;
    onOpenMigration: () => void;
    onOpenImport: () => void;
    /** Removes a linked (Ollama / LM Studio) model from the list; rejects with the backend error. */
    onUnlink: (modelId: string) => Promise<void>;
    definitions: ModelDefinition[];
    selectedDefinitionId: string | null;
    onSelectDefinition: (id: string) => void;
    /** Definition menu: new / duplicate / edit / rename / export / import / delete. */
    onDefinitionAction: (action: DefinitionAction) => void;
    /** The model page was shown (or is showing); until then its detail reads are deferred. */
    active?: boolean;
  }

  let {
    models,
    selectedModelId,
    selectedModel,
    isScanning,
    routerMode = $bindable(),
    modelsDir,
    fitResult,
    recommendedNgl,
    gpuLayers,
    onSelectModel,
    onRefresh,
    onGoToHub,
    onOpenTour,
    onApplyRecommendedNgl,
    onOpenMigration,
    onOpenImport,
    onUnlink,
    definitions,
    selectedDefinitionId,
    onSelectDefinition,
    onDefinitionAction,
    active = true,
  }: Props = $props();

  const uid = $props.id();
</script>

{#snippet iconAction(label: string, onclick: () => void, icon: Snippet, disabled = false)}
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <Button {...props} variant="ghost" size="icon-xs" {onclick} {disabled} aria-label={label}>
          {@render icon()}
        </Button>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content>{label}</Tooltip.Content>
  </Tooltip.Root>
{/snippet}

{#snippet refreshIcon()}<RefreshCw class={isScanning ? 'animate-spin' : ''} />{/snippet}
{#snippet importIcon()}<Download />{/snippet}
{#snippet tourIcon()}<CircleHelp />{/snippet}

<section class="flex flex-col gap-3 border-b border-border pb-4" aria-labelledby="{uid}-title">
  <div class="flex items-center justify-between gap-2">
    <SectionLabel><span id="{uid}-title" title={t('dash.ms_title_tip')}>{t('dash.n_model')}</span></SectionLabel>
    <div class="flex items-center gap-0.5">
      <AdvancedOnly>
        <Tooltip.Root>
          <Tooltip.Trigger>
            {#snippet child({ props })}
              <label {...props} class="mr-1 flex cursor-pointer items-center gap-1.5 text-xs text-muted-foreground">
                <FieldHelp text={t('help.router')}><Switch size="sm" bind:checked={routerMode} aria-label={t('dash.ms_router_btn')} /></FieldHelp>
                {t('dash.ms_router_btn')}
              </label>
            {/snippet}
          </Tooltip.Trigger>
          <Tooltip.Content class="max-w-64">{t('dash.ms_router_title')}</Tooltip.Content>
        </Tooltip.Root>
      </AdvancedOnly>
      {@render iconAction(t('dash.ms_refresh_title'), onRefresh, refreshIcon, isScanning)}
      {@render iconAction(t('imp.open_btn'), onOpenImport, importIcon)}
      {@render iconAction(t('dash.ms_tour_title'), onOpenTour, tourIcon)}
    </div>
  </div>

  {#if isScanning}
    <SectionLoading label={t('sl.models_label')} shape="panel" minHeight="10rem" />
  {:else if models.length === 0}
    <Empty.Root class="gap-3 rounded-md border border-dashed border-border p-4">
      <Empty.Header>
        <Empty.Media variant="icon"><FolderOpen /></Empty.Media>
        <Empty.Title>{t('dash.ms_empty_title')}</Empty.Title>
        <Empty.Description>{t('dash.n_empty_desc')}</Empty.Description>
      </Empty.Header>
      <Empty.Content>
        <div class="flex flex-wrap justify-center gap-1.5">
          <Button size="sm" variant="outline" onclick={onGoToHub}>{t('dash.n_go_hub')}</Button>
          <Button size="sm" variant="ghost" onclick={onOpenImport}>{t('imp.open_btn_short')}</Button>
          <Button size="sm" variant="ghost" onclick={onRefresh} disabled={isScanning}>{t('dash.ms_rescan_btn')}</Button>
        </div>
      </Empty.Content>
    </Empty.Root>
  {:else if routerMode}
    <AdvancedOnly><RouterInfo {models} {modelsDir} /></AdvancedOnly>
    {#if !uiLevel.advanced}
      <div class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground" role="status">
        <span>{t('dash.ms_router_badge')}</span>
        <Button variant="ghost" size="xs" disabled={uiLevel.busy} onclick={() => uiLevel.set('advanced')}>{t('dash.show_advanced')}</Button>
      </div>
    {/if}
  {:else}
    <div class="flex min-w-0 items-start gap-1">
      <div class="min-w-0 flex-1">
        <DefinitionPicker
          {models}
          {definitions}
          {selectedModelId}
          {selectedDefinitionId}
          {onSelectDefinition}
          onSelectFile={onSelectModel}
        />
      </div>
      <AdvancedOnly>
        <DropdownMenu.Root>
          <DropdownMenu.Trigger class={cn(buttonVariants({ variant: 'ghost', size: 'icon-sm' }), 'mt-1 shrink-0')} aria-label={t('def.menu')}>
            <MoreHorizontal />
          </DropdownMenu.Trigger>
          <DropdownMenu.Content align="end" class="w-56">
            <DropdownMenu.Label class="text-xs text-muted-foreground">{t('def.menu')}</DropdownMenu.Label>
            <DropdownMenu.Item disabled={!selectedModel} onclick={() => onDefinitionAction('new')}><Plus />{t('def.action_new')}</DropdownMenu.Item>
            <DropdownMenu.Item disabled={!selectedDefinitionId} onclick={() => onDefinitionAction('duplicate')}><Copy />{t('def.action_duplicate')}</DropdownMenu.Item>
            <DropdownMenu.Item disabled={!selectedDefinitionId} onclick={() => onDefinitionAction('rename')}><Pencil />{t('def.action_rename')}</DropdownMenu.Item>
            <DropdownMenu.Item disabled={!selectedDefinitionId} onclick={() => onDefinitionAction('edit')}><Pencil />{t('def.action_edit')}</DropdownMenu.Item>
            <DropdownMenu.Separator />
            <DropdownMenu.Item disabled={!selectedDefinitionId} onclick={() => onDefinitionAction('export')}><FileDown />{t('def.action_export')}</DropdownMenu.Item>
            <DropdownMenu.Item onclick={() => onDefinitionAction('import')}><FileUp />{t('def.action_import')}</DropdownMenu.Item>
            <DropdownMenu.Item onclick={() => onDefinitionAction('import_modelfile')}><FileUp />{t('mf.action')}</DropdownMenu.Item>
            <DropdownMenu.Item onclick={() => onDefinitionAction('import_ollama')}><FileUp />{t('mf.action_ollama')}</DropdownMenu.Item>
            <DropdownMenu.Separator />
            <DropdownMenu.Item disabled={!selectedDefinitionId} class="text-destructive data-highlighted:text-destructive" onclick={() => onDefinitionAction('delete')}>
              <Trash2 />{t('def.action_delete')}
            </DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Root>
      </AdvancedOnly>
    </div>
    {#if selectedModel}
      <ModelDetails
        model={selectedModel}
        {fitResult}
        {recommendedNgl}
        {gpuLayers}
        {onApplyRecommendedNgl}
        {onOpenMigration}
        {onUnlink}
        {active}
      />
    {/if}
  {/if}
</section>
