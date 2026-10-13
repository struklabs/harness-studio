<script lang="ts">
  import Field from '../dashboard/parts/Field.svelte';
  import { t } from '$lib/i18n.svelte';
  import { invoke } from '$lib/ipc';
  import { toast } from 'svelte-sonner';
  import { localizeError } from '$lib/network';
  import type { GatewayOverview } from '$lib/types';
  import { app } from '$lib/stores/app.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Badge } from '$lib/components/ui/badge';
  import { Checkbox } from '$lib/components/ui/checkbox';
  import { Input } from '$lib/components/ui/input';
  import { Label } from '$lib/components/ui/label';
  import { Textarea } from '$lib/components/ui/textarea';
  import { LoaderCircle, Play, Power, TriangleAlert, Workflow } from '@lucide/svelte';
  import { buildRequest, highlight, isFormTask, parseResponse, runPipeline, type PipelineForm, type PipelineResult } from './pipeline';

  interface Props {
    /** Definition alias sent to `/v1/hf/{model}`. */
    model: string;
    /** Hugging Face pipeline task of the model's file. */
    task: string | undefined;
  }

  let { model, task }: Props = $props();

  let form = $state<PipelineForm>({ text: '', labels: t('pl.sample_labels'), multiLabel: false, question: t('pl.sample_question'), context: t('pl.sample_context') });
  let raw = $state('{\n  "inputs": ""\n}');
  let busy = $state(false);
  let error = $state('');
  let result = $state<{ res: PipelineResult; ms: number; text: string } | null>(null);
  let controller: AbortController | null = null;

  // A sample that fits the task whenever the task changes (and the text is still a sample or empty).
  let lastTask: string | undefined;
  $effect(() => {
    const next = task;
    if (next === lastTask) return;
    lastTask = next;
    result = null;
    error = '';
    form.text =
      next === 'zero-shot-classification' ? t('pl.sample_zero_shot')
      : next === 'token-classification' ? t('pl.sample_ner')
      : next === 'fill-mask' ? t('pl.sample_mask')
      : t('pl.sample_text');
  });

  let body = $derived.by<Record<string, unknown> | null>(() => {
    if (isFormTask(task)) return buildRequest(task, form);
    try {
      const v = JSON.parse(raw) as unknown;
      return v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : null;
    } catch {
      return null;
    }
  });
  let canRun = $derived(!busy && app.gatewayRunning && !!model && body !== null);

  async function run() {
    if (!canRun || !body) return;
    controller?.abort();
    const c = new AbortController();
    controller = c;
    busy = true;
    error = '';
    const text = form.text;
    try {
      const { json, ms } = await runPipeline(app.inferenceUrl, model, body, app.inferenceHeaders(), c.signal);
      result = { res: parseResponse(task, json), ms, text };
    } catch (e) {
      if (!c.signal.aborted) {
        error = e instanceof Error ? e.message : String(e);
        result = null;
      }
    } finally {
      if (controller === c) {
        controller = null;
        busy = false;
      }
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void run();
    }
  }

  let enablingGateway = $state(false);
  /** Same call as the Settings > Gateway switch: the stored settings with `enabled` on. */
  async function enableGateway() {
    enablingGateway = true;
    try {
      const current = app.gateway ?? (await invoke<GatewayOverview>('get_gateway_status'));
      const next = await invoke<GatewayOverview>('set_gateway_settings', {
        settings: { ...current.settings, enabled: true },
      });
      app.setGateway(next);
      toast.success(t('asr.gateway_on'));
    } catch (e) {
      toast.error(t('asr.gateway_on_failed', { error: localizeError(e) }));
    } finally {
      enablingGateway = false;
    }
  }

  const pct = (score: number) => `${Math.max(2, Math.min(100, score * 100)).toFixed(1)}%`;
</script>

<div class="mx-auto flex w-full max-w-[860px] flex-col gap-5 px-4 py-6">
  {#if !app.gatewayRunning}
    <div class="flex items-start gap-2 rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-foreground" role="status">
      <TriangleAlert class="mt-px size-4 shrink-0 text-warning" aria-hidden="true" />
      <span class="min-w-0 flex-1">{t('pl.need_gateway')}</span>
      <Button variant="outline" size="sm" class="shrink-0" disabled={enablingGateway} onclick={enableGateway}>
        {#if enablingGateway}<LoaderCircle class="animate-spin" />{:else}<Power />{/if}
        {t('asr.enable_gateway')}
      </Button>
    </div>
  {/if}
  {#if !model}
    <div class="flex items-start gap-2 rounded-md border border-border bg-muted/40 px-3 py-2 text-xs text-foreground" role="status">
      <TriangleAlert class="mt-px size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
      <span class="min-w-0 flex-1">{t('pl.no_model')}</span>
    </div>
  {/if}

  <section class="flex flex-col gap-3" aria-labelledby="pl-input">
    <div class="flex flex-wrap items-center gap-2">
      <h3 id="pl-input" class="text-sm font-semibold text-foreground">{t('pl.task')}</h3>
      <Badge variant="outline" class="font-mono">{task ?? t('pl.task_unknown')}</Badge>
    </div>

    {#if task === 'question-answering'}
      <div class="flex flex-col gap-1.5">
        <Label for="pl-question" class="text-sm font-medium text-foreground">{t('pl.question_label')}</Label>
        <Input id="pl-question" bind:value={form.question} disabled={busy} onkeydown={(e) => e.key === 'Enter' && run()} />
      </div>
      <div class="flex flex-col gap-1.5">
        <Label for="pl-context" class="text-sm font-medium text-foreground">{t('pl.context_label')}</Label>
        <Textarea id="pl-context" rows={5} bind:value={form.context} disabled={busy} onkeydown={onKey} />
      </div>
    {:else if isFormTask(task)}
      <Field label={t('pl.text_label')} id="pl-text" hint={task === 'fill-mask' ? t('pl.mask_hint') : undefined}>
        <Textarea id="pl-text" rows={4} bind:value={form.text} placeholder={t('pl.text_placeholder')} disabled={busy} onkeydown={onKey} />
      </Field>
      {#if task === 'zero-shot-classification'}
        <Field label={t('pl.labels_label')} id="pl-labels" hint={t('pl.labels_hint')}>
          <Input id="pl-labels" bind:value={form.labels} disabled={busy} />
        </Field>
        <div class="flex items-center gap-2">
          <Checkbox id="pl-multi" bind:checked={form.multiLabel} disabled={busy} />
          <Label for="pl-multi" class="text-sm font-normal">{t('pl.multi_label')}</Label>
        </div>
      {/if}
    {:else}
      <Field label={t('pl.raw_label')} id="pl-raw">
        <Textarea id="pl-raw" rows={8} class="font-mono text-xs" bind:value={raw} disabled={busy} onkeydown={onKey} />
      </Field>
      {#if body === null && raw.trim()}
        <span class="text-xs text-destructive">{t('pl.raw_invalid')}</span>
      {/if}
    {/if}

    <div>
      <Button size="sm" onclick={run} disabled={!canRun}>
        {#if busy}<LoaderCircle class="animate-spin" />{:else}<Play />{/if}
        {busy ? t('pl.running') : t('pl.run')}
      </Button>
    </div>
  </section>

  {#if error}
    <div class="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive [overflow-wrap:anywhere]" role="alert">
      {error}
    </div>
  {/if}

  {#if result}
    {@const res = result.res}
    <section class="flex flex-col gap-3" aria-labelledby="pl-result">
      <div class="flex flex-wrap items-center gap-2">
        <h3 id="pl-result" class="flex items-center gap-1 text-sm font-semibold text-foreground"><Workflow class="size-4" aria-hidden="true" />{t('pl.result')}</h3>
        <Badge variant="outline" class="font-num">{t('pl.took', { ms: Math.round(result.ms) })}</Badge>
      </div>

      {#if res.kind === 'labels'}
        <ul class="flex flex-col gap-2">
          {#each res.items as item (item.label)}
            <li class="flex flex-col gap-1">
              <div class="flex items-center justify-between gap-3 text-xs">
                <span class="min-w-0 truncate text-foreground">{item.label}</span>
                <span class="font-num text-muted-foreground">{item.score.toFixed(3)}</span>
              </div>
              <div class="h-1.5 w-full overflow-hidden rounded-full bg-muted" aria-hidden="true">
                <div class="h-full rounded-full bg-primary" style:width={pct(item.score)}></div>
              </div>
            </li>
          {/each}
        </ul>
      {:else if res.kind === 'entities'}
        {#if res.items.length === 0}
          <p class="text-xs text-muted-foreground">{t('pl.no_entities')}</p>
        {:else}
          <p class="rounded-md border border-border px-3 py-2 text-sm leading-7 text-foreground [overflow-wrap:anywhere]">
            {#each highlight(result.text.trim(), res.items) as piece, i (i)}
              {#if piece.entity}
                <mark class="rounded bg-primary/15 px-1 text-foreground">{piece.text}<span class="ml-1 font-mono text-[10px] font-semibold text-primary">{piece.entity}</span></mark>
              {:else}{piece.text}{/if}
            {/each}
          </p>
          <table class="w-full border-collapse overflow-hidden rounded-lg border border-border text-xs">
            <tbody>
              {#each res.items as e, i (i)}
                <tr class="border-t border-border first:border-t-0">
                  <td class="w-24 px-3 py-1.5 font-mono font-semibold text-foreground">{e.entity}</td>
                  <td class="px-3 py-1.5 text-foreground [overflow-wrap:anywhere]">{e.word}</td>
                  <td class="w-20 px-3 py-1.5 text-right font-num text-muted-foreground">{e.score.toFixed(3)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      {:else if res.kind === 'fills'}
        <ul class="flex flex-col gap-2">
          {#each res.items as f, i (i)}
            <li class="flex flex-col gap-1 rounded-md border border-border px-3 py-2">
              <div class="flex items-center justify-between gap-3 text-xs">
                <span class="font-mono font-semibold text-foreground">{f.token}</span>
                <span class="font-num text-muted-foreground">{f.score.toFixed(3)}</span>
              </div>
              <span class="text-xs text-muted-foreground [overflow-wrap:anywhere]">{f.sequence}</span>
            </li>
          {/each}
        </ul>
      {:else if res.kind === 'answers'}
        <ul class="flex flex-col gap-2">
          {#each res.items as a, i (i)}
            <li class="flex items-center justify-between gap-3 rounded-md border border-border px-3 py-2 text-sm">
              <span class="min-w-0 text-foreground [overflow-wrap:anywhere]"><span class="mr-2 text-xs text-muted-foreground">{t('pl.answer')}</span>{a.answer}</span>
              <span class="shrink-0 font-num text-xs text-muted-foreground">{t('pl.score')} {a.score.toFixed(3)}</span>
            </li>
          {/each}
        </ul>
      {:else}
        <pre class="max-h-96 overflow-auto rounded-md border border-border bg-muted/30 p-3 font-mono text-xs text-foreground">{JSON.stringify(res.value, null, 2)}</pre>
      {/if}
    </section>
  {/if}
</div>
