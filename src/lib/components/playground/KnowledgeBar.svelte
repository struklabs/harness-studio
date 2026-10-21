<script lang="ts">
  // Knowledge bases of the Playground: a popover to create a base from a folder or files, watch
  // its indexing (progress, cancel, measured time), rebuild a stale one and attach bases to the
  // open conversation; attached bases show as chips above the composer. The recommended small
  // multilingual embedding model installs from here when the library has none.
  import { untrack } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { toast } from 'svelte-sonner';
  import { errorText } from '#lib/errors.js';
  import { t, locale } from '#lib/i18n.svelte.js';
  import { cn } from '#lib/utils.js';
  import { formatBytes } from '#lib/format.js';
  import { domain } from '#lib/stores/domain.svelte.js';
  import * as Popover from '#lib/components/ui/popover/index.js';
  import { Button, buttonVariants } from '#lib/components/ui/button/index.js';
  import { Progress } from '#lib/components/ui/progress/index.js';
  import { BookOpen, Cpu, Download, FilePlus, FolderOpen, LoaderCircle, RotateCcw, Square, Trash2, TriangleAlert, X } from '@lucide/svelte';
  import { ragApi, type RagModels } from './rag';
  import {
    baseNameOf,
    embedderApi,
    embedderBusy,
    formatIndexTime,
    kbApi,
    kbUsable,
    runFraction,
    type EmbedderStatus,
    type KbProgress,
    type KnowledgeBase,
  } from './knowledge';

  interface Props {
    conversationId: string | null;
    /** Creates the conversation on demand (a base can be attached before the first message). */
    ensureConversation: (title: string) => Promise<string | null>;
    /** Embedding alias in use (the selected one, whether or not the gateway runs). */
    embedAlias: string | null;
    models: RagModels | null;
    /** Aliases of the library's OCR-capable models (for a base's OCR model). */
    ocrAliases?: string[];
    readOnly: boolean;
    /** Bases attached to the open conversation. */
    attached: KnowledgeBase[];
    /** The installed recommended embedder should be used for new indexes. */
    onUseEmbedder: (alias: string) => void;
  }

  let { conversationId, ensureConversation, embedAlias, models, ocrAliases = [], readOnly, attached = $bindable(), onUseEmbedder }: Props = $props();

  let open = $state(false);
  let bases = $state<KnowledgeBase[]>([]);
  let progress = $state<Record<string, KbProgress>>({});
  let embedder = $state<EmbedderStatus | null>(null);
  let confirmDelete = $state<string | null>(null);
  let busy = $state(false);

  let hasEmbedding = $derived((models?.embedding.length ?? 0) > 0);
  let downloading = $derived(embedderBusy(embedder));
  let downloadPercent = $derived(embedder && embedder.size_bytes ? Math.min(100, Math.round((embedder.downloaded_bytes / embedder.size_bytes) * 100)) : 0);

  function fmt(n: number): string {
    return n.toLocaleString(locale(), { maximumFractionDigits: 0 });
  }

  let listRequest = 0;
  async function refresh() {
    const req = ++listRequest;
    try {
      const list = await kbApi.list(embedAlias);
      if (req === listRequest) bases = list;
    } catch (e) {
      toast.error(errorText(e));
    }
    await refreshAttached();
  }

  let attachedRequest = 0;
  async function refreshAttached() {
    const req = ++attachedRequest;
    const id = conversationId;
    if (!id) {
      attached = [];
      return;
    }
    try {
      const list = await kbApi.attached(id, embedAlias);
      if (req === attachedRequest && id === conversationId) attached = list;
    } catch {
      // The chips stay as they were.
    }
  }

  // Attached bases follow the conversation and the embedding model in use.
  $effect(() => {
    void conversationId;
    void embedAlias;
    untrack(() => void refresh());
  });

  $effect(() => {
    if (open) untrack(() => void loadEmbedder());
  });

  $effect(() => {
    const un = listen<KbProgress>('kb-progress', (e) => {
      const p = e.payload;
      progress = { ...progress, [p.kb_id]: p };
      if (p.status !== 'indexing') {
        void refresh();
        const { [p.kb_id]: _, ...rest } = progress;
        progress = rest;
      } else if (p.docs_done === 0 && p.docs_total === 0) {
        void refresh();
      }
    });
    return () => void un.then((f) => f());
  });

  async function loadEmbedder() {
    try {
      embedder = await embedderApi.status();
    } catch {
      embedder = null;
    }
  }

  // While the model downloads: poll its status; when it is complete make its definition (pinned
  // to the CPU), let the Playground pick it up and use it.
  $effect(() => {
    if (!downloading) return;
    const timer = setInterval(async () => {
      await loadEmbedder();
      if (embedder?.installed) {
        clearInterval(timer);
        await finishInstall();
      } else if (!embedderBusy(embedder)) {
        clearInterval(timer);
      }
    }, 1500);
    return () => clearInterval(timer);
  });

  async function finishInstall() {
    try {
      const alias = (await embedderApi.sync()) ?? embedder?.alias ?? null;
      await domain.refreshDefinitions();
      if (alias) onUseEmbedder(alias);
      await loadEmbedder();
      if (alias) await reembedKeywordOnly(alias);
    } catch (e) {
      toast.error(errorText(e));
    }
  }

  /** Bases indexed before an embedding model existed are searched by keyword only: once the
   *  model is in place they are embedded again, so the new model is actually used. */
  async function reembedKeywordOnly(alias: string) {
    const list = await kbApi.list(alias);
    const keywordOnly = list.filter((kb) => kb.state === 'stale' && !kb.embed_model && !isRunning(kb));
    if (keywordOnly.length === 0) return;
    toast.info(t('kb.reembed_started', { count: keywordOnly.length }));
    for (const kb of keywordOnly) await startIndex(kb.id, { embedModel: alias, rebuild: true });
  }

  async function installEmbedder() {
    try {
      embedder = await embedderApi.install();
    } catch (e) {
      toast.error(errorText(e));
    }
  }

  function isRunning(kb: KnowledgeBase): boolean {
    return kb.state === 'indexing' || !!progress[kb.id];
  }

  async function startIndex(kbId: string, opts: { folder?: string | null; paths?: string[]; rebuild?: boolean; embedModel?: string | null }) {
    try {
      await kbApi.index(kbId, { embedModel: embedAlias, ...opts });
      progress = { ...progress, [kbId]: { kb_id: kbId, status: 'indexing', docs_done: 0, docs_total: 0, current: null, chunk_done: 0, chunk_total: 0, elapsed_ms: 0 } };
    } catch (e) {
      toast.error(errorText(e));
    }
  }

  async function createFromFolder() {
    if (busy) return;
    busy = true;
    try {
      const folder = await kbApi.pickFolder();
      if (!folder) return;
      const preview = await kbApi.scanFolder(folder);
      if (preview.files === 0) {
        toast.error(t('kb.folder_empty'));
        return;
      }
      const kb = await kbApi.create(baseNameOf(folder), folder);
      await refresh();
      await startIndex(kb.id, {});
    } catch (e) {
      toast.error(errorText(e));
    } finally {
      busy = false;
    }
  }

  async function createFromFiles() {
    if (busy) return;
    busy = true;
    try {
      const files = await ragApi.pick(t('doc.picker_filter')).catch(() => [] as string[]);
      if (!files.length) return;
      const kb = await kbApi.create(files.length === 1 ? baseNameOf(files[0]) : t('kb.default_name', { name: baseNameOf(files[0]) }), null);
      await refresh();
      await startIndex(kb.id, { paths: files });
    } catch (e) {
      toast.error(errorText(e));
    } finally {
      busy = false;
    }
  }

  async function toggleAttach(kb: KnowledgeBase) {
    const on = attached.some((a) => a.id === kb.id);
    try {
      if (on) {
        if (conversationId) await kbApi.detach(conversationId, kb.id);
      } else {
        const id = conversationId ?? (await ensureConversation(kb.name));
        if (!id) return;
        await kbApi.attach(id, kb.id);
        // A new conversation id arrives through the prop; list for the id used here right away.
        attached = [...attached.filter((a) => a.id !== kb.id), kb];
      }
      await refreshAttached();
    } catch (e) {
      toast.error(errorText(e));
    }
  }

  async function remove(kb: KnowledgeBase) {
    if (confirmDelete !== kb.id) {
      confirmDelete = kb.id;
      return;
    }
    confirmDelete = null;
    try {
      await kbApi.remove(kb.id);
      await refresh();
    } catch (e) {
      toast.error(errorText(e));
    }
  }

  async function setOcrModel(kb: KnowledgeBase, alias: string) {
    try {
      await kbApi.setOcrModel(kb.id, alias || null);
      await refresh();
    } catch (e) {
      toast.error(errorText(e));
    }
  }

  function stateLabel(kb: KnowledgeBase): string {
    return t(`kb.state_${kb.state}` as 'kb.state_ready');
  }

  function metaText(kb: KnowledgeBase): string {
    const parts = [t('kb.meta', { docs: fmt(kb.documents), chunks: fmt(kb.chunks) })];
    if (kb.ocr_pages > 0) parts.push(t('kb.ocr_pages', { pages: fmt(kb.ocr_pages) }));
    if (kb.indexed_at) {
      parts.push(
        t('kb.indexed_in', {
          time: formatIndexTime(kb.index_ms, locale()),
          model: kb.embed_model ?? t('kb.keyword_only'),
        })
      );
    }
    return parts.join(' · ');
  }

  function warning(kb: KnowledgeBase): string {
    if (kb.state === 'stale') {
      return kb.stale_reason === 'dims'
        ? t('kb.stale_dims')
        : t('kb.stale_model', { built: kb.embed_model ?? t('kb.keyword_only'), current: embedAlias ?? t('kb.keyword_only') });
    }
    if (kb.state === 'incomplete') return t('kb.incomplete_desc', { failed: kb.failed });
    return '';
  }
</script>

<div class="mb-1.5 flex min-w-0 items-start gap-1.5">
  <Popover.Root bind:open>
    <Popover.Trigger
      class={cn(buttonVariants({ variant: 'ghost', size: 'sm' }), 'h-7 shrink-0 gap-1.5 px-2 text-xs')}
      aria-label={t('kb.open')}
      title={t('kb.open')}
      disabled={readOnly}
    >
      <BookOpen class="size-3.5" aria-hidden="true" />
      {t('kb.title')}
    </Popover.Trigger>
    <Popover.Content align="start" class="flex w-[26rem] max-w-[90vw] flex-col gap-3 p-3" onCloseAutoFocus={(e) => e.preventDefault()}>
      <span class="text-sm font-semibold text-foreground">{t('kb.title')}</span>

      <div class="flex flex-col gap-1.5 rounded-md border border-border bg-muted/40 p-2.5">
        <span class="text-xs font-medium text-foreground">{t('kb.embedder_title')}</span>
        {#if downloading}
          <p class="m-0 text-xs text-muted-foreground" role="status">{t('kb.embedder_downloading', { pct: downloadPercent })}</p>
          <Progress value={downloadPercent} />
        {:else if !hasEmbedding && !embedder?.installed}
          <p class="m-0 text-xs text-muted-foreground">{t('kb.embedder_missing', { size: formatBytes(embedder?.size_bytes ?? 132439008, 0) })}</p>
          <Button variant="outline" size="sm" class="self-start" onclick={installEmbedder}>
            <Download />
            {t('kb.embedder_install')}
          </Button>
        {:else if embedAlias}
          <p class="m-0 flex items-center gap-1.5 text-xs text-muted-foreground">
            <Cpu class="size-3.5 shrink-0" aria-hidden="true" />
            <span class="min-w-0 truncate">{t('kb.embedder_using', { alias: embedAlias })}</span>
          </p>
        {:else}
          <p class="m-0 text-xs text-muted-foreground">{t('kb.embedder_off')}</p>
        {/if}
      </div>

      {#if bases.length === 0}
        <p class="m-0 text-xs text-muted-foreground">{t('kb.empty')}</p>
      {:else}
        <ul class="m-0 flex max-h-80 list-none flex-col gap-2 overflow-y-auto p-0" aria-label={t('kb.list_label')}>
          {#each bases as kb (kb.id)}
            {@const run = progress[kb.id]}
            {@const running = isRunning(kb)}
            {@const isOn = attached.some((a) => a.id === kb.id)}
            {@const note = warning(kb)}
            <li class="flex min-w-0 flex-col gap-1.5 rounded-md border border-border p-2.5">
              <div class="flex min-w-0 items-center gap-2">
                <span class="min-w-0 flex-1 truncate text-sm font-medium text-foreground" title={kb.folder ?? kb.name}>{kb.name}</span>
                <span class={cn('shrink-0 text-xs', kb.state === 'stale' || kb.state === 'incomplete' ? 'text-warning' : 'text-muted-foreground')}>{stateLabel(kb)}</span>
              </div>
              <span class="text-xs text-muted-foreground [overflow-wrap:anywhere]">{metaText(kb)}</span>
              <label class="flex min-w-0 items-center gap-2 text-xs text-muted-foreground" title={t('kb.ocr_hint')}>
                <span class="shrink-0">{t('kb.ocr_model')}</span>
                <select
                  class="h-7 min-w-0 flex-1 rounded-md border border-input bg-background px-2 text-xs text-foreground"
                  value={kb.ocr_model ?? ''}
                  disabled={running || readOnly}
                  onchange={(e) => void setOcrModel(kb, e.currentTarget.value)}
                >
                  <option value="">{t('kb.ocr_none')}</option>
                  {#if kb.ocr_model && !ocrAliases.includes(kb.ocr_model)}<option value={kb.ocr_model}>{kb.ocr_model}</option>{/if}
                  {#each ocrAliases as alias (alias)}<option value={alias}>{alias}</option>{/each}
                </select>
              </label>

              {#if running}
                <Progress value={Math.round(runFraction(run) * 100)} />
                <p class="m-0 text-xs text-muted-foreground [overflow-wrap:anywhere]" role="status">
                  {t('kb.progress', { done: run?.docs_done ?? 0, total: run?.docs_total ?? 0 })}
                  {#if run?.current}· {run.current}{#if run.chunk_total}&nbsp;({run.chunk_done}/{run.chunk_total}){/if}{/if}
                </p>
              {:else if note}
                <p class="m-0 flex items-start gap-1.5 text-xs text-warning">
                  <TriangleAlert class="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
                  <span>{note}</span>
                </p>
              {/if}

              <div class="flex flex-wrap items-center justify-end gap-1.5">
                {#if running}
                  <Button variant="outline" size="sm" onclick={() => void kbApi.cancel(kb.id)}>
                    <Square class="fill-current" />
                    {t('kb.cancel')}
                  </Button>
                {:else}
                  {#if kb.state === 'stale' || kb.state === 'incomplete' || (kb.state === 'empty' && kb.folder)}
                    <Button variant="outline" size="sm" onclick={() => startIndex(kb.id, { rebuild: kb.state === 'stale' })}>
                      <RotateCcw />
                      {kb.state === 'stale' ? t('kb.rebuild') : kb.state === 'empty' ? t('kb.index') : t('kb.resume')}
                    </Button>
                  {/if}
                  <Button variant={isOn ? 'secondary' : 'outline'} size="sm" disabled={!kbUsable(kb) || readOnly} onclick={() => toggleAttach(kb)} aria-pressed={isOn}>
                    {isOn ? t('kb.detach') : t('kb.attach')}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    class="text-destructive hover:text-destructive"
                    onclick={() => remove(kb)}
                    aria-label={confirmDelete === kb.id ? t('kb.delete_confirm', { name: kb.name }) : t('kb.delete')}
                  >
                    <Trash2 />
                    {confirmDelete === kb.id ? t('kb.delete_sure') : t('kb.delete')}
                  </Button>
                {/if}
              </div>
            </li>
          {/each}
        </ul>
      {/if}

      <div class="flex flex-wrap items-center justify-end gap-1.5">
        <Button variant="outline" size="sm" disabled={busy || readOnly} onclick={createFromFiles}>
          <FilePlus />
          {t('kb.new_files')}
        </Button>
        <Button size="sm" disabled={busy || readOnly} onclick={createFromFolder}>
          {#if busy}<LoaderCircle class="animate-spin" />{:else}<FolderOpen />{/if}
          {t('kb.new_folder')}
        </Button>
      </div>
    </Popover.Content>
  </Popover.Root>

  {#if attached.length > 0}
    <ul class="m-0 flex min-w-0 flex-1 list-none flex-wrap gap-1.5 p-0" aria-label={t('kb.attached_label')}>
      {#each attached as kb (kb.id)}
        <li class="min-w-0">
          <span
            class={cn(
              'flex h-7 max-w-56 min-w-0 items-center gap-1.5 rounded-md border bg-card pr-1 pl-2 text-xs text-foreground',
              kb.state === 'stale' || kb.state === 'incomplete' ? 'border-warning/50' : 'border-border'
            )}
            title={kb.state === 'stale' || kb.state === 'incomplete' ? warning(kb) : metaText(kb)}
          >
            {#if kb.state === 'indexing'}
              <LoaderCircle class="size-3.5 shrink-0 animate-spin text-muted-foreground" aria-hidden="true" />
            {:else if kb.state === 'stale' || kb.state === 'incomplete'}
              <TriangleAlert class="size-3.5 shrink-0 text-warning" aria-hidden="true" />
            {:else}
              <BookOpen class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
            {/if}
            <span class="min-w-0 truncate">{kb.name}</span>
            <button
              type="button"
              class="flex size-5 shrink-0 items-center justify-center rounded text-muted-foreground outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50"
              aria-label={t('kb.detach_named', { name: kb.name })}
              disabled={readOnly}
              onclick={() => toggleAttach(kb)}
            >
              <X class="size-3" aria-hidden="true" />
            </button>
          </span>
        </li>
      {/each}
    </ul>
  {/if}
</div>
