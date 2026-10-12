<script lang="ts">
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  import { t, locale } from '$lib/i18n.svelte';
  import { cn } from '$lib/utils';
  import { errorText } from '$lib/errors';
  import * as Collapsible from '$lib/components/ui/collapsible';
  import { ChevronRight, FileText } from '@lucide/svelte';
  import { citedNumbers, pageRange, type RagSource, type RagTurn } from './rag';

  interface Props {
    rag: RagTurn;
    /** The answer text (to mark the excerpts it cites). */
    answer: string;
  }

  let { rag, answer }: Props = $props();

  let open = $state(false);
  let expanded = $state<Record<number, boolean>>({});
  let cited = $derived(citedNumbers(answer));
  let excerpts = $derived(rag.sources.filter((s) => !s.full));
  let whole = $derived(rag.sources.filter((s) => s.full));

  function pages(s: RagSource): string {
    const r = pageRange(s.page_start, s.page_end);
    if (!r) return '';
    return 'one' in r ? t('doc.page_one', { n: r.one }) : t('doc.page_range', { from: r.from, to: r.to });
  }

  function score(v: number): string {
    return v.toLocaleString(locale(), { maximumFractionDigits: 2, minimumFractionDigits: 2 });
  }

  let summary = $derived.by(() => {
    const parts: string[] = [];
    if (excerpts.length) parts.push(t('doc.sources_excerpts', { n: excerpts.length }));
    if (whole.length) parts.push(t('doc.sources_full', { n: whole.length }));
    return parts.join(' · ');
  });

  let method = $derived(
    rag.retrieval === 'hybrid'
      ? rag.reranked
        ? t('doc.method_hybrid_rerank')
        : t('doc.method_hybrid')
      : rag.retrieval === 'keyword'
        ? t('doc.method_keyword')
        : ''
  );
</script>

{#if rag.sources.length || rag.notices.length}
  <Collapsible.Root bind:open>
    <Collapsible.Trigger
      class="group flex max-w-full items-center gap-1.5 rounded-sm text-xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
    >
      <ChevronRight class="size-3.5 shrink-0 transition-transform group-data-[state=open]:rotate-90" aria-hidden="true" />
      <FileText class="size-3.5 shrink-0" aria-hidden="true" />
      <span class="font-medium">{summary || t('doc.sources_none')}</span>
      {#if method}<span class="truncate">· {method}</span>{/if}
    </Collapsible.Trigger>
    <Collapsible.Content>
      <ol class="mt-2 flex list-none flex-col gap-1.5 p-0" aria-label={t('doc.sources_label')}>
        {#each whole as s (s.document_id)}
          <li class="flex items-center gap-2 rounded-md border border-border bg-card px-2.5 py-1.5 text-xs">
            <span class="shrink-0 rounded-sm bg-muted px-1.5 py-0.5 text-muted-foreground">{t('doc.full_badge')}</span>
            <span class="min-w-0 truncate font-medium text-foreground" title={s.document_name}>{s.document_name}</span>
            {#if s.page_end}<span class="shrink-0 text-muted-foreground">{t('doc.pages_count', { n: s.page_end })}</span>{/if}
          </li>
        {/each}
        {#each excerpts as s (s.n)}
          {@const isCited = cited.has(s.n)}
          {@const long = s.text.length > 220}
          <li class="flex min-w-0 flex-col gap-1 rounded-md border border-border bg-card px-2.5 py-2 text-xs">
            <div class="flex min-w-0 items-center gap-2">
              <span
                class={cn(
                  'shrink-0 rounded-sm px-1.5 py-0.5 font-num',
                  isCited ? 'bg-muted text-foreground' : 'bg-muted text-muted-foreground'
                )}
                title={isCited ? t('doc.cited') : t('doc.not_cited')}
              >
                [{s.n}]
              </span>
              <span class="min-w-0 truncate font-medium text-foreground" title={s.document_name}>{s.document_name}</span>
              {#if pages(s)}<span class="shrink-0 text-muted-foreground">{pages(s)}</span>{/if}
              <AdvancedOnly>{#if s.rerank_score != null || s.similarity != null}
                <span
                  class="ml-auto shrink-0 font-num text-muted-foreground"
                  title={s.rerank_score != null ? t('doc.score_rerank') : t('doc.score_similarity')}
                >
                  {score(s.rerank_score ?? s.similarity ?? 0)}
                </span>
              {/if}</AdvancedOnly>
            </div>
            {#if s.heading}
              <span class="truncate text-muted-foreground" title={s.heading}>{s.heading}</span>
            {/if}
            <p
              class={cn(
                'm-0 leading-relaxed whitespace-pre-wrap text-foreground/85 [overflow-wrap:anywhere]',
                long && !expanded[s.n] && 'line-clamp-3'
              )}
            >
              {s.text}
            </p>
            {#if long}
              <button
                type="button"
                class="self-start rounded-sm text-xs text-muted-foreground underline-offset-2 outline-none hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring/50"
                aria-expanded={!!expanded[s.n]}
                onclick={() => (expanded[s.n] = !expanded[s.n])}
              >
                {expanded[s.n] ? t('doc.show_less') : t('doc.show_more')}
              </button>
            {/if}
          </li>
        {/each}
      </ol>
      {#each rag.notices as n (n)}
        <p class="m-0 mt-1.5 text-xs text-muted-foreground [overflow-wrap:anywhere]">{errorText(n)}</p>
      {/each}
    </Collapsible.Content>
  </Collapsible.Root>
{/if}
