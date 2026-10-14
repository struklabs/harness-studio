<script lang="ts">
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import AdvancedOnly from '#lib/components/common/AdvancedOnly.svelte';
  import { invoke } from '#lib/ipc.js';
  import type { EffectiveLaunchValues, ServerStatus } from '#lib/types.js';
  import { locale, t } from '#lib/i18n.svelte.js';
  import { windowActive } from '#lib/windowActivity.js';
  import { spillWarning } from '#lib/fit.js';

  interface Props {
    serverStatus: ServerStatus;
    serverId: string;
    /** The form's KV cache types (empty = automatic) and custom arguments. */
    kv?: { k: string | null | undefined; v: string | null | undefined; customArgs?: string | null };
    onReloadQ8?: () => void;
  }

  let { serverStatus, serverId, kv = { k: '', v: '' }, onReloadQ8 }: Props = $props();

  let values = $state<EffectiveLaunchValues | null>(null);

  const formatNumber = (n: number) => n.toLocaleString(locale());
  let running = $derived(serverStatus === 'Running');

  // Values come after each start (the backend probes /props right after /health), then are
  // read again every few seconds: some arrive later (the CUDA slow KV pair warning comes
  // with the first generated token).
  const REFRESH_MS = 5000;
  $effect(() => {
    const id = serverId;
    if (!running) {
      values = null;
      return;
    }
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async (attempt: number) => {
      // Hidden (minimized or in the tray): nothing to show; look again later.
      if (!windowActive()) {
        timer = setTimeout(() => poll(attempt), REFRESH_MS);
        return;
      }
      try {
        const v = await invoke<EffectiveLaunchValues | null>('get_effective_launch_values', { serverId: id });
        if (cancelled) return;
        if (v) {
          values = v;
          timer = setTimeout(() => poll(0), REFRESH_MS);
          return;
        }
      } catch {
        // server went away; the status change resets this effect
      }
      if (!cancelled && attempt < 20) timer = setTimeout(() => poll(attempt + 1), 1000);
    };
    poll(0);
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  });

  let spill = $derived(spillWarning(values, kv));

  let ctxText = $derived.by(() => {
    if (!values?.n_ctx) return null;
    const effective = `${formatNumber(values.n_ctx)} ${values.total_slots && values.total_slots > 1 ? t('caps.eff_ctx_per_slot') : ''}`.trim();
    if (!values.ctx_adjusted) return t('caps.eff_ctx_same', { effective });
    const requested = values.requested_ctx === 0 ? `0 (${t('caps.eff_ctx_model_default')})` : formatNumber(values.requested_ctx);
    return t('caps.eff_ctx_value', { requested, effective });
  });
</script>

{#if running && values && (uiLevel.advanced || values.ctx_adjusted || values.kv_fa_fallback || values.fit_failed || values.shared_memory_slowdown)}
  <section class="flex flex-wrap items-baseline gap-x-4 gap-y-1 border-b border-border px-4 py-2 text-xs" aria-label={t('caps.eff_title')} title={t('caps.eff_tip')}>
    <AdvancedOnly><span class="font-medium text-foreground">{t('caps.eff_title')}</span></AdvancedOnly>
    {#if ctxText && (uiLevel.advanced || values.ctx_adjusted)}
      <span class={values.ctx_adjusted ? 'text-warning' : 'text-foreground'} title={values.ctx_adjusted ? t('caps.eff_ctx_adjusted') : undefined}>
        <span class="text-muted-foreground">{t('caps.eff_ctx')}:</span>
        <span class="font-num">{ctxText}</span>
      </span>
    {/if}
    <AdvancedOnly>
      {#if values.total_slots != null}
        <span><span class="text-muted-foreground">{t('caps.eff_slots')}:</span> <span class="font-num">{values.total_slots}</span></span>
      {/if}
      {#if values.kv_unified != null}
        <span>
          <span class="text-muted-foreground">{t('caps.eff_kv_unified')}:</span>
          {values.kv_unified ? t('caps.bool_on') : t('caps.bool_off')}
        </span>
      {/if}
    </AdvancedOnly>
    {#if values.kv_fa_fallback}
      <span class="text-warning" title={t('caps.eff_kv_fallback_tip', { pair: values.kv_fa_fallback })}>
        <span>{t('caps.eff_kv_fallback')}:</span>
        <AdvancedOnly><span class="font-mono text-muted-foreground">{values.kv_fa_fallback}</span></AdvancedOnly>
      </span>
    {/if}
    {#if values.fit_failed}
      <span class="text-warning" title={t('caps.eff_fit_failed_tip')}>{t('caps.eff_fit_failed')}</span>
    {/if}
    {#if values.shared_memory_slowdown && !values.fit_failed}
      <span class="text-warning" title={t('caps.eff_shared_slowdown_tip', { mib: Math.round((values.shared_memory_bytes ?? 0) / 1048576) })}>{t('caps.eff_shared_slowdown')}</span>
    {/if}
    <AdvancedOnly>
      {#if values.gpu_layers_offloaded != null}
        <span title={values.gpu_layers_source === 'derived' ? t('caps.eff_gpu_layers_derived') : values.gpu_layers_source === 'log' ? t('caps.eff_gpu_layers_log') : undefined}>
          <span class="text-muted-foreground">{t('caps.eff_gpu_layers')}:</span>
          <span class="font-num">{values.gpu_layers_offloaded}/{values.gpu_layers_total ?? '?'}</span>
        </span>
      {:else if values.gpu_layers_by_fit}
        <span title={t('caps.eff_gpu_layers_fit_tip')}>
          <span class="text-muted-foreground">{t('caps.eff_gpu_layers')}:</span>
          {t('caps.eff_gpu_layers_fit')}{values.gpu_layers_total != null ? ` (/${values.gpu_layers_total})` : ''}
        </span>
      {/if}
      {#if values.n_ctx_train != null}
        <span><span class="text-muted-foreground">{t('caps.eff_train_ctx')}:</span> <span class="font-num">{formatNumber(values.n_ctx_train)}</span></span>
      {/if}
      {#if values.build_info}
        <span class="ml-auto font-mono text-muted-foreground">{values.build_info}</span>
      {/if}
    </AdvancedOnly>
  </section>
  {#if spill.show}
    <div class="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-border bg-warning/10 px-4 py-2 text-xs text-warning" role="alert">
      <span>{t('caps.spill_warn')}</span>
      {#if spill.canReloadQ8 && onReloadQ8}
        <button type="button" class="rounded border border-warning px-2 py-0.5 font-medium hover:bg-warning/20" onclick={onReloadQ8}>{t('caps.spill_reload_q8')}</button>
      {/if}
    </div>
  {/if}
{/if}
