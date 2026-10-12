<script lang="ts">
  import HelpPopover from '../common/HelpPopover.svelte';
  import Field from '../dashboard/parts/Field.svelte';
  import { t } from '$lib/i18n.svelte';
  import { app } from '$lib/stores/app.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Badge } from '$lib/components/ui/badge';
  import { Play, TriangleAlert } from '@lucide/svelte';
  import TextListInput from './TextListInput.svelte';
  import { InvalidResponseError, fetchEmbeddings, similarityMatrix, type EmbeddingResult } from './embed';

  interface Props {
    /** Model id sent with the request (the Playground's selected alias). */
    model: string;
  }

  let { model }: Props = $props();

  let texts = $state<string[]>([t('emb.sample_1'), t('emb.sample_2'), t('emb.sample_3')]);
  let busy = $state(false);
  let error = $state('');
  let result = $state<{ res: EmbeddingResult; texts: string[] } | null>(null);
  let controller: AbortController | null = null;

  let mode = $derived(app.activeServer?.serve_mode ?? 'chat');
  // The selected server is itself the embedding server: talk to it directly (its own endpoint and key).
  // The Playground's chat model must never be sent to it, and the gateway is not needed.
  let direct = $derived(app.isRunning && mode === 'embedding');
  let filled = $derived(texts.map((s) => s.trim()).filter(Boolean));
  let matrix = $derived(result ? similarityMatrix(result.res.vectors) : []);

  async function run() {
    if (busy || filled.length < 2 || !app.isRunning) return;
    controller?.abort();
    const c = new AbortController();
    controller = c;
    busy = true;
    error = '';
    const input = [...filled];
    try {
      result = { res: await fetchEmbeddings(direct ? app.baseUrl : app.inferenceUrl, input, direct ? '' : model, direct ? app.authHeaders() : app.inferenceHeaders(), c.signal), texts: input };
    } catch (e) {
      if (!c.signal.aborted) {
        error = e instanceof InvalidResponseError ? t('emb.invalid_response') : e instanceof Error ? e.message : String(e);
        result = null;
      }
    } finally {
      if (controller === c) {
        controller = null;
        busy = false;
      }
    }
  }

  function short(s: string): string {
    return s.length > 48 ? `${s.slice(0, 48)}…` : s;
  }

  /** Cell tint: similarity 0..1 mapped to 0..70 % of the neutral foreground. */
  function tint(v: number): string {
    const p = Math.round(Math.max(0, Math.min(1, v)) * 70);
    return `background-color: color-mix(in oklab, var(--muted-foreground) ${p}%, transparent)`;
  }
</script>

<div class="mx-auto flex w-full max-w-[860px] flex-col gap-5 px-4 py-6">
  {#if app.isRunning && mode !== 'embedding'}
    <div class="flex items-start gap-2 rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-foreground" role="status">
      <TriangleAlert class="mt-px size-4 shrink-0 text-warning" aria-hidden="true" />
      <span>{t('emb.not_embedding_server', { mode: t(`emb.mode_${mode}`) })}</span>
    </div>
  {/if}

  <section class="flex flex-col gap-2" aria-label={t('emb.texts_label')}>
    <Field label={t('emb.texts_label')} hint={t('emb.texts_desc')}>
    <TextListInput
      bind:items={texts}
      placeholder={(n) => t('emb.text_placeholder', { n })}
      addLabel={t('emb.add_text')}
      removeLabel={t('emb.remove_text')}
      disabled={busy}
    />
    </Field>
    <div class="flex items-center gap-3 pl-7">
      <Button size="sm" onclick={run} disabled={busy || filled.length < 2 || !app.isRunning}>
        <Play />
        {busy ? t('emb.running') : t('emb.run')}
      </Button>
      {#if filled.length < 2}
        <span class="text-xs text-muted-foreground">{t('emb.need_two')}</span>
      {/if}
    </div>
  </section>

  {#if error}
    <div class="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive [overflow-wrap:anywhere]" role="alert">
      {error}
    </div>
  {/if}

  {#if result}
    <section class="flex flex-col gap-3" aria-labelledby="emb-result">
      <div class="flex flex-wrap items-center gap-2">
        <h3 id="emb-result" class="text-sm font-semibold text-foreground">{t('emb.result_title')}</h3>
        <Badge variant="secondary" class="font-num">{t('emb.stat_dims', { n: result.res.dims })}</Badge>
        <Badge variant="outline" class="font-num">{t('emb.stat_count', { n: result.res.vectors.length })}</Badge>
        <Badge variant="outline" class="font-num">{t('emb.stat_time', { value: Math.round(result.res.ms) })}</Badge>
        {#if result.res.promptTokens !== null}
          <Badge variant="outline" class="font-num">{t('emb.stat_tokens', { n: result.res.promptTokens })}</Badge>
        {/if}
        {#if result.res.model}
          <span class="min-w-0 truncate font-mono text-xs text-muted-foreground" title={result.res.model}>{result.res.model}</span>
        {/if}
      </div>

      <div class="flex flex-col gap-1">
        <h4 class="flex items-center gap-1 text-sm font-medium text-foreground">{t('emb.matrix_title')}<HelpPopover text={t('emb.matrix_desc')} /></h4>
      </div>
      <div class="overflow-x-auto rounded-lg border border-border">
        <table class="w-full border-collapse text-xs" aria-label={t('emb.matrix_label')}>
          <thead>
            <tr>
              <th class="w-10 bg-muted/40 px-2 py-1.5"></th>
              {#each result.texts as txt, j (j)}
                <th scope="col" class="bg-muted/40 px-2 py-1.5 text-center font-num font-medium text-muted-foreground" title={txt}>{j + 1}</th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each matrix as row, i (i)}
              <tr class="border-t border-border">
                <th scope="row" class="bg-muted/40 px-2 py-1.5 text-left font-num font-medium text-muted-foreground" title={result.texts[i]}>{i + 1}</th>
                {#each row as v, j (j)}
                  <td class="px-2 py-1.5 text-center font-num text-foreground" style={tint(v)} title={`${i + 1} ↔ ${j + 1}: ${v.toFixed(4)}`}>
                    {v.toFixed(3)}
                  </td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      <ol class="flex flex-col gap-0.5 text-xs text-muted-foreground">
        {#each result.texts as txt, i (i)}
          <li class="min-w-0 truncate" title={txt}><span class="font-num">{i + 1}.</span> {short(txt)}</li>
        {/each}
      </ol>
    </section>
  {/if}
</div>
