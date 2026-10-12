<script lang="ts">
  import AdvancedOnly from '$lib/components/common/AdvancedOnly.svelte';
  // Runtime (llama-server build) of the launch form, with the capability summary of the
  // selected build and the advice from the model's GGUF tensor types (e.g. Bonsai PQ2_0 needs
  // the PrismML fork). Hidden while there is nothing to choose and nothing to warn about.
  import { invoke } from '$lib/ipc';
  import { onMount } from 'svelte';
  import type { RuntimeAdvice } from '$lib/types';
  import { t } from '$lib/i18n.svelte';
  import { localizeError } from '$lib/network';
  import { runtimesStore, runtimeBuildLabel } from '$lib/stores/runtimes.svelte';
  import { security } from '$lib/stores/security.svelte';
  import { Button } from '$lib/components/ui/button';
  import { TriangleAlert } from '@lucide/svelte';
  import Field from './parts/Field.svelte';
  import NativeSelect from './parts/NativeSelect.svelte';

  interface Props {
    /** Runtime id ('' = official). */
    runtime: string;
    /** Selected model file (null in router mode). */
    modelPath: string | null;
    /** Compact variant for the definition dialog (no advice banner). */
    compact?: boolean;
    hint?: string;
  }

  let { runtime = $bindable(), modelPath, compact = false, hint }: Props = $props();
  const uid = $props.id();

  onMount(() => {
    void runtimesStore.load();
  });

  let selected = $derived(runtimesStore.get(runtime));
  let hidden = $derived(selected?.unsupported_params.length ?? 0);
  /** A saved choice that is no longer registered stays selectable (and shows as missing). */
  let unknownChoice = $derived(!!runtime && runtime !== 'official' && !runtimesStore.get(runtime) && runtimesStore.list.length > 0);

  let advice = $state<RuntimeAdvice | null>(null);
  $effect(() => {
    const path = modelPath;
    const current = runtime || null;
    if (compact || !path) {
      advice = null;
      return;
    }
    let cancelled = false;
    invoke<RuntimeAdvice>('get_model_runtime_advice', { modelPath: path, runtimeId: current })
      .then((a) => {
        if (!cancelled) advice = a;
      })
      .catch(() => {
        if (!cancelled) advice = null;
      });
    return () => {
      cancelled = true;
    };
  });

  let fit = $derived(advice?.runtimes.find((r) => r.runtime_id === (runtime || 'official')));
  let blocked = $derived(fit?.support === 'unsupported');
  let types = $derived(
    (fit?.missing.length ? fit.missing : (advice?.special_types ?? [])).map((x) => x.name ?? `#${x.id}`).join(', '),
  );
  let suggested = $derived(advice?.suggested_runtime ? runtimesStore.get(advice.suggested_runtime) : undefined);
  let legacyPrism = $derived(advice?.special_types.some((type) => type.id === 4294967295) ?? false);
  let visible = $derived(runtimesStore.hasExtra || blocked || unknownChoice);

  function nameOf(id: string | null | undefined): string {
    const r = runtimesStore.get(id);
    if (!r || r.id === 'official') return t('rt.official');
    if (r.kind === 'system') return t('rt.system');
    return r.name || r.id;
  }
</script>

{#if visible || compact}
  <div class="flex flex-col gap-2">
    <Field advanced label={t('rt.label')} id="{uid}-runtime" help={t('rt.help')} hint={hint}>
      <NativeSelect id="{uid}-runtime" bind:value={runtime}>
        <option value="">{t('rt.official')} · {runtimeBuildLabel(runtimesStore.get('official')) || t('rt.official_hint')}</option>
        {#each runtimesStore.list.filter((r) => r.id !== 'official') as r (r.id)}
          <option value={r.id}>{r.kind === 'system' ? t('rt.system') : r.name || r.id}{runtimeBuildLabel(r) ? ` · ${runtimeBuildLabel(r)}` : ''}</option>
        {/each}
        {#if unknownChoice}
          <option value={runtime}>{runtime} ({t('runtime.not_found')})</option>
        {/if}
      </NativeSelect>
    </Field>

    <AdvancedOnly>
      {#if selected}
        <p class="text-xs text-muted-foreground" role="status">
          {#if !selected.available}
            <span class="text-destructive">{t('rt.unavailable', { error: localizeError(selected.error ?? '') })}</span>
          {:else}
            {t('rt.summary', { flags: selected.flag_count })} ·
            {hidden > 0 ? t('rt.hidden_params', { count: hidden }) : t('rt.hidden_none')}
            {#if selected.quant_types.length > 0}
              · {t('rt.quants', { count: selected.quant_types.length })}
            {/if}
          {/if}
        </p>
      {/if}

    </AdvancedOnly>
    {#if blocked && advice}
      <div class="flex flex-col gap-1.5 rounded-md border border-warning/40 bg-warning/10 px-2.5 py-2 text-xs" role="alert">
        <span class="flex items-start gap-1.5">
          <TriangleAlert class="mt-0.5 size-3.5 shrink-0 text-warning" />
          <span>{t('rt.advice_unsupported', { types })}</span>
        </span>
        <div class="flex flex-wrap gap-2">
          {#if legacyPrism}
            <span class="basis-full text-muted-foreground">{t('rt.advice_legacy_prism')}</span>
          {/if}
          {#if suggested}
            <Button size="xs" variant="outline" onclick={() => (runtime = suggested.id === 'official' ? '' : suggested.id)}>
              {t('rt.advice_use', { name: nameOf(suggested.id) })}
            </Button>
          {:else if advice.fork}
            <span class="text-muted-foreground">{t('rt.advice_fork_hint', { repo: advice.fork.repo })}</span>
            <Button size="xs" variant="outline" onclick={() => security.openSettings('runtime')}>
              {t('rt.advice_add_fork', { name: advice.fork.name })}
            </Button>
          {:else}
            <span class="text-muted-foreground">{t('rt.advice_none')}</span>
          {/if}
        </div>
      </div>
    {/if}
  </div>
{/if}
