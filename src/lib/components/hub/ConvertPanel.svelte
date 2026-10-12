<script lang="ts">
  // Model Hub > a safetensors LLM repo: "Convert to GGUF". The backend plans the conversion from
  // the repo's own files and the converter's architecture list (nothing is downloaded yet); the
  // user picks the quantization and sees the sizes and the disk the run needs. The job then runs
  // in the conversion queue.
  import { invoke } from '$lib/ipc';
  import { toast } from 'svelte-sonner';
  import { untrack } from 'svelte';
  import { t, type TKey } from '$lib/i18n.svelte';
  import { formatBytes } from '$lib/format';
  import { localizeError } from '$lib/network';
  import { converts, isConvertActive } from '$lib/stores/converts.svelte';
  import type { ConvertPlan } from '$lib/types';
  import { Button } from '$lib/components/ui/button';
  import { Checkbox } from '$lib/components/ui/checkbox';
  import { Label } from '$lib/components/ui/label';
  import * as RadioGroup from '$lib/components/ui/radio-group';
  import { Skeleton } from '$lib/components/ui/skeleton';
  import { cn } from '$lib/utils';
  import { AlertCircle, Ban, Info, LoaderCircle, RefreshCw, Repeat } from '@lucide/svelte';

  interface Props {
    repoId: string;
  }

  let { repoId }: Props = $props();

  const uid = $props.id();
  const fmt = (b: number) => formatBytes(b, 1);

  let plan = $state<ConvertPlan | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let quant = $state('Q4_K_M');
  let keepOriginal = $state(false);
  let starting = $state(false);
  let token = 0;

  const QUANT_KEYS = ['Q4_K_M', 'Q5_K_M', 'Q6_K', 'Q8_0', 'F16', 'BF16'];

  async function load(id: string) {
    const my = ++token;
    loading = true;
    error = null;
    plan = null;
    try {
      const res = await invoke<ConvertPlan>('convert_plan', { repoId: id });
      if (my !== token) return;
      plan = res;
      quant = res.default_quant;
    } catch (e) {
      if (my === token) error = t('cvt.plan_failed', { error: localizeError(e) });
    } finally {
      if (my === token) loading = false;
    }
  }

  $effect(() => {
    const id = repoId;
    untrack(() => void load(id));
  });

  let estimate = $derived(plan?.quants.find((q) => q.quant === quant) ?? null);
  let enoughDisk = $derived(!plan || !estimate || plan.free_bytes >= estimate.disk_bytes);
  let running = $derived.by(() => {
    const job = converts.jobFor(repoId);
    return job && isConvertActive(job) ? job : null;
  });
  let weightsBytes = $derived(plan ? plan.files.filter((f) => f.name.endsWith('.safetensors')).reduce((a, f) => a + f.size, 0) : 0);

  async function start() {
    if (!plan || starting) return;
    starting = true;
    try {
      await converts.enqueue(repoId, quant, keepOriginal);
      toast.success(t('cvt.queued_toast', { repo: repoId }));
    } catch (e) {
      toast.error(t('cvt.start_failed', { error: localizeError(e) }));
    } finally {
      starting = false;
    }
  }
</script>

<section class="flex flex-col gap-3 rounded-lg border border-border bg-card p-3" aria-labelledby="{uid}-title">
  <header class="flex items-center gap-2">
    <Repeat class="size-4 shrink-0 text-primary" />
    <h3 id="{uid}-title" class="text-sm font-semibold">{t('cvt.panel_title')}</h3>
  </header>
  <p class="text-xs text-muted-foreground">{t('cvt.panel_intro')}</p>

  {#if loading}
    <div class="flex flex-col gap-2" role="status" aria-label={t('cvt.loading')}>
      <Skeleton class="h-5 w-2/3" />
      <Skeleton class="h-16 w-full" />
    </div>
  {:else if error}
    <div class="flex items-center justify-between gap-3 rounded-lg border border-destructive/30 bg-destructive/10 p-3 text-xs text-destructive" role="alert">
      <span class="flex min-w-0 gap-2"><AlertCircle class="mt-px size-3.5 shrink-0" /><span class="break-words">{error}</span></span>
      <Button variant="outline" size="xs" onclick={() => load(repoId)}><RefreshCw />{t('cvt.retry')}</Button>
    </div>
  {:else if plan && !plan.convertible}
    <div class="flex items-start gap-2 rounded-lg border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-foreground" role="status">
      <Ban class="mt-px size-3.5 shrink-0 text-warning" />
      <span class="min-w-0 flex-1 break-words">{plan.reason ? localizeError(plan.reason) : t('cvt.not_convertible')}</span>
    </div>
  {:else if plan}
    <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs">
      {#if plan.architecture}
        <dt class="text-muted-foreground">{t('cvt.architecture')}</dt>
        <dd class="font-mono">{plan.architecture}</dd>
      {/if}
      <dt class="text-muted-foreground">{t('cvt.parameters')}</dt>
      <dd class="font-num">{new Intl.NumberFormat(undefined, { notation: 'compact', maximumFractionDigits: 1 }).format(plan.parameters)}</dd>
      <dt class="text-muted-foreground">{t('cvt.download_size')}</dt>
      <dd class="font-num">{fmt(plan.download_bytes)}</dd>
      {#if estimate}
        <dt class="text-muted-foreground">{t('cvt.result_size')}</dt>
        <dd class="font-num">≈ {fmt(estimate.size_bytes)}</dd>
        <dt class="text-muted-foreground">{t('cvt.disk_needed')}</dt>
        <dd class={cn('font-num', !enoughDisk && 'text-destructive')}>≈ {fmt(estimate.disk_bytes)} <span class="text-muted-foreground">({t('cvt.disk_free')}: {fmt(plan.free_bytes)})</span></dd>
      {/if}
      {#if plan.license}
        <dt class="text-muted-foreground">{t('cvt.license')}</dt>
        <dd>{plan.license}</dd>
      {/if}
    </dl>

    <RadioGroup.Root class="grid grid-cols-2 gap-1.5 max-[520px]:grid-cols-1" aria-label={t('cvt.quant_label')} bind:value={quant}>
      {#each QUANT_KEYS as q (q)}
        <label
          for="{uid}-q-{q}"
          class={cn(
            'flex min-w-0 cursor-pointer items-center gap-2 rounded-md border px-2.5 py-1.5 text-xs transition-colors hover:bg-muted/50 has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring/50',
            quant === q ? 'border-primary/40 bg-primary/5' : 'border-border'
          )}
        >
          <RadioGroup.Item id="{uid}-q-{q}" value={q} />
          <span class="min-w-0 flex-1 truncate">{t(`cvt.q_${q}` as TKey)}</span>
          <span class="shrink-0 font-num text-muted-foreground">{fmt(plan.quants.find((x) => x.quant === q)?.size_bytes ?? 0)}</span>
        </label>
      {/each}
    </RadioGroup.Root>

    <div class="flex items-start gap-2">
      <Checkbox id="{uid}-keep" bind:checked={keepOriginal} class="mt-0.5" />
      <div class="flex min-w-0 flex-col gap-0.5">
        <Label for="{uid}-keep" class="text-xs font-normal">{t('cvt.keep_original')}</Label>
        <span class="text-xs text-muted-foreground">{t('cvt.keep_original_hint', { size: fmt(weightsBytes) })}</span>
      </div>
    </div>

    {#if plan.vision}
      <p class="flex items-start gap-1.5 text-xs text-muted-foreground"><Info class="mt-px size-3.5 shrink-0" />{t('cvt.vision_note')}</p>
    {/if}
    {#if plan.gated}
      <p class="flex items-start gap-1.5 text-xs text-warning"><Info class="mt-px size-3.5 shrink-0" />{t('cvt.gated_note')}</p>
    {/if}
    {#if !plan.env_ready}
      <p class="flex items-start gap-1.5 text-xs text-muted-foreground"><Info class="mt-px size-3.5 shrink-0" />{t('cvt.env_needed', { size: fmt(plan.env_estimated_bytes) })}</p>
    {/if}
    {#if !plan.architectures_checked}
      <p class="flex items-start gap-1.5 text-xs text-muted-foreground"><Info class="mt-px size-3.5 shrink-0" />{t('cvt.unchecked')}</p>
    {/if}
    {#if !enoughDisk && estimate}
      <p class="flex items-start gap-1.5 text-xs text-destructive" role="alert">
        <AlertCircle class="mt-px size-3.5 shrink-0" />{t('cvt.no_disk', { need: fmt(estimate.disk_bytes), free: fmt(plan.free_bytes) })}
      </p>
    {/if}

    <div class="flex items-center gap-3">
      <Button size="sm" disabled={starting || !enoughDisk || !!running} onclick={start}>
        {#if starting}<LoaderCircle class="animate-spin" />{t('cvt.starting')}{:else}<Repeat />{t('cvt.button')}{/if}
      </Button>
      {#if running}
        <span class="text-xs text-muted-foreground">{t(`cvt.stage_${running.stage}` as TKey)}</span>
      {/if}
    </div>
  {/if}
</section>
