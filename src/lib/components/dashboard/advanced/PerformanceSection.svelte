<script lang="ts">
  import { t } from '$lib/i18n.svelte';
  import type { SystemHardwareInfo } from '$lib/types';
  import { Input } from '$lib/components/ui/input';
  import { capsStore } from '$lib/stores/caps.svelte';
  import { app } from '$lib/stores/app.svelte';
  import type { LaunchParamsForm } from '../params';
  import Field from '../parts/Field.svelte';
  import NativeSelect from '../parts/NativeSelect.svelte';
  import AdvSection from './AdvSection.svelte';
  import SwitchRow from './SwitchRow.svelte';
  import CatalogFields from './CatalogFields.svelte';
  import FeatureBadge from './FeatureBadge.svelte';

  interface Props {
    params: LaunchParamsForm;
    hardware?: SystemHardwareInfo | null;
  }

  let { params = $bindable(), hardware = null }: Props = $props();

  const uid = $props.id();
  // With detected GPUs the main GPU is chosen in the key parameters; the number field is the fallback.
  let hasGpuList = $derived((hardware?.gpus?.length ?? 0) > 0);
  let splitTensor = $derived(!!capsStore.flag('--split-mode')?.enum_values.includes('tensor'));
  // Harness-level `load_timeout` (seconds; not a llama-server flag). Empty = the setting.
  let loadTimeout = $derived.by(() => {
    const v = params.catalog_params.load_timeout;
    return typeof v === 'number' ? v : null;
  });
  function setLoadTimeout(v: number) {
    if (Number.isFinite(v) && v > 0) params.catalog_params.load_timeout = Math.min(7200, Math.max(10, Math.round(v)));
    else delete params.catalog_params.load_timeout;
  }
</script>

<AdvSection open={true} title={t('dash.n_sec_performance')}>
  {#if capsStore.supports('threads')}
    <Field label={t('dash.lp_threads_label_short')} id="{uid}-threads" help={t('dash.lp_help_threads')} flag="-t">
      <Input id="{uid}-threads" type="number" min="1" placeholder={t('dash.lp_threads_placeholder')} bind:value={params.threads} class="font-num" />
    </Field>
  {/if}

  <!-- Concurrent requests (-np) live in the key parameters (LaunchParams) with their recommendation. -->
  <div class="grid grid-cols-2 gap-2">
    {#if capsStore.supports('logical_batch')}
      <Field help={t('help.batch')} label={t('dash.adv_logical_batch_label')} id="{uid}-lb" flag="-b">
        <Input id="{uid}-lb" type="number" step="256" placeholder="2048" bind:value={params.logical_batch} class="px-2 font-num" />
      </Field>
    {/if}
    {#if capsStore.supports('physical_batch')}
      <Field help={t('help.ubatch')} label={t('dash.adv_physical_batch_label')} id="{uid}-pb" flag="-ub">
        <Input id="{uid}-pb" type="number" step="128" placeholder="512" bind:value={params.physical_batch} class="px-2 font-num" />
      </Field>
    {/if}
  </div>

  {#if capsStore.supports('flash_attn')}
    <SwitchRow
      id="{uid}-flash"
      label={t('dash.adv_flash_attn_label')}
      hint={t('dash.flash_attn_hint')}
      note={t('dash.adv_flash_attn_sub')}
      feature="flash_attn"
      bind:checked={params.flash_attn}
      flag="--flash-attn"
    />
  {/if}

  <div class="grid grid-cols-2 gap-2">
    {#if capsStore.supports('split_mode')}
      <Field help={t('help.split')} label={t('dash.adv_splitmode_label')} id="{uid}-split" flag="--split-mode">
        {#snippet aside()}
          {#if params.split_mode === 'tensor'}<FeatureBadge feature="split_mode_tensor" />{/if}
        {/snippet}
        <NativeSelect id="{uid}-split" bind:value={params.split_mode}>
          <option value="">{t('dash.adv_splitmode_default')}</option>
          <option value="layer">{t('dash.adv_splitmode_layer')}</option>
          <option value="row">{t('dash.adv_splitmode_row')}</option>
          <option value="none">{t('dash.adv_splitmode_none')}</option>
          {#if splitTensor || params.split_mode === 'tensor'}
            <option value="tensor">tensor</option>
          {/if}
        </NativeSelect>
      </Field>
    {/if}
    {#if !hasGpuList && capsStore.supports('main_gpu')}
      <Field help={t('help.main_gpu')} label={t('dash.adv_maingpu_label')} id="{uid}-maingpu" flag="--main-gpu">
        <Input id="{uid}-maingpu" type="number" min="0" placeholder="0" bind:value={params.main_gpu} class="font-num" />
      </Field>
    {/if}
    {#if capsStore.supports('tensor_split')}
      <Field help={t('help.tensor_split')} label={t('dash.adv_tensorsplit_label')} id="{uid}-ts" class={hasGpuList ? '' : 'col-span-2'} flag="-ts">
        {#snippet aside()}<FeatureBadge feature="tensor_split" />{/snippet}
        <Input id="{uid}-ts" type="text" placeholder={t('dash.adv_tensorsplit_placeholder')} bind:value={params.tensor_split} class="font-num" />
      </Field>
    {/if}
  </div>

  <Field label={t('dash.load_timeout_label')} id="{uid}-load-timeout" help={t('dash.load_timeout_help')}>
    <Input
      id="{uid}-load-timeout"
      type="number"
      min="10"
      max="7200"
      step="30"
      placeholder={t('dash.load_timeout_placeholder', { seconds: app.settings?.server.load_timeout_secs ?? 300 })}
      value={loadTimeout}
      onchange={(e) => setLoadTimeout(e.currentTarget.valueAsNumber)}
      class="font-num"
    />
  </Field>

  <CatalogFields bind:params group="performance" />
</AdvSection>
