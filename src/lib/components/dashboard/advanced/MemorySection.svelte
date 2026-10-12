<script lang="ts">
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { capsStore } from '#lib/stores/caps.svelte.js';
  import type { SystemHardwareInfo } from '#lib/types.js';
  import type { LaunchParamsForm } from '../params';
  import Field from '../parts/Field.svelte';
  import NativeSelect from '../parts/NativeSelect.svelte';
  import AdvSection from './AdvSection.svelte';
  import CatalogFields from './CatalogFields.svelte';
  import FeatureBadge from './FeatureBadge.svelte';

  interface Props {
    params: LaunchParamsForm;
    hardware?: SystemHardwareInfo | null;
    /** Context length the selected model was trained on (GGUF metadata), when known. */
    trainedContext?: number | null;
  }

  let { params = $bindable(), hardware = null, trainedContext = null }: Props = $props();

  const uid = $props.id();

  /** Only a suggestion: the chosen context exceeds the trained one and no YaRN is set. */
  let yarnHint = $derived.by(() => {
    const ctx = params.context_size;
    if (!trainedContext || trainedContext <= 0 || !ctx || ctx <= trainedContext) return null;
    if (params.rope_scaling === 'yarn' || !capsStore.supports('rope_scaling')) return null;
    return t('param.yarn_hint', {
      ctx: ctx.toLocaleString(),
      trained: trainedContext.toLocaleString(),
      factor: Math.ceil((ctx / trainedContext) * 10) / 10,
    });
  });

  const KV_TYPES = [
    { value: 'f16', label: 'dash.adv_cache_f16' },
    { value: 'q8_0', label: 'dash.adv_cache_q8' },
    { value: 'q4_0', label: 'dash.adv_cache_q4' },
  ] as const;
  /** Our KV types the runtime lists (all of them while it is unknown). */
  let kvTypes = $derived.by(() => {
    const spec = capsStore.spec('cache_type_k');
    const allowed = spec ? capsStore.allowedValues(spec) : [];
    return allowed.length ? KV_TYPES.filter((k) => allowed.includes(k.value)) : KV_TYPES;
  });

  // "Konuşma hafızası hassasiyeti": named levels over the K/V types (kv_policy::KvLevel).
  // Empty types = automatic (the launch keeps f16 unless the context does not fit).
  type Precision = 'auto' | 'full' | 'balanced' | 'saver' | 'custom';
  let equalPairs = $derived(['Cuda12', 'Metal', 'Rocm'].includes(hardware?.recommended_backend ?? ''));
  let LEVELS = $derived<Record<Exclude<Precision, 'auto' | 'custom'>, [string, string]>>({
    full: ['f16', 'f16'],
    balanced: ['q8_0', 'q8_0'],
    saver: [equalPairs ? 'q4_0' : 'q8_0', 'q4_0'],
  });
  let PRECISIONS = $derived<{ value: Precision; label: TKey; hint: TKey }[]>([
    { value: 'auto', label: 'kv.auto', hint: 'kv.auto_hint' },
    { value: 'full', label: 'kv.full', hint: 'kv.full_hint' },
    { value: 'balanced', label: 'kv.balanced', hint: 'kv.balanced_hint' },
    { value: 'saver', label: equalPairs ? 'kv.saver_equal' : 'kv.saver', hint: equalPairs ? 'kv.saver_equal_hint' : 'kv.saver_hint' },
    { value: 'custom', label: 'kv.custom', hint: 'kv.custom_hint' },
  ]);
  /** The user picked "custom" (K/V selects stay open even on a named pair). */
  let customOpen = $state(false);
  let precision = $derived.by<Precision>(() => {
    if (customOpen) return 'custom';
    const k = params.cache_type_k || '';
    const v = params.cache_type_v || '';
    if (!k && !v) return 'auto';
    for (const [name, [lk, lv]] of Object.entries(LEVELS)) {
      if ((k || 'f16') === lk && (v || 'f16') === lv) return name as Precision;
    }
    return 'custom';
  });
  let precisionHint = $derived(PRECISIONS.find((p) => p.value === precision)?.hint ?? 'kv.auto_hint');
  let vQuantized = $derived(!!params.cache_type_v && params.cache_type_v !== 'f16');
  let rocmMixed = $derived(hardware?.recommended_backend === 'Rocm' &&
    (params.cache_type_k || 'f16') !== (params.cache_type_v || 'f16'));

  function setPrecision(value: Precision) {
    customOpen = value === 'custom';
    if (value === 'auto') {
      params.cache_type_k = '';
      params.cache_type_v = '';
    } else if (value !== 'custom') {
      [params.cache_type_k, params.cache_type_v] = LEVELS[value];
    }
  }
</script>

<AdvSection open={true} title={t('dash.n_sec_memory')}>
  {#if capsStore.supports('cache_type_k')}
    <Field label={t('kv.label')} id="{uid}-kvp" help={t('kv.help')} note={[t(precisionHint), vQuantized && !params.flash_attn ? t('kv.fa_note') : undefined].filter(Boolean).join(' ')} flag="-ctk / -ctv">
      {#snippet aside()}<FeatureBadge feature="kv_quant" />{/snippet}
      <NativeSelect id="{uid}-kvp" value={precision} onchange={(e) => setPrecision((e.currentTarget as HTMLSelectElement).value as Precision)}>
        {#each PRECISIONS as p (p.value)}<option value={p.value}>{t(p.label)}</option>{/each}
      </NativeSelect>
    </Field>
    {#if rocmMixed}
      <p class="text-xs leading-snug text-warning" role="status">{t('kv.rocm_mixed_warning')}</p>
    {/if}
  {/if}

  {#if capsStore.supports('cache_type_k') && precision === 'custom'}
    <div class="grid grid-cols-2 gap-2">
      <Field help={t('help.cache')} label={t('dash.adv_cache_k_label')} id="{uid}-ck" flag="-ctk">
        <NativeSelect id="{uid}-ck" bind:value={params.cache_type_k}>
          <option value="">{t('dash.adv_cache_default')}</option>
          {#each kvTypes as k (k.value)}<option value={k.value}>{t(k.label)}</option>{/each}
        </NativeSelect>
      </Field>
      <Field help={t('help.cache')} label={t('dash.adv_cache_v_label')} id="{uid}-cv" flag="-ctv">
        <NativeSelect id="{uid}-cv" bind:value={params.cache_type_v}>
          <option value="">{t('dash.adv_cache_default')}</option>
          {#each kvTypes as k (k.value)}<option value={k.value}>{t(k.label)}</option>{/each}
        </NativeSelect>
      </Field>
    </div>
  {/if}

  {#if capsStore.supports('context_shift')}
    <Field label={t('dash.adv_ctxshift_label')} id="{uid}-cs" help={t('dash.adv_ctxshift_hint')}>
      <NativeSelect id="{uid}-cs" bind:value={params.context_shift}>
        <option value="default">{t('dash.adv_ctxshift_default')}</option>
        <option value="enabled">{t('dash.adv_ctxshift_enabled')}</option>
        <option value="disabled">{t('dash.adv_ctxshift_disabled')}</option>
      </NativeSelect>
    </Field>
  {/if}

  <CatalogFields bind:params group="memory" />

  {#if yarnHint}
    <p class="text-xs leading-snug text-muted-foreground" role="note">{yarnHint}</p>
  {/if}
  {#if capsStore.supports('rope_scaling')}
    <div class="grid grid-cols-3 gap-2">
      <Field help={t('help.rope')} label={t('dash.adv_rope_mode_label')} id="{uid}-rope">
        <NativeSelect id="{uid}-rope" bind:value={params.rope_scaling} class="px-1.5">
          <option value="">{t('dash.adv_default_only')}</option>
          <option value="linear">Linear</option>
          <option value="yarn">YaRN</option>
          <option value="none">{t('dash.adv_rope_none')}</option>
        </NativeSelect>
      </Field>
      <Field help={t('help.freq_base')} label={t('dash.adv_freqbase_label')} id="{uid}-rfb">
        <Input id="{uid}-rfb" type="number" placeholder="10000" bind:value={params.rope_freq_base} class="px-2 font-num" />
      </Field>
      <Field help={t('help.freq_scale')} label={t('dash.adv_freqscale_label')} id="{uid}-rfs">
        <Input id="{uid}-rfs" type="number" step="0.1" placeholder="1.0" bind:value={params.rope_freq_scale} class="px-2 font-num" />
      </Field>
    </div>
  {/if}

  <CatalogFields bind:params group="context" />
</AdvSection>
