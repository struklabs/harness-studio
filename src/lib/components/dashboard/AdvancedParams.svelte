<script lang="ts">
  import AdvancedOnly from '#lib/components/common/AdvancedOnly.svelte';
  import type { LocalModel, SystemHardwareInfo } from '#lib/types.js';
  import { t } from '#lib/i18n.svelte.js';
  import { ListTree } from '@lucide/svelte';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { toast } from 'svelte-sonner';
  import { capsStore } from '#lib/stores/caps.svelte.js';
  import { toolsExposureRisk, type LaunchParamsForm } from './params';
  import Field from './parts/Field.svelte';
  import PerformanceSection from './advanced/PerformanceSection.svelte';
  import MemorySection from './advanced/MemorySection.svelte';
  import ModelExtrasSection from './advanced/ModelExtrasSection.svelte';
  import ToolsSection from './advanced/ToolsSection.svelte';
  import NetworkSection from './advanced/NetworkSection.svelte';
  import CatalogSection from './advanced/CatalogSection.svelte';
  import ServeModeSection from './advanced/ServeModeSection.svelte';
  import AllFlagsDialog from './AllFlagsDialog.svelte';

  interface Props {
    page: string;
    params: LaunchParamsForm;
    /** Candidates for the speculative-decoding draft model. */
    models: LocalModel[];
    selectedModelId: string;
    /** Host the server will bind to (for the tools/MCP exposure warning). */
    host: string;
    hardware?: SystemHardwareInfo | null;
    routerMode?: boolean;
    /** Server whose runtime the "all flags" explorer reads. */
    serverId: string;
    onGoToConnections: () => void;
  }

  let {
    page,
    params = $bindable(),
    models,
    selectedModelId,
    host,
    hardware = null,
    routerMode = false,
    serverId,
    onGoToConnections,
  }: Props = $props();

  const uid = $props.id();
  let showFlags = $state(false);

  let exposureRisk = $derived(toolsExposureRisk(host, params.api_key, params));

  function addFlag(flag: string) {
    const current = params.custom_args.trim();
    params.custom_args = current ? `${current} ${flag}` : flag;
    toast.success(t('caps.flags_added', { flag }));
  }
</script>

<section class="flex flex-col gap-2">
      <AdvancedOnly>
        {#if capsStore.error}
          <p class="pb-2 text-xs leading-snug text-muted-foreground" role="status">{t('caps.runtime_unknown')}</p>
        {/if}
      </AdvancedOnly>
<div hidden={page !== 'gpu'} class="flex flex-col gap-3">
      <PerformanceSection bind:params {hardware} />
</div>
<div hidden={page !== 'context'} class="flex flex-col gap-3">
      <AdvancedOnly><MemorySection bind:params {hardware} trainedContext={models.find((m) => m.id === selectedModelId)?.context_length ?? null} /></AdvancedOnly>
</div>
<div hidden={!['template', 'other'].includes(page)} class="flex flex-col gap-3">
      <ModelExtrasSection {page} bind:params {models} {selectedModelId} />
      <div hidden={page !== 'template'}><CatalogSection bind:params group="reasoning" /></div>
</div>
<div hidden={page !== 'other'} class="flex flex-col gap-3">
      {#if !routerMode}
        <ServeModeSection bind:params {models} {selectedModelId} />
      {/if}
      {#if routerMode}
        <CatalogSection bind:params group="router" />
      {/if}
</div>
<div hidden={page !== 'tools'} class="flex flex-col gap-3">
      <ToolsSection bind:params {onGoToConnections} />
</div>
<div hidden={page !== 'network'} class="flex flex-col gap-3">
      <NetworkSection bind:params {host} {exposureRisk} />
</div>
<div hidden={page !== 'other'} class="flex flex-col gap-3">
      <div class="pt-3">
        <Field help={t('help.args')} label={t('dash.adv_custom_args_label')} id="{uid}-args">
          {#snippet aside()}
            <Button variant="link" size="xs" class="h-auto px-0 text-xs" onclick={() => (showFlags = true)}>
              <ListTree class="size-3" />
              {t('caps.flags_open')}
            </Button>
          {/snippet}
          <Input id="{uid}-args" type="text" placeholder={t('dash.adv_custom_args_placeholder')} bind:value={params.custom_args} class="font-mono" />
        </Field>
      </div>


</div>
  <!-- Blocking / risky settings stay visible even while the section is collapsed. -->
  {#if exposureRisk}
    <div class="mb-1.5 rounded-md border border-warning/30 bg-warning/10 px-2.5 py-2 text-xs text-warning" role="alert">
      {t('dash.adv_tools_exposure_warning', { host })}
    </div>
  {/if}
</section>

{#if showFlags}
  <AllFlagsDialog {serverId} onAdd={addFlag} onClose={() => (showFlags = false)} />
{/if}
