<script lang="ts">
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { cn } from '#lib/utils.js';
  import { formatBytes, formatBytesPairText } from '#lib/format.js';
  import { kvText, type PreparePlan, type PrepareOption } from './agentTuning';

  interface Props {
    pp: PreparePlan;
    options: PrepareOption[];
    selected: number;
    name: string;
    disabled?: boolean;
  }
  let { pp, options, selected = $bindable(-1), name, disabled = false }: Props = $props();
</script>

<p class={cn('text-xs', pp.verdict === 'recommended' ? 'text-success' : pp.verdict === 'below_recommended' ? 'text-warning' : 'text-destructive')}>
  {#if !uiLevel.advanced}
    {t(pp.verdict === 'recommended' ? 'conn.prepare_recommended' : pp.verdict === 'below_recommended' ? 'conn.prepare_below' : 'conn.prepare_insufficient', { name })}
  {:else if pp.verdict === 'recommended' && pp.choice}
    {#if pp.cpu_only}
      {t('conn.tune_v_ok_cpu', { ctx: pp.choice.context_size.toLocaleString(), kv: kvText(pp.choice), pair: formatBytesPairText(pp.choice.ram_bytes + pp.choice.vram_bytes, pp.budget_ram_bytes) })}
    {:else}
      {t('conn.tune_v_ok', { ctx: pp.choice.context_size.toLocaleString(), kv: kvText(pp.choice), pair: formatBytesPairText(pp.choice.vram_bytes, pp.budget_vram_bytes) })}
    {/if}
  {:else if pp.verdict === 'below_recommended' && pp.choice}
    {t(pp.cpu_only ? 'conn.tune_v_below_cpu' : 'conn.tune_v_below', { name, rec: pp.need.recommended.toLocaleString(), ctx: pp.choice.context_size.toLocaleString() })}
  {:else}
    {t(pp.cpu_only ? 'conn.tune_v_min_cpu' : 'conn.tune_v_min', { name, min: pp.need.min.toLocaleString() })}
  {/if}
</p>
<AdvancedOnly>
  <ul class="list-disc space-y-0.5 pl-4 text-xs text-muted-foreground">
    {#if pp.choice && pp.limit === 'ceiling'}<li>{t('conn.tune_l_ceiling', { name })}</li>{/if}
    {#if pp.limit === 'model'}<li>{t('conn.tune_l_model', { max: pp.model.max.toLocaleString() })}</li>{/if}
    {#if pp.choice && pp.limit === 'memory'}<li>{t(pp.cpu_only ? 'conn.tune_l_memory_cpu' : 'conn.tune_l_memory')}</li>{/if}
    {#if pp.limit === 'available' && pp.available_memory}<li>{t('conn.tune_l_available', { free: formatBytes(pp.available_memory.available_vram_bytes, 1), headroom: formatBytes(pp.available_memory.headroom_bytes, 1) })}</li>{/if}
    {#if !pp.available_memory && !pp.cpu_only}<li>{t('conn.tune_l_capacity_only')}</li>{/if}
    {#if pp.model.extended_by_rope && pp.model.rope}<li>{t('conn.tune_l_rope', { rope: pp.model.rope, trained: pp.model.trained.toLocaleString(), max: pp.model.max.toLocaleString() })}</li>{/if}
    {#if pp.choice && pp.choice.kv_level !== 'full'}<li>{t(`conn.tune_l_kv_${pp.choice.kv_level}`)}</li>{/if}
    {#if pp.cpu_only}<li>{t(pp.shared_igpu ? 'conn.tune_l_igpu' : 'conn.tune_l_cpu')}</li>{/if}
  </ul>
</AdvancedOnly>
<div role="radiogroup" aria-label={t('conn.tune_title', { name })} class="flex flex-col gap-1.5">
  {#each options as o, i (i)}
    <button
      type="button"
      role="radio"
      aria-checked={selected === i}
      class={cn('rounded-md border p-2.5 text-left text-xs transition-colors', selected === i ? 'border-primary bg-primary/5' : 'border-border hover:bg-muted/50')}
      onclick={() => (selected = i)}
      {disabled}
    >
      <span class="font-medium">{t(`conn.tune_o_${o.kind}`)}</span>
      <AdvancedOnly>
        <span class="mt-0.5 block font-num text-muted-foreground">
          {#if pp.cpu_only}
            {t('conn.tune_o_line_cpu', { ctx: o.context_size.toLocaleString(), kv: kvText(o), pair: formatBytesPairText(o.ram_bytes + o.vram_bytes, pp.budget_ram_bytes), percent: o.percent })}
          {:else}
            {t('conn.tune_o_line', { ctx: o.context_size.toLocaleString(), kv: kvText(o), pair: formatBytesPairText(o.vram_bytes, pp.budget_vram_bytes), percent: o.percent })}
          {/if}
          {#if o.gpu_layers !== null && !pp.cpu_only}· {t('conn.tune_o_layers', { n: o.gpu_layers, ram: formatBytes(o.ram_bytes, 1) })}{/if}
        </span>
      </AdvancedOnly>
    </button>
  {/each}
</div>

{#if options.length === 0}
  <p class="text-xs text-muted-foreground">{t('intg.launch_prepare_none')}</p>
{/if}
