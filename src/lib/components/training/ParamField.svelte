<script lang="ts">
  // One tunable setting: label, plain-language help, the recommended value and a reset.
  import { RotateCcw } from '@lucide/svelte';
  import { t } from '#lib/i18n.svelte.js';
  import InfoTip from '#lib/components/common/InfoTip.svelte';
  import { Input } from '#lib/components/ui/input/index.js';
  import NativeSelect from '#lib/components/dashboard/parts/NativeSelect.svelte';

  interface Option {
    value: string;
    label: string;
  }
  interface Props {
    id: string;
    label: string;
    help: string;
    value: number | string;
    /** The recommended value (shown, and restored by the reset button). */
    recommended?: number | string | null;
    options?: Option[];
    min?: number;
    max?: number;
    step?: number;
    invalid?: boolean;
  }
  let { id, label, help, value = $bindable(), recommended = null, options, min, max, step, invalid = false }: Props = $props();

  let changed = $derived(recommended !== null && value !== recommended);
  const shown = (v: number | string) => (options?.find((o) => o.value === v)?.label ?? String(v));
</script>

<div class="flex flex-col gap-1 text-sm">
  <div class="flex items-center gap-1">
    <label for={id} class="font-medium">{label}</label>
    <InfoTip text={help} {label} />
    {#if recommended !== null}
      {#if changed}
        <button
          type="button"
          class="ml-auto inline-flex items-center gap-1 rounded text-xs text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          onclick={() => (value = recommended)}
        >
          <RotateCcw class="size-3" aria-hidden="true" />{t('training.param_reset')}
        </button>
      {:else}
        <span class="ml-auto rounded-full bg-emerald-500/15 px-2 py-0.5 text-xs text-emerald-700 dark:text-emerald-400">{t('training.recommended')}</span>
      {/if}
    {/if}
  </div>
  {#if options}
    <NativeSelect {id} bind:value>
      {#each options as o (o.value)}<option value={o.value}>{o.label}</option>{/each}
    </NativeSelect>
  {:else}
    <Input {id} type="number" {min} {max} {step} bind:value aria-invalid={invalid} />
  {/if}
  {#if changed && recommended !== null}
    <p class="text-xs text-muted-foreground">{t('training.param_recommended_is', { value: shown(recommended) })}</p>
  {/if}
</div>
