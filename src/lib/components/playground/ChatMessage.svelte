<script lang="ts">
  import { splitThinking } from './thinking';
  import { audioUrl } from './audio';
  import { lengthStop } from './contextBudget';
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  import { t, locale } from '#lib/i18n.svelte.js';
  import { cn } from '#lib/utils.js';
  import SectionLoading from '#lib/components/common/SectionLoading.svelte';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import * as Collapsible from '#lib/components/ui/collapsible/index.js';
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import MessageContent from './MessageContent.svelte';
  import LogprobView from './LogprobView.svelte';
  import { recalledLogprobs } from './logprobs';
  import ToolCallCard from './ToolCallCard.svelte';
  import RagSources from './RagSources.svelte';
  import type { ChatMessage } from './types';
  import { AlertCircle, ChevronLeft, ChevronRight, Check, Copy, GitBranch, LoaderCircle, RotateCcw, Pencil, Percent, Square, Volume2 } from '@lucide/svelte';
  import { readAloud } from './tts.svelte';
  import { estimateMessageHeight } from './longChat';

  interface Props {
    message: ChatMessage;
    /** A reply is being generated somewhere in the chat: no editing or regenerating. */
    busy?: boolean;
    /** A server is running (needed to regenerate, resend or rewind). */
    canChat?: boolean;
    /** Last assistant message of a turn: regenerate and variant navigation live here. */
    isTurnEnd?: boolean;
    /** Last message of the path. */
    isLast?: boolean;
    /** Variants of the turn's answer (regenerations). */
    variantNav?: { index: number; count: number } | null;
    onRegenerate?: () => void;
    onVariant?: (dir: -1 | 1) => void;
    onSibling?: (dir: -1 | 1) => void;
    /**
     * Edit from here: rewind the chat to just before this user message (the messages after it
     * leave the path, the text goes back to the input). The parent asks for confirmation.
     */
    onRewind?: () => void;
    /**
     * Edit in place: the edited text is stored as another version of this user message and the
     * answer is generated from there (any message of the history). Replaces the rewind edit.
     */
    onEditSend?: (text: string) => void;
    onFork?: () => void;
    onApproveTool?: (callId: string, always: boolean) => void;
    onDenyTool?: (callId: string) => void;
    contextSize?: number | null;
  }

  let {
    message,
    busy = false,
    canChat = false,
    isTurnEnd = false,
    isLast = false,
    variantNav = null,
    onRegenerate,
    onVariant,
    onSibling,
    onRewind,
    onEditSend,
    onFork,
    onApproveTool,
    onDenyTool,
    contextSize = null,
  }: Props = $props();

  let editing = $state(false);
  // Finished, off-the-end messages skip layout/paint while off-screen (the streaming and last ones
  // stay fully rendered so auto-scroll and approvals measure real sizes).
  let skipOffscreen = $derived(!message.streaming && !isLast && !editing);
  let placeholderStyle = $derived(skipOffscreen ? `content-visibility:auto;contain-intrinsic-size:auto ${estimateMessageHeight(message.content.length)}px` : undefined);
  /** Rendered-markdown cache id: finished messages only, so streaming prefixes never fill the cache. */
  let cacheId = $derived(message.streaming ? undefined : (message.variantId ?? message.id));
  let draft = $state('');

  function startEdit() {
    draft = message.content;
    editing = true;
  }

  function submitEdit() {
    const text = draft.trim();
    editing = false;
    if (text && text !== message.content.trim()) onEditSend?.(text);
  }

  function editKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      editing = false;
    } else if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      submitEdit();
    }
  }

  // A reply started elsewhere (or the message changed under the editor): leave the editor.
  $effect(() => {
    if (busy) editing = false;
  });

  let copied = $state(false);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => () => clearTimeout(copyTimer));

  async function copyReasoning(text: string) {
    try { await navigator.clipboard.writeText(text); } catch { /* clipboard unavailable */ }
  }

  async function copyMessage() {
    try {
      await navigator.clipboard.writeText(message.content);
      copied = true;
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copied = false), 1500);
    } catch {
      // clipboard unavailable
    }
  }

  function num(v: number, digits = 1): string {
    return v.toLocaleString(locale(), { maximumFractionDigits: digits, minimumFractionDigits: digits });
  }

  function duration(ms: number): string {
    return ms < 1000 ? `${Math.round(ms)} ms` : `${num(ms / 1000, 2)} s`;
  }

  // Per-token probabilities (when the request asked for them); session memory, see logprobs.ts.
  let tokenLogprobs = $derived(message.logprobs ?? recalledLogprobs(message.variantId));
  let showProbs = $state(false);
  let isAssistant = $derived(message.role === 'assistant');
  // "Read Aloud": the answer goes through the gateway's /v1/audio/speech (sherpa-onnx).
  let reading = $derived(readAloud.activeId === message.id);
  let readLabel = $derived(reading ? t('speak.stop_reading') : t('speak.read_aloud'));
  let stats = $derived(message.stats);
  let lengthStatus = $derived(lengthStop(message.finishReason, stats, message.contextSize ?? contextSize, message.reasoning, message.content));
  // Only an assistant reply carries protocol <think> markup; a user's text stays as typed.
  let thinkingParts = $derived(isAssistant ? splitThinking(message.content, message.reasoning) : { content: message.content, reasoning: '', live: false });
  let thinkingLive = $derived(!!message.streaming && !thinkingParts.content);
  function durationLabel(ms: number | undefined) {
    if (ms === undefined) return t('pg.thinking');
    return ms < 1000 ? t('pg.thinking_brief') : t('pg.thinking_duration', { seconds: Math.round(ms / 1000) });
  }
  let thinkingLabel = $derived(durationLabel(message.reasoningMs));
  let hasToolCalls = $derived(!!message.toolCalls?.length);
  let reasoningOpen = $state(false);
  let showReasoning = $derived(thinkingLive || reasoningOpen);
  $effect(() => { if (!thinkingLive) reasoningOpen = false; });
  let showFooter = $derived(
    isAssistant && !message.streaming && !!(stats || message.stopped || message.finishReason === 'length')
  );
  let siblingCount = $derived(message.siblingIds?.length ?? 1);
  let canRegenerate = $derived(canChat && !busy && ((isAssistant && isTurnEnd) || (!isAssistant && isLast)));
  let regenerateLabel = $derived(!isAssistant ? t('pg.resend') : message.error ? t('pg.retry') : t('pg.regenerate'));
</script>

{#snippet action(label: string, onclick: () => void, icon: import('svelte').Snippet)}
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <Button {...props} variant="ghost" size="icon-xs" aria-label={label} {onclick}>
          {@render icon()}
        </Button>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content>{label}</Tooltip.Content>
  </Tooltip.Root>
{/snippet}

{#snippet copyIcon()}
  {#if copied}<Check />{:else}<Copy />{/if}
{/snippet}
{#snippet readIcon()}
  {#if reading && readAloud.preparing}<LoaderCircle class="animate-spin" />{:else if reading}<Square />{:else}<Volume2 />{/if}
{/snippet}
{#snippet regenIcon()}<RotateCcw />{/snippet}
{#snippet editIcon()}<Pencil />{/snippet}
{#snippet forkIcon()}<GitBranch />{/snippet}
{#snippet probIcon()}<Percent />{/snippet}

{#snippet navControl(index: number, count: number, move: (dir: -1 | 1) => void, groupLabel: string)}
  <div class="flex items-center gap-0.5" role="group" aria-label={groupLabel}>
    <Button variant="ghost" size="icon-xs" disabled={busy || index <= 0} aria-label={t('pg.nav_prev')} onclick={() => move(-1)}>
      <ChevronLeft />
    </Button>
    <span class="min-w-8 text-center font-num text-xs text-muted-foreground">
      {t('pg.nav_position', { index: index + 1, count })}
    </span>
    <Button
      variant="ghost"
      size="icon-xs"
      disabled={busy || index >= count - 1}
      aria-label={t('pg.nav_next')}
      onclick={() => move(1)}
    >
      <ChevronRight />
    </Button>
  </div>
{/snippet}

<article
  class={cn('group/msg flex min-w-0 flex-col gap-1', isAssistant ? 'items-stretch' : 'items-end')}
  aria-busy={message.streaming ? 'true' : 'false'}
  style={placeholderStyle}
>
  <span class="sr-only">{isAssistant ? t('pg.role_assistant') : t('pg.role_user')}</span>

  {#if isAssistant}
    <div class="flex min-w-0 flex-col gap-3">
      <!-- Earlier rounds of this answer (a split-view side's tool loop): thinking, text, tool calls. -->
      {#each message.priorRounds ?? [] as round, i (i)}
        {@const roundParts = splitThinking(round.content, round.reasoning)}
        {#if roundParts.reasoning}
          <Collapsible.Root>
            <div class="flex items-center gap-1">
              <Collapsible.Trigger
                class="group flex items-center gap-1 rounded-sm text-xs font-medium text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
              >
                <ChevronRight class="size-3.5 transition-transform group-data-[state=open]:rotate-90" aria-hidden="true" />
                {durationLabel(round.reasoningMs)}
              </Collapsible.Trigger>
              <Button variant="ghost" size="icon-xs" class="text-muted-foreground" aria-label={t('pg.copy_thinking')} title={t('pg.copy_thinking')}
                onclick={() => copyReasoning(roundParts.reasoning)}><Copy class="size-3" /></Button>
            </div>
            <Collapsible.Content>
              <p
                class="mt-1.5 max-h-72 overflow-y-auto border-l-2 border-border pl-3 text-xs leading-relaxed whitespace-pre-wrap text-muted-foreground [overflow-wrap:anywhere]"
              >
                {roundParts.reasoning}
              </p>
            </Collapsible.Content>
          </Collapsible.Root>
        {/if}
        {#if round.content}
          <MessageContent text={roundParts.content} cacheId={cacheId && `${cacheId}:r${i}`} />
        {/if}
        {#if round.toolCalls?.length}
          <div class="flex flex-col gap-2">
            {#each round.toolCalls as call (call.id)}
              <ToolCallCard {call} />
            {/each}
          </div>
        {/if}
        {#if round.error}
          <p class="m-0 flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs whitespace-pre-wrap text-destructive select-text [overflow-wrap:anywhere]" role="alert">
            <AlertCircle class="mt-px size-3.5 shrink-0" />
            <span>{round.error}</span>
          </p>
        {/if}
      {/each}

      {#if thinkingParts.reasoning}
        <Collapsible.Root open={showReasoning} onOpenChange={(v) => (reasoningOpen = v)}>
          <div class="flex items-center gap-1">
            <Collapsible.Trigger
              class="group flex items-center gap-1 rounded-sm text-xs font-medium text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
            >
              <ChevronRight class="size-3.5 transition-transform group-data-[state=open]:rotate-90" aria-hidden="true" />
              <span class={cn(thinkingLive && 'animate-pulse')}>
                {thinkingLive ? t('pg.thinking_live') : thinkingLabel}
              </span>
            </Collapsible.Trigger>
            <Button variant="ghost" size="icon-xs" class="text-muted-foreground" aria-label={t('pg.copy_thinking')} title={t('pg.copy_thinking')}
              onclick={() => copyReasoning(thinkingParts.reasoning)}><Copy class="size-3" /></Button>
          </div>
          <Collapsible.Content>
            <p
              class="mt-1.5 max-h-72 overflow-y-auto border-l-2 border-border pl-3 text-xs leading-relaxed whitespace-pre-wrap text-muted-foreground [overflow-wrap:anywhere]"
            >
              {thinkingParts.reasoning}
            </p>
          </Collapsible.Content>
        </Collapsible.Root>
      {/if}

      {#if thinkingParts.content === message.content && message.content && showProbs && tokenLogprobs?.length && !message.streaming}
        <LogprobView tokens={tokenLogprobs} />
      {:else if thinkingParts.content}
        <MessageContent text={isAssistant ? thinkingParts.content : message.content} {cacheId} />
      {:else if message.streaming && !thinkingParts.reasoning && !hasToolCalls}
        <SectionLoading shape="inline" label={t('pg.waiting')} phases={[t('pg.waiting'), t('sl.reply_2')]} />
      {/if}

      {#if message.rag}
        <RagSources rag={message.rag} answer={message.content} />
      {/if}

      {#if hasToolCalls}
        <div class="flex flex-col gap-2">
          {#each message.toolCalls ?? [] as call (call.id)}
            <ToolCallCard {call} onApprove={onApproveTool} onDeny={onDenyTool} />
          {/each}
        </div>
      {/if}

      {#if message.toolLimitReached}
        <p class="m-0 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning" role="status">
          {t('pg.tool_limit_reached')}
        </p>
      {/if}

      {#if message.error}
        <div class="flex flex-col items-start gap-2">
          <p
            class="m-0 flex w-full items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs whitespace-pre-wrap text-destructive select-text [overflow-wrap:anywhere]"
            role="alert"
          >
            <AlertCircle class="mt-px size-3.5 shrink-0" />
            <span>{message.error}</span>
          </p>
          <!-- Visible, not only in the hover actions: the request is sent again as is (a busy
               model, a dropped connection) without adding a user message. -->
          {#if isAssistant && canRegenerate && onRegenerate}
            <Button variant="outline" size="sm" onclick={onRegenerate}>
              <RotateCcw />{t('pg.retry')}
            </Button>
          {/if}
        </div>
      {/if}
    </div>
  {:else if editing}
    <div class="flex w-full min-w-0 flex-col gap-2">
      <!-- svelte-ignore a11y_autofocus -->
      <Textarea
        bind:value={draft}
        autofocus
        rows={3}
        class="max-h-72 min-h-16 text-base"
        aria-label={t('pg.edit_resend')}
        onkeydown={editKeydown}
      />
      <div class="flex justify-end gap-2">
        <Button variant="outline" size="sm" onclick={() => (editing = false)}>{t('pg.cancel')}</Button>
        <Button size="sm" disabled={!draft.trim() || draft.trim() === message.content.trim()} onclick={submitEdit}>
          {t('pg.edit_send')}
        </Button>
      </div>
    </div>
  {:else}
    <div
      class="w-full min-w-0 rounded-md bg-muted/60 px-3 py-2 text-base leading-relaxed text-foreground [overflow-wrap:anywhere]"
    >
      {#if message.images?.length}
        <div class="mb-2 flex flex-wrap gap-2 whitespace-normal" data-message-images>
          {#each message.images as img, i (i)}
            <img src={img.data} alt={img.name} title={img.name} class="max-h-48 max-w-full rounded-md border border-border object-contain" />
          {/each}
        </div>
      {/if}
      {#each message.audio ?? [] as clip, i (i)}
        <div class="mb-2 flex flex-wrap items-center gap-2 whitespace-normal">
          <span class="text-sm">{clip.name}</span>
          <audio controls preload="none" src={audioUrl(clip)} aria-label={clip.name}></audio>
        </div>
      {/each}
      <!-- Only the typed text keeps its line breaks; template whitespace around it must not show. -->
      <span class="whitespace-pre-wrap">{message.content}</span>
    </div>
  {/if}

  {#if !message.streaming}
    <div class={cn('flex min-h-6 items-center gap-2', !isAssistant && 'justify-end')}>
      {#if showFooter}
        <div
          class="flex min-w-0 flex-wrap items-center gap-x-2.5 gap-y-0.5 font-num text-xs text-muted-foreground"
        >
          <AdvancedOnly>
          {#if stats?.predictedPerSecond !== undefined}
            <span title={t('pg.stat_speed_title')}>
              {#if stats.source === 'usage'}≈{/if}{t('pg.stat_speed', { value: num(stats.predictedPerSecond) })}
            </span>
          {/if}
          {#if stats?.predictedN !== undefined}
            <span title={t('pg.stat_tokens_title')}>{#if stats.source === 'estimate'}≈{/if}{t('pg.stat_tokens', { n: stats.predictedN.toLocaleString(locale()) })}</span>
          {/if}
          {#if stats?.ttftMs !== undefined}
            <span title={t('pg.stat_ttft_title')}>{t('pg.stat_ttft', { value: duration(stats.ttftMs) })}</span>
          {/if}
          {#if stats?.promptN !== undefined}
            <span>
              {#if stats.source === 'estimate'}≈{/if}{t('pg.stat_prompt', { n: stats.promptN.toLocaleString(locale()) })}{#if stats.cachedN}&nbsp;({t('pg.stat_cached', { n: stats.cachedN })}){/if}
            </span>
          {/if}
          {#if stats?.promptPerSecond !== undefined}
            <span>{t('pg.stat_prompt_speed', { value: num(stats.promptPerSecond) })}</span>
          {/if}
          {#if stats?.totalMs !== undefined}
            <span>{t('pg.stat_total', { value: duration(stats.totalMs) })}</span>
          {/if}
          {#if stats?.draftN}
            {@const accepted = stats.draftAcceptedN ?? 0}
            <span title={t('spec.stat_draft_title')}>
              {t('spec.stat_draft', { pct: Math.round((accepted / stats.draftN) * 100), accepted, drafted: stats.draftN })}
            </span>
          {/if}
          {#if stats?.source === 'usage'}
            <span class="cursor-help" title={t('pg.stat_usage_only')}>{t('pg.stat_usage_badge')}</span>
          {/if}
          </AdvancedOnly>
          {#if message.stopped}
            <span class="text-warning">{t('pg.stopped')}</span>
          {:else if message.finishReason === 'length'}
            <span class="text-warning">
              {lengthStatus.contextFull
                ? t('pg.finish_context', { context: new Intl.NumberFormat(locale()).format(message.contextSize ?? contextSize ?? 0) })
                : t('pg.finish_length')}
              {#if lengthStatus.thinkingOnly} {t('pg.finish_thinking')}{/if}
            </span>
          {/if}
        </div>
      {/if}
      <div class={cn('flex items-center gap-1', isAssistant && 'ml-auto')}>
        {#if !isAssistant && siblingCount > 1 && onSibling}
          {@render navControl(message.siblingIndex ?? 0, siblingCount, onSibling, t('pg.branch_nav'))}
        {/if}
        <div
          class={cn(
            'flex items-center gap-0.5 transition-opacity group-focus-within/msg:opacity-100 group-hover/msg:opacity-100',
            reading ? 'opacity-100' : 'opacity-0',
          )}
        >
          {#if message.content}
            {@render action(copied ? t('pg.copied') : t('pg.copy'), copyMessage, copyIcon)}
          {/if}
          {#if isAssistant && message.content && !message.streaming && !message.error}
            {@render action(readLabel, () => readAloud.toggle(message.id, message.content), readIcon)}
          {/if}
          {#if !isAssistant && !editing && canChat && !busy}
            {#if onEditSend}
              {@render action(t('pg.edit_resend'), startEdit, editIcon)}
            {:else if onRewind}
              {@render action(t('pg.edit'), onRewind, editIcon)}
            {/if}
          {/if}
          {#if isAssistant && tokenLogprobs?.length}
            {@render action(showProbs ? t('pg.logprobs_hide') : t('pg.logprobs_show'), () => (showProbs = !showProbs), probIcon)}
          {/if}
          {#if canRegenerate && onRegenerate}
            {@render action(regenerateLabel, onRegenerate, regenIcon)}
          {/if}
          {#if onFork && !busy && message.variantId}
            {@render action(t('pg.fork'), onFork, forkIcon)}
          {/if}
        </div>
        {#if isAssistant && isTurnEnd && variantNav && variantNav.count > 1 && onVariant}
          {@render navControl(variantNav.index, variantNav.count, onVariant, t('pg.variant_nav'))}
        {/if}
      </div>
    </div>
  {/if}
</article>
