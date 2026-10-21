<script lang="ts">
  import { fixed } from '#lib/format.js';
  import HelpPopover from '../common/HelpPopover.svelte';
  import Field from '../dashboard/parts/Field.svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import { ListOrdered, TriangleAlert } from '@lucide/svelte';
  import TextListInput from './TextListInput.svelte';
  import { InvalidResponseError, fetchRerank, type RerankResult } from './embed';

  interface Props {
    /** Model id sent with the request (the Playground's selected alias). */
    model: string;
  }

  let { model }: Props = $props();

  let query = $state(t('rr.sample_query'));
  let docs = $state<string[]>([t('rr.sample_1'), t('rr.sample_2'), t('rr.sample_3')]);
  let busy = $state(false);
  let error = $state('');
  let result = $state<{ res: RerankResult; docs: string[] } | null>(null);
  let controller: AbortController | null = null;

  let mode = $derived(app.activeServer?.serve_mode ?? 'chat');
  // The selected server is itself the rerank server: talk to it directly (its own endpoint and key).
  // The Playground's chat model must never be sent to it, and the gateway is not needed.
  let direct = $derived(app.isRunning && mode === 'rerank');
  let filled = $derived(docs.map((s) => s.trim()).filter(Boolean));
  let canRun = $derived(!busy && app.isRunning && query.trim() !== '' && filled.length > 0);
  /** Bar widths: scores are raw logits, so they are scaled between this ranking's min and max. */
  let range = $derived.by(() => {
    const scores = result?.res.items.map((i) => i.score) ?? [];
    return { min: Math.min(...scores), max: Math.max(...scores) };
  });

  async function run() {
    if (!canRun) return;
    controller?.abort();
    const c = new AbortController();
    controller = c;
    busy = true;
    error = '';
    const documents = [...filled];
    try {
      result = {
        res: await fetchRerank(direct ? app.baseUrl : app.inferenceUrl, query.trim(), documents, direct ? '' : model, direct ? app.authHeaders() : app.inferenceHeaders(), c.signal),
        docs: documents,
      };
    } catch (e) {
      if (!c.signal.aborted) {
        error = e instanceof InvalidResponseError ? t('rr.invalid_response') : e instanceof Error ? e.message : String(e);
        result = null;
      }
    } finally {
      if (controller === c) {
        controller = null;
        busy = false;
      }
    }
  }

  function width(score: number): number {
    const span = range.max - range.min;
    return span > 0 ? 8 + ((score - range.min) / span) * 92 : 100;
  }
</script>

<div class="mx-auto flex w-full max-w-[860px] flex-col gap-5 px-4 py-6">
  {#if app.isRunning && mode !== 'rerank'}
    <div class="flex items-start gap-2 rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-foreground" role="status">
      <TriangleAlert class="mt-px size-4 shrink-0 text-warning" aria-hidden="true" />
      <span>{t('emb.not_rerank_server', { mode: t(`emb.mode_${mode}`) })}</span>
    </div>
  {/if}

  <section class="flex flex-col gap-3" aria-labelledby="rr-input">
    <h3 id="rr-input" class="sr-only">{t('rr.docs_label')}</h3>
    <div class="flex flex-col gap-1.5">
      <Label for="rr-query" class="text-sm font-semibold text-foreground">{t('rr.query_label')}</Label>
      <Input
        id="rr-query"
        bind:value={query}
        placeholder={t('rr.query_placeholder')}
        disabled={busy}
        onkeydown={(e) => {
          if (e.key === 'Enter') run();
        }}
      />
    </div>
    <Field label={t('rr.docs_label')} hint={t('rr.docs_desc')}>
    <TextListInput
      bind:items={docs}
      placeholder={(n) => t('rr.doc_placeholder', { n })}
      addLabel={t('rr.add_doc')}
      removeLabel={t('rr.remove_doc')}
      disabled={busy}
    />
    </Field>
    <div class="flex items-center gap-3 pl-7">
      <Button size="sm" onclick={run} disabled={!canRun}>
        <ListOrdered />
        {busy ? t('rr.running') : t('rr.run')}
      </Button>
      {#if query.trim() === '' || filled.length === 0}
        <span class="text-xs text-muted-foreground">{t('rr.need_input')}</span>
      {/if}
    </div>
  </section>

  {#if error}
    <div class="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive [overflow-wrap:anywhere]" role="alert">
      {error}
    </div>
  {/if}

  {#if result}
    <section class="flex flex-col gap-3" aria-labelledby="rr-result">
      <div class="flex flex-wrap items-center gap-2">
        <h3 id="rr-result" class="flex items-center gap-1 text-sm font-semibold text-foreground">{t('rr.result_title')}<HelpPopover text={t('rr.score_desc')} /></h3>
        <Badge variant="outline" class="font-num">{t('emb.stat_time', { value: Math.round(result.res.ms) })}</Badge>
        {#if result.res.promptTokens !== null}
          <Badge variant="outline" class="font-num">{t('emb.stat_tokens', { n: result.res.promptTokens })}</Badge>
        {/if}
        {#if result.res.model}
          <span class="min-w-0 truncate font-mono text-xs text-muted-foreground" title={result.res.model}>{result.res.model}</span>
        {/if}
      </div>
      <div class="overflow-hidden rounded-lg border border-border">
        <table class="w-full border-collapse text-xs">
          <thead class="bg-muted/40 text-muted-foreground">
            <tr>
              <th scope="col" class="w-12 px-3 py-1.5 text-left font-medium">{t('rr.col_rank')}</th>
              <th scope="col" class="w-40 px-3 py-1.5 text-left font-medium">{t('rr.col_score')}</th>
              <th scope="col" class="px-3 py-1.5 text-left font-medium">{t('rr.col_doc')}</th>
            </tr>
          </thead>
          <tbody>
            {#each result.res.items as item, rank (item.index)}
              <tr class="border-t border-border align-top">
                <td class="px-3 py-2 font-num text-muted-foreground">{rank + 1}</td>
                <td class="px-3 py-2">
                  <div class="flex flex-col gap-1">
                    <span class="font-num text-foreground">{fixed(item.score, 3)}</span>
                    <div class="h-1 w-full overflow-hidden rounded-full bg-muted" aria-hidden="true">
                      <div class="h-full rounded-full bg-muted-foreground" style:width={`${width(item.score)}%`}></div>
                    </div>
                  </div>
                </td>
                <td class="px-3 py-2 text-foreground [overflow-wrap:anywhere]">
                  <span class="mr-1 font-num text-xs text-muted-foreground">#{item.index + 1}</span>{result.docs[item.index] ?? ''}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>
  {/if}
</div>
