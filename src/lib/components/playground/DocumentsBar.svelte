<script lang="ts">
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  // Documents attached to the open conversation (local RAG): one chip per document with its
  // indexing state, a popover per document (use, cancel, retry, remove) and the search settings
  // (embedding model, reranker, excerpts per question).
  import { errorText } from '$lib/errors';
  import * as ToggleGroup from '$lib/components/ui/toggle-group';
  import RagSettings from './RagSettings.svelte';
  import { t, locale, type TKey } from '$lib/i18n.svelte';
  import { cn } from '$lib/utils';
  import { formatBytes } from '$lib/format';
  import * as Popover from '$lib/components/ui/popover';
  import { Button, buttonVariants } from '$lib/components/ui/button';
  import { AlertCircle, FileCode, FileText, LoaderCircle, Settings2, Trash2, RotateCcw, Square, Plus } from '@lucide/svelte';
  import { isActive, resolveEmbedModel, type RagDocument, type RagMode, type RagModels } from './rag';
  import type { PgSettings } from './types';

  interface Props {
    docs: RagDocument[];
    settings: PgSettings;
    models: RagModels | null;
    /** Chat model context (tokens), for the "full text fits" hint. */
    contextTokens: number | null;
    readOnly: boolean;
    onAttach: () => void;
    onRemove: (id: string) => void;
    onCancel: (id: string) => void;
    onRetry: (id: string) => void;
    onMode: (id: string, mode: RagMode) => void;
    /** The embedding choice changed: re-index with it. */
    onEmbedChange: () => void;
    onGoToHub: () => void;
  }

  let {
    docs,
    settings = $bindable(),
    models,
    contextTokens,
    readOnly,
    onAttach,
    onRemove,
    onCancel,
    onRetry,
    onMode,
    onEmbedChange,
    onGoToHub,
  }: Props = $props();

  const DEFAULT_CTX = 4096;
  let ctx = $derived(contextTokens ?? DEFAULT_CTX);
  let effectiveEmbed = $derived(resolveEmbedModel(settings.ragEmbedModel, models?.embedding ?? []));

  function fmt(n: number): string {
    return n.toLocaleString(locale(), { maximumFractionDigits: 0 });
  }

  function statusText(d: RagDocument): string {
    switch (d.status) {
      case 'pending':
        return t('doc.status_pending');
      case 'extracting':
        return t('doc.status_extracting');
      case 'embedding':
        return t('doc.status_embedding', { done: d.embedded, total: d.chunks });
      case 'failed':
        return t('doc.status_failed');
      case 'cancelled':
        return t('doc.status_cancelled');
      default:
        return d.mode === 'full' ? t('doc.mode_full') : d.mode === 'retrieval' ? t('doc.mode_retrieval') : '';
    }
  }

  function fits(d: RagDocument): boolean {
    return d.tokens <= ctx / 2;
  }

  function autoHint(d: RagDocument): string {
    return fits(d)
      ? t('doc.auto_fits', { tokens: fmt(d.tokens), ctx: fmt(ctx) })
      : t('doc.auto_search', { tokens: fmt(d.tokens), ctx: fmt(ctx) });
  }

  function searchHint(d: RagDocument): string {
    if (d.status !== 'ready') return '';
    if (d.embedded > 0 && d.embed_model) {
      return d.embed_model === effectiveEmbed
        ? t('doc.search_semantic', { model: d.embed_model })
        : t('doc.search_stale', { model: d.embed_model });
    }
    return t('doc.search_keyword');
  }


</script>

{#if docs.length > 0}
  <div class="mb-1.5 flex min-w-0 items-start gap-1.5">
    <ul class="m-0 flex max-h-16 min-w-0 flex-1 list-none flex-wrap gap-1.5 overflow-y-auto p-0" aria-label={t('doc.list_label')}>
      {#each docs as d (d.id)}
        {@const active = isActive(d)}
        {@const status = statusText(d)}
        <li class="min-w-0">
          <Popover.Root>
            <Popover.Trigger
              class={cn(
                'relative flex h-7 max-w-56 min-w-0 items-center gap-1.5 overflow-hidden rounded-md border bg-card px-2 text-xs text-foreground outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50',
                d.status === 'failed' ? 'border-destructive/50' : 'border-border'
              )}
              aria-label={t('doc.open', { name: d.name })}
            >
              {#if active}
                <LoaderCircle class="size-3.5 shrink-0 animate-spin text-muted-foreground" aria-hidden="true" />
              {:else if d.status === 'failed'}
                <AlertCircle class="size-3.5 shrink-0 text-destructive" aria-hidden="true" />
              {:else if d.kind === 'code'}
                <FileCode class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
              {:else}
                <FileText class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
              {/if}
              <span class="min-w-0 truncate">{d.name}</span>
              {#if status}
                <span class={cn('shrink-0 font-num', d.status === 'failed' ? 'text-destructive' : 'text-muted-foreground')}>
                  {status}
                </span>
              {/if}
              {#if d.status === 'embedding' && d.chunks > 0}
                <span
                  class="absolute bottom-0 left-0 h-0.5 bg-muted-foreground transition-[width]"
                  style:width={`${Math.round((d.embedded / d.chunks) * 100)}%`}
                  aria-hidden="true"
                ></span>
              {/if}
            </Popover.Trigger>
            <Popover.Content align="start" class="flex w-80 flex-col gap-3 p-3" onCloseAutoFocus={(e) => e.preventDefault()}>
              <div class="flex min-w-0 flex-col gap-0.5">
                <span class="text-sm font-semibold break-all text-foreground">{d.name}</span>
                <span class="text-xs text-muted-foreground">
                  {t(`doc.kind_${d.kind}` as TKey)}
                  {#if d.size}· {formatBytes(d.size, 1)}{/if}
                  {#if d.pages}· {t('doc.pages_count', { n: d.pages })}{/if}
                  <AdvancedOnly>{#if d.chunks}· {t('doc.chunks_count', { n: d.chunks })} · ≈{t('doc.tokens_count', { n: fmt(d.tokens) })}{/if}</AdvancedOnly>
                </span>
              </div>

              {#if d.status === 'failed' && d.error}
                <p class="m-0 rounded-md border border-destructive/40 bg-destructive/10 px-2.5 py-1.5 text-xs text-destructive [overflow-wrap:anywhere]" role="alert">
                  {errorText(d.error)}
                </p>
              {:else if active}
                <p class="m-0 text-xs text-muted-foreground" role="status">{statusText(d)}</p>
              {:else if d.status === 'cancelled'}
                <p class="m-0 text-xs text-muted-foreground">{t('doc.cancelled_desc')}</p>
              {/if}

              {#if d.status === 'ready'}
                <div class="flex flex-col gap-1.5">
                  <span class="text-xs font-medium text-foreground">{t('doc.mode_label')}</span>
                  <ToggleGroup.Root
                    type="single"
                    variant="outline"
                    size="sm"
                    value={d.mode}
                    disabled={readOnly}
                    onValueChange={(v) => {
                      if (v && v !== d.mode) onMode(d.id, v as RagMode);
                    }}
                    class="w-full"
                    aria-label={t('doc.mode_label')}
                  >
                    <ToggleGroup.Item value="auto" class="h-7 flex-1 px-2 text-xs">{t('doc.mode_auto')}</ToggleGroup.Item>
                    <ToggleGroup.Item value="full" class="h-7 flex-1 px-2 text-xs">{t('doc.mode_full')}</ToggleGroup.Item>
                    <ToggleGroup.Item value="retrieval" class="h-7 flex-1 px-2 text-xs">{t('doc.mode_retrieval')}</ToggleGroup.Item>
                  </ToggleGroup.Root>
                  <p class="m-0 text-xs text-muted-foreground">
                    {#if d.mode === 'auto'}
                      {autoHint(d)}
                    {:else if d.mode === 'full'}
                      {t('doc.full_desc', { tokens: fmt(d.tokens) })}
                      {#if !fits(d)}<span class="text-warning">{t('doc.full_too_big', { ctx: fmt(ctx) })}</span>{/if}
                    {:else}
                      {t('doc.retrieval_desc')}
                    {/if}
                  </p>
                  {#if d.mode !== 'full' && fits(d)}
                    <Button variant="outline" size="sm" class="self-start" disabled={readOnly} onclick={() => onMode(d.id, 'full')}>
                      {t('doc.add_full_text')}
                    </Button>
                  {/if}
                  {#if searchHint(d)}
                    <p class="m-0 text-xs text-muted-foreground">{searchHint(d)}</p>
                  {/if}
                </div>
              {/if}

              <div class="flex items-center justify-end gap-1.5">
                {#if active}
                  <Button variant="outline" size="sm" onclick={() => onCancel(d.id)}>
                    <Square class="fill-current" />
                    {t('doc.cancel')}
                  </Button>
                {:else if d.status === 'failed' || d.status === 'cancelled' || (d.status === 'ready' && effectiveEmbed && (d.embed_model !== effectiveEmbed || d.embedded < d.chunks))}
                  <Button variant="outline" size="sm" disabled={readOnly} onclick={() => onRetry(d.id)}>
                    <RotateCcw />
                    {d.status === 'ready' ? t('doc.reindex') : t('doc.retry')}
                  </Button>
                {/if}
                <Button variant="ghost" size="sm" class="text-destructive hover:text-destructive" disabled={readOnly} onclick={() => onRemove(d.id)}>
                  <Trash2 />
                  {t('doc.remove')}
                </Button>
              </div>
            </Popover.Content>
          </Popover.Root>
        </li>
      {/each}
    </ul>

    <Button variant="ghost" size="icon-sm" class="shrink-0" disabled={readOnly} aria-label={t('doc.attach')} title={t('doc.attach')} onclick={onAttach}>
      <Plus />
    </Button>

    <Popover.Root>
      <Popover.Trigger
        class={cn(buttonVariants({ variant: 'ghost', size: 'icon-sm' }), 'shrink-0')}
        aria-label={t('doc.settings')}
        title={t('doc.settings')}
      >
        <Settings2 />
      </Popover.Trigger>
      <Popover.Content align="end" class="flex w-80 flex-col gap-3 p-3" onCloseAutoFocus={(e) => e.preventDefault()}>
        <span class="text-sm font-semibold text-foreground">{t('doc.settings')}</span>

        <RagSettings bind:settings {models} {onEmbedChange} {onGoToHub} />
      </Popover.Content>
    </Popover.Root>
  </div>
{/if}
