<script lang="ts">
  import { Cpu, MemoryStick, Zap } from '@lucide/svelte';
  import type { SystemHardwareInfo } from '$lib/types';
  import { formatBytes } from '$lib/format';
  import { t } from '$lib/i18n.svelte';
  import { Badge } from '$lib/components/ui/badge';
  import { BACKEND_TOKEN, backendName } from '$lib/runtimeBackend';

  interface Props {
    hardware: SystemHardwareInfo | null;
  }

  let { hardware }: Props = $props();

  let gpus = $derived(hardware?.gpus ?? []);
  let discreteGpu = $derived(gpus.find((g) => g.is_discrete) ?? gpus[0]);
  let hasIntegrated = $derived(gpus.some((g) => g !== discreteGpu && !g.is_discrete));
</script>

<div class="flex flex-col gap-4">
  <div class="grid grid-cols-2 gap-3">
    <div class="flex min-w-0 flex-col gap-1 rounded-lg border border-border bg-card p-3">
      <span class="flex items-center gap-1.5 text-xs text-muted-foreground"><Cpu class="size-3.5" /> {t('onboarding.wizard_cpu_label')}</span>
      <span class="truncate text-sm font-medium" title={hardware?.cpu_name}>{hardware?.cpu_name ?? t('onboarding.wizard_unknown')}</span>
      <span class="font-num text-xs text-muted-foreground">{t('onboarding.wizard_cores', { count: hardware?.cpu_cores ?? 0 })}</span>
    </div>
    <div class="flex min-w-0 flex-col gap-1 rounded-lg border border-border bg-card p-3">
      <span class="flex items-center gap-1.5 text-xs text-muted-foreground"><MemoryStick class="size-3.5" /> {t('onboarding.wizard_ram_label')}</span>
      <span class="font-num text-sm font-medium">{formatBytes(hardware?.total_ram_bytes, 1)}</span>
      <span class="font-num text-xs text-muted-foreground">{t('onboarding.wizard_available', { size: formatBytes(hardware?.available_ram_bytes, 1) })}</span>
    </div>
  </div>

  {#if discreteGpu}
    <div class="flex min-w-0 flex-col gap-1.5 rounded-lg border border-primary/30 bg-primary/5 p-3">
      <div class="flex min-w-0 items-center justify-between gap-3">
        <span class="text-xs text-muted-foreground">{t('onboarding.wizard_primary_gpu')}</span>
        <Badge variant="outline" class="max-w-[70%] font-mono">
          <span class="truncate">{discreteGpu.name}</span>
        </Badge>
      </div>
      <div class="flex flex-wrap gap-x-4 gap-y-1 font-num text-xs text-muted-foreground">
        {#if discreteGpu.vram_bytes}
          <span>{t('onboarding.wizard_vram_label')}: <span class="text-foreground">{formatBytes(discreteGpu.vram_bytes, 1)}</span></span>
        {:else if !discreteGpu.is_discrete}
          <span>{t('onboarding.wizard_vram_label')}: <span class="text-foreground">{t('onboarding.wizard_vram_shared')}</span></span>
        {/if}
        {#if discreteGpu.driver_version}
          <span>{t('onboarding.wizard_driver')} <span class="text-foreground">{discreteGpu.driver_version}</span></span>
        {/if}
      </div>
      {#if hasIntegrated}
        <p class="text-xs text-muted-foreground">{t('onboarding.wizard_igpu_skipped')}</p>
      {/if}
    </div>
  {/if}

  <div class="flex items-center justify-between gap-3 rounded-lg border border-border bg-card px-3 py-2.5">
    <span class="flex items-center gap-1.5 text-sm"><Zap class="size-3.5 text-primary" /> {t('onboarding.wizard_recommended_backend')}</span>
    <!-- The CUDA version comes with the release (the lowest one it has), not known here. -->
    <Badge variant="secondary">{backendName(BACKEND_TOKEN[hardware?.recommended_backend ?? 'Cpu'])}</Badge>
  </div>
  {#if hardware?.backend_advice?.reason_key === 'backend.reason_nvidia_driver_old'}
    <p class="text-xs text-warning" role="status">
      {t('backend.reason_nvidia_driver_old', { driver: hardware.driver_info.cuda_driver_version ?? '?' })}
    </p>
  {/if}
</div>
