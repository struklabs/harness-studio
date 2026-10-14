<script lang="ts">
  import AdvancedOnly from '#lib/components/common/AdvancedOnly.svelte';
  import type { HardwareFitResult, LibraryEntry, LibraryView, LocalModel } from '#lib/types.js';
  import { invoke } from '#lib/ipc.js';
  import { library } from '#lib/stores/library.svelte.js';
  import { t } from '#lib/i18n.svelte.js';
  import { sourceLabel, type LocalModelExt } from '#lib/import-types.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import { Link2, Folder, AlertTriangle } from '@lucide/svelte';
  import FitIndicator from './FitIndicator.svelte';

  interface Props {
    model: LocalModel;
    fitResult: HardwareFitResult | null;
    recommendedNgl: number | null;
    gpuLayers: number;
    onApplyRecommendedNgl: () => void;
    onOpenMigration: () => void;
    /** Removes a linked (Ollama / LM Studio) model from the list; rejects with the backend error. */
    onUnlink: (modelId: string) => Promise<void>;
  }

  let { model, fitResult, recommendedNgl, gpuLayers, onApplyRecommendedNgl, onOpenMigration, onUnlink }: Props = $props();

  // Capabilities come from the model registry record (the same one Kütüphanem shows), not from the
  // file name. Until the record has arrived nothing is claimed.
  let entry = $state<LibraryEntry | null>(null);
  $effect(() => {
    const id = model.id;
    void library.version;
    let stale = false;
    invoke<LibraryView>('list_library')
      .then((v) => {
        if (!stale) entry = v.entries.find((e) => e.model.id === id) ?? null;
      })
      .catch(() => {
        if (!stale) entry = null;
      });
    return () => {
      stale = true;
    };
  });
  let capabilities = $derived.by(() => {
    const list = entry && entry.model.id === model.id ? entry.record.capabilities : [];
    return {
      embedding: list.includes('embedding'),
      chat: list.includes('chat'),
      vision: list.includes('vision'),
      // A template fact the record does not carry: the chat template's own tool support.
      tools: !!entry && entry.model.id === model.id && entry.caps.tools,
    };
  });
  let source = $derived((model as LocalModelExt).source ?? null);
  let facts = $derived(
    [
      capabilities.embedding ? t('dash.ms_badge_embedding') : capabilities.chat ? t('dash.ms_badge_chat') : null,
      capabilities.vision ? t('dash.ms_badge_vision') : null,
      capabilities.tools ? t('dash.ms_badge_tools') : null,
      model.context_length ? t('dash.ms_ctx_short', { count: model.context_length }) : null,
      model.architecture,
      model.is_split && model.is_ready ? t('dash.ms_split_complete', { present: model.present_shards ?? 0, total: model.total_shards ?? 0 }) : null,
    ]
      .filter(Boolean)
      .join(' · ')
  );

  // Inline unlink confirmation, keyed by model id so switching models resets it.
  let confirmUnlinkId = $state<string | null>(null);
  let unlinking = $state(false);
  let unlinkError = $state<{ id: string; msg: string } | null>(null);

  async function confirmUnlink(id: string) {
    unlinking = true;
    unlinkError = null;
    try {
      await onUnlink(id);
      confirmUnlinkId = null;
    } catch (e) {
      unlinkError = { id, msg: t('imp.unlink_failed', { error: String(e) }) };
    } finally {
      unlinking = false;
    }
  }
</script>

<div class="flex min-w-0 flex-col gap-2">
  <!-- One line of facts instead of a badge per fact; a split model's parts only when some are missing. -->
  <AdvancedOnly>
    <p class="flex min-w-0 flex-wrap items-center gap-x-1.5 text-xs text-muted-foreground">
      {#if source}
        <span class="inline-flex items-center gap-1 text-muted-foreground" title={t('imp.badge_linked_title', { source: sourceLabel(source) })}>
          <Link2 class="size-3" />{sourceLabel(source)}
        </span>
        <span aria-hidden="true">·</span>
      {/if}
      <span class="min-w-0 truncate">{facts}</span>
    </p>
  </AdvancedOnly>

  {#if fitResult}
    <FitIndicator {fitResult} {recommendedNgl} {gpuLayers} {onApplyRecommendedNgl} />
  {/if}

  {#if model.is_split && !model.is_ready}
    <div class="flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-2 text-xs text-destructive" role="alert">
      <AlertTriangle class="mt-0.5 size-3.5 shrink-0" />
      <span><strong class="font-medium">{t('dash.ms_split_warn_title')}</strong> {t('dash.ms_split_warn_body')}</span>
    </div>
  {/if}

  <AdvancedOnly>
    <div class="flex items-center justify-between gap-2">
      <Tooltip.Root>
        <Tooltip.Trigger class="flex min-w-0 items-center gap-1.5 rounded text-left text-xs text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none">
          <Folder class="size-3 shrink-0" />
          <span class="truncate font-mono">{model.root_dir}</span>
        </Tooltip.Trigger>
        <Tooltip.Content class="max-w-80 break-all">{model.root_dir}</Tooltip.Content>
      </Tooltip.Root>

      <div class="flex shrink-0 items-center gap-1">
        {#if source}
          <Tooltip.Root>
            <Tooltip.Trigger>
              {#snippet child({ props })}
                <!-- A disabled button gets no pointer events: the focusable wrapper carries the tooltip. -->
                <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
                <span {...props} role="note" tabindex="0" class="rounded focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none">
                  <Button variant="ghost" size="xs" disabled tabindex={-1} class="pointer-events-none">{t('dash.ms_migrate_btn')}</Button>
                </span>
              {/snippet}
            </Tooltip.Trigger>
            <Tooltip.Content class="max-w-64">{t('imp.migrate_disabled_title', { source: sourceLabel(source) })}</Tooltip.Content>
          </Tooltip.Root>
          <Tooltip.Root>
            <Tooltip.Trigger>
              {#snippet child({ props })}
                <Button
                  {...props}
                  variant="ghost"
                  size="xs"
                  class="text-destructive hover:text-destructive"
                  disabled={unlinking || confirmUnlinkId === model.id}
                  onclick={() => {
                    confirmUnlinkId = model.id;
                    unlinkError = null;
                  }}
                >
                  {t('imp.unlink_btn')}
                </Button>
              {/snippet}
            </Tooltip.Trigger>
            <Tooltip.Content>{t('imp.unlink_title')}</Tooltip.Content>
          </Tooltip.Root>
        {:else}
          <Tooltip.Root>
            <Tooltip.Trigger>
              {#snippet child({ props })}
                <Button {...props} variant="ghost" size="xs" onclick={onOpenMigration}>{t('dash.ms_migrate_btn')}</Button>
              {/snippet}
            </Tooltip.Trigger>
            <Tooltip.Content>{t('dash.ms_migrate_title')}</Tooltip.Content>
          </Tooltip.Root>
        {/if}
      </div>
    </div>

    {#if source && confirmUnlinkId === model.id}
      <div class="flex flex-col gap-2 rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-2 text-xs" role="group" aria-label={t('imp.unlink_btn')}>
        <span>{t('imp.unlink_confirm', { source: sourceLabel(source) })}</span>
        <div class="flex gap-1.5">
          <Button variant="destructive" size="xs" onclick={() => confirmUnlink(model.id)} disabled={unlinking}>{t('imp.unlink_yes')}</Button>
          <Button variant="outline" size="xs" onclick={() => (confirmUnlinkId = null)} disabled={unlinking}>{t('imp.unlink_no')}</Button>
        </div>
      </div>
    {/if}
    {#if unlinkError && unlinkError.id === model.id}
      <div class="rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-2 text-xs text-destructive" role="alert">{unlinkError.msg}</div>
    {/if}
  </AdvancedOnly>
</div>
