<script lang="ts">
  import { t, type TKey } from '$lib/i18n.svelte';
  import { cn } from '$lib/utils';
  import type { DeviceReport, RuntimeDeviceLive, SlotAdvice, SystemHardwareInfo } from '$lib/types';
  import { invoke } from '$lib/ipc';
  import { app } from '$lib/stores/app.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Slider } from '$lib/components/ui/slider';
  import { Badge } from '$lib/components/ui/badge';
  import { Checkbox } from '$lib/components/ui/checkbox';
  import * as ToggleGroup from '$lib/components/ui/toggle-group';
  import type { LaunchParamsForm } from './params';
  import Field from './parts/Field.svelte';
  import NativeSelect from './parts/NativeSelect.svelte';
  import { capsStore } from '$lib/stores/caps.svelte';

  interface Props {
    page?: string;
    params: LaunchParamsForm;
    port: number;
    /** Harness picks the port at every start (`port` is then the last pick, 0 = none yet). */
    portAuto: boolean;
    maxGpuLayers: number;
    hardware?: SystemHardwareInfo | null;
    /** Shown under the port field (e.g. the port is already used by another of our servers). */
    portWarning?: string | null;
    /** Concurrent requests of the current form (from the memory estimate). */
    slotAdvice?: SlotAdvice | null;
  }

  let { page = 'gpu', params = $bindable(), port = $bindable(), portAuto = $bindable(), maxGpuLayers, hardware = null, portWarning = null, slotAdvice = null }: Props = $props();

  const uid = $props.id();
  const CTX_PRESETS = [2048, 4096, 8192, 16384, 32768, 65536];

  function formatBytes(bytes: number | null | undefined, integrated = false): string {
    // An integrated GPU has no VRAM of its own: it borrows system RAM.
    if ((!bytes || bytes <= 0) && integrated) return t('dash.lp_gpu_shared_mem');
    if (!bytes || bytes <= 0) return '0 MB';
    if (bytes >= 1024 * 1024 * 1024) return (bytes / (1024 * 1024 * 1024)).toFixed(1) + ' GB';
    return Math.round(bytes / (1024 * 1024)) + ' MB';
  }

  let gpus = $derived(hardware?.gpus ?? []);
  let hasMultipleGpus = $derived(gpus.length > 1);
  let defaultGpu = $derived(gpus.find((g) => g.is_default) ?? gpus[0]);
  let activeGpuIndex = $derived(params.main_gpu ?? defaultGpu?.index ?? 0);
  let activeGpu = $derived(gpus.find((g) => g.index === activeGpuIndex) ?? defaultGpu);
  let skippedStockGpu = $derived(gpus.find((g) => !g.is_discrete && g.index !== activeGpuIndex));

  /** Devices as the llama-server backend sees them (`--list-devices`, total/free VRAM with the
   *  OS usage folded in), supplementary to DXGI; the capability probe is the fallback. */
  let report = $state<DeviceReport | null>(null);
  $effect(() => {
    const serverId = app.selectedServerId;
    let cancelled = false;
    invoke<DeviceReport>('get_runtime_devices', { serverId, refresh: false })
      .then((r) => {
        if (!cancelled) report = r;
      })
      .catch(() => {
        if (!cancelled) report = null;
      });
    return () => {
      cancelled = true;
    };
  });
  const MIB = 1024 * 1024;
  let devices = $derived<RuntimeDeviceLive[]>(
    report?.devices.length
      ? report.devices
      : (capsStore.caps?.devices ?? []).map((d, i) => ({
          id: d.id,
          index: i,
          backend: '',
          name: d.name,
          total_bytes: d.total_mib == null ? null : d.total_mib * MIB,
          free_bytes: d.free_mib == null ? null : d.free_mib * MIB,
          os_used_bytes: null,
          available_bytes: d.free_mib == null ? null : d.free_mib * MIB,
          integrated: false,
        })),
  );
  let split = $derived(report?.split ?? null);
  /** A listed GPU the runtime cannot load the model on (an integrated one next to a discrete). */
  let unusableGpu = $derived(gpus.find((g) => !gpuAvailable(g.name)) ?? null);
  function gpuAvailable(name: string): boolean {
    if (!report || report.error) return true;
    const normalize = (value: string) => value.toLowerCase().replace(/\((?:r|tm)\)/g, ' ').split(/[^a-z0-9]+/).filter(Boolean).join(' ');
    const adapter = normalize(name);
    // llama.cpp loads a model only on discrete devices as soon as one exists (an integrated GPU
    // next to a discrete one cannot be chosen with --main-gpu).
    const discreteExists = report.devices.some((device) => !device.integrated);
    return report.devices.some((device) => {
      const runtime = normalize(device.name);
      return (
        (!discreteExists || !device.integrated) &&
        adapter.length > 0 &&
        runtime.length > 0 &&
        (adapter.includes(runtime) || runtime.includes(adapter))
      );
    });
  }
  let splitApplied = $derived(
    split !== null &&
      params.main_gpu === split.main_gpu &&
      params.tensor_split === split.tensor_split &&
      params.split_mode === split.split_mode,
  );
  function applySplit() {
    if (!split) return;
    params.main_gpu = split.main_gpu;
    params.tensor_split = split.tensor_split;
    params.split_mode = split.split_mode;
  }
  const bytes = (v: number | null) => (v == null ? '?' : formatBytes(v));

  let ctxPreset = $derived(CTX_PRESETS.includes(params.context_size) ? String(params.context_size) : '');
  const ctxLabel = (v: number) => (v >= 1024 ? `${v / 1024}k` : String(v));

  // Concurrent requests (-np): Auto (null) or a value; "Custom" shows a number field.
  const SLOT_PRESETS = [1, 2, 4, 8];
  let customSlots = $state(false);
  let slotMode = $derived(
    params.parallel_slots == null || params.parallel_slots <= 0
      ? 'auto'
      : customSlots || !SLOT_PRESETS.includes(params.parallel_slots)
        ? 'custom'
        : String(params.parallel_slots),
  );
  function setSlotMode(v: string) {
    customSlots = v === 'custom';
    if (v === 'auto') params.parallel_slots = null;
    else if (v === 'custom') params.parallel_slots = params.parallel_slots ?? slotAdvice?.recommended ?? 1;
    else params.parallel_slots = Number(v);
  }
  let slotReason = $derived(
    slotAdvice
      ? t(slotAdvice.reason as TKey, {
          mb: formatBytes(slotAdvice.per_slot_bytes),
          ctx: ctxLabel(Math.floor(slotAdvice.ctx_per_slot / 1024) * 1024),
        })
      : '',
  );
  /** One line under the field: what Auto picks, or the recommendation next to a set value. */
  let slotHint = $derived.by(() => {
    const a = slotAdvice;
    if (!a) return null;
    if (a.auto) {
      const agents = a.recommended_agents !== a.recommended ? ` ${t('dash.lp_slots_agents_note', { n: a.recommended_agents })}` : '';
      return `${t('dash.lp_slots_auto_value', { n: a.slots })} · ${slotReason}${agents}`;
    }
    return a.slots === a.recommended ? slotReason : `${t('dash.lp_slots_recommended', { n: a.recommended })} · ${slotReason}`;
  });
</script>

{#snippet gpuKindBadge(discrete: boolean)}
  <Badge variant="outline" class={cn('text-xs', discrete ? 'text-success' : 'text-warning')}>
    {discrete ? t('dash.lp_gpu_discrete_badge') : t('dash.lp_gpu_igpu_badge')}
  </Badge>
{/snippet}

<section hidden={!['gpu', 'context', 'network'].includes(page)} class="flex flex-col gap-3.5 border-b border-border pb-4" aria-labelledby="{uid}-title">
<span id="{uid}-title" class="sr-only">{t('dash.n_params')}</span>

<div hidden={page !== 'gpu'} class="flex flex-col gap-3">
  {#if gpus.length > 0}
    <Field label={t('dash.lp_gpu_select_label')} id="{uid}-gpu" help={t('dash.lp_help_gpu_select')}>
      {#snippet aside()}
        {#if activeGpu}{@render gpuKindBadge(activeGpu.is_discrete)}{/if}
      {/snippet}
      {#if hasMultipleGpus}
        <NativeSelect
          id="{uid}-gpu"
          value={params.main_gpu === null ? '' : String(params.main_gpu)}
          onchange={(e) => {
            const val = e.currentTarget.value;
            params.main_gpu = val === '' ? null : Number(val);
          }}
        >
          <option value="">
            {t('dash.lp_gpu_auto')}{defaultGpu ? ` (${defaultGpu.name} - ${formatBytes(defaultGpu.vram_bytes, !defaultGpu.is_discrete)})` : ''}
          </option>
          {#each gpus as g (g.index)}
            {@const usable = gpuAvailable(g.name)}
            <option value={String(g.index)} disabled={!usable} title={usable ? undefined : t('dash.lp_gpu_unavailable_note', { name: g.name })}>
              GPU {g.index}: {g.name} ({formatBytes(g.vram_bytes, !g.is_discrete)}){g.is_discrete ? ` ${t('dash.lp_gpu_discrete_suffix')}` : ''}{usable ? '' : ` ${t('dash.lp_gpu_unavailable_suffix')}`}
            </option>
          {/each}
        </NativeSelect>
      {:else if activeGpu}
        <div class="flex h-8 items-center justify-between gap-2 rounded-lg border border-border bg-muted/30 px-2.5 text-sm">
          <span class="truncate font-medium">{activeGpu.name}</span>
          <span class="shrink-0 font-num text-xs text-muted-foreground">{formatBytes(activeGpu.vram_bytes, !activeGpu.is_discrete)}</span>
        </div>
      {/if}
      {#if unusableGpu}
        <p class="text-xs leading-snug text-muted-foreground" role="note">{t('dash.lp_gpu_unavailable_note', { name: unusableGpu.name })}</p>
      {/if}
      {#if activeGpu && !gpuAvailable(activeGpu.name)}
        <p class="text-xs leading-snug text-warning" role="alert">
          {t('runtime.gpu_unavailable', { name: activeGpu.name, index: activeGpu.index })}
        </p>
      {/if}
      {#if skippedStockGpu && skippedStockGpu.index !== unusableGpu?.index && (params.main_gpu === null || params.main_gpu !== skippedStockGpu.index)}
        <p class="text-xs leading-snug text-muted-foreground" role="status">
          {t('dash.lp_gpu_skipped_note', { name: skippedStockGpu.name, vram: formatBytes(skippedStockGpu.vram_bytes, !skippedStockGpu.is_discrete) })}
        </p>
      {/if}
    </Field>
  {/if}

  {#if devices.length > 0}
    <div class="flex flex-col gap-1" title={t('caps.devices_tip')}>
      <span class="text-xs font-medium text-muted-foreground">{t('caps.devices_label')}</span>
      <ul class="flex flex-col gap-0.5">
        {#each devices as d (d.id)}
          <li class="flex items-baseline justify-between gap-2 text-xs">
            <span class="min-w-0 truncate">
              <code class="font-mono text-muted-foreground">{d.id}</code> {d.name}{#if d.integrated}<span class="text-muted-foreground"> · {t('dash.lp_device_integrated')}</span>{/if}
            </span>
            {#if d.total_bytes != null}
              <span class="shrink-0 font-num text-muted-foreground">
                {t('dash.lp_device_available', { avail: bytes(d.available_bytes ?? d.free_bytes), total: bytes(d.total_bytes) })}
              </span>
            {/if}
          </li>
        {/each}
      </ul>
      {#if split}
        <div class="mt-1 flex flex-col gap-1 rounded-md border border-border bg-muted/30 px-2.5 py-2" role="status">
          <span class="text-xs font-medium">{t('dash.lp_split_title')}</span>
          <span class="text-xs text-muted-foreground">
            {t('dash.lp_split_desc', { device: split.main_device, split: split.tensor_split })}
          </span>
          <div>
            <Button variant="outline" size="xs" onclick={applySplit} disabled={splitApplied}>
              {splitApplied ? t('dash.lp_split_applied') : t('dash.lp_split_apply')}
            </Button>
          </div>
        </div>
      {/if}
    </div>
  {/if}

  <Field label={t('dash.lp_gpu_layers_label_short')} id="{uid}-ngl" help={t('dash.lp_help_gpu_layers')} flag="-ngl">
    <div class="flex items-center gap-3">
      <Slider
        type="single"
        min={0}
        max={maxGpuLayers}
        step={1}
        value={params.gpu_layers ?? 0}
        onValueChange={(v) => (params.gpu_layers = v)}
        aria-label={t('dash.lp_gpu_layers_label_short')}
        class="flex-1"
      />
      <Input
        id="{uid}-ngl"
        type="number"
        min="0"
        max={maxGpuLayers}
        bind:value={params.gpu_layers}
        class="h-7 w-16 shrink-0 px-1.5 text-center font-num text-xs"
      />
    </div>
  </Field>

</div>
<div hidden={page !== 'context'} class="flex flex-col gap-3">
  <Field label={t('dash.lp_ctx_label_short')} id="{uid}-ctx" hint={t('dash.lp_help_ctx')} note={params.context_size < 65536 ? t('dash.lp_ctx_agent_hint') : undefined} flag="-c">
    <ToggleGroup.Root
      type="single"
      variant="outline"
      size="sm"
      value={ctxPreset}
      onValueChange={(v) => v && (params.context_size = Number(v))}
      class="w-full"
      aria-label={t('dash.lp_ctx_label_short')}
    >
      {#each CTX_PRESETS as v (v)}
        <ToggleGroup.Item value={String(v)} class="flex-1 px-0 font-num text-xs data-[state=on]:bg-primary/15 data-[state=on]:text-primary">
          {ctxLabel(v)}
        </ToggleGroup.Item>
      {/each}
    </ToggleGroup.Root>
    <Input id="{uid}-ctx" type="number" step="512" min="512" bind:value={params.context_size} class="font-num" />
  </Field>

  {#if capsStore.supports('parallel_slots')}
    <Field advanced label={t('dash.lp_slots_label')} id="{uid}-slots" help={t('dash.lp_help_slots')} note={slotHint ?? undefined} flag="-np">
      <div class="flex items-center gap-2">
        <NativeSelect id="{uid}-slots" value={slotMode} onchange={(e) => setSlotMode(e.currentTarget.value)} class="flex-1">
          <option value="auto">{t('dash.lp_slots_auto')}{slotAdvice?.auto ? ` (${slotAdvice.slots})` : ''}</option>
          {#each SLOT_PRESETS as n (n)}
            <option value={String(n)}>{n}</option>
          {/each}
          <option value="custom">{t('dash.lp_slots_custom')}</option>
        </NativeSelect>
        {#if slotMode === 'custom'}
          <Input
            type="number"
            min="1"
            max="64"
            aria-label={t('dash.lp_slots_custom')}
            bind:value={params.parallel_slots}
            class="h-8 w-20 shrink-0 font-mono tabular"
          />
        {/if}
      </div>
    </Field>
  {/if}

</div>
<div hidden={page !== 'network'} class="flex flex-col gap-3">
  <Field label={t('dash.port')} id="{uid}-port" help={t('dash.lp_help_port')} hint={portAuto ? t('dash.port_auto_hint') : undefined}>
    <div class="flex items-center gap-3">
      <Input
        id="{uid}-port"
        type="number"
        value={portAuto && !port ? '' : port}
        oninput={(e) => (port = Number(e.currentTarget.value) || 0)}
        disabled={portAuto}
        placeholder={portAuto ? t('dash.port_auto') : undefined}
        class="w-28 font-num"
        aria-invalid={portWarning ? 'true' : undefined}
        aria-describedby={portWarning ? `${uid}-port-warning` : undefined}
      />
      <label class="flex items-center gap-1.5 text-xs">
        <Checkbox bind:checked={portAuto} />
        {t('dash.port_auto')}
      </label>
    </div>
    {#if portWarning}
      <p class="text-xs leading-snug text-warning" id="{uid}-port-warning" role="alert">{portWarning}</p>
    {/if}
  </Field>
</div>
</section>
