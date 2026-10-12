<script lang="ts">
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  // A compaction marker in the chat: "N earlier messages were summarized", the summary on
  // demand, and undo / re-apply. The summarized messages stay above it (dimmed).
  import { t, locale } from '$lib/i18n.svelte';
  import { cn } from '$lib/utils';
  import { Button } from '$lib/components/ui/button';
  import * as Collapsible from '$lib/components/ui/collapsible';
  import { ChevronDown, FoldVertical, Redo2, Undo2 } from '@lucide/svelte';
  import MessageContent from './MessageContent.svelte';
  import type { ChatMessage } from './types';

  interface Props {
    message: ChatMessage;
    /** This marker is the one the next request applies. */
    active: boolean;
    /** Undo / re-apply are offered (not while answering, not read-only). */
    canToggle?: boolean;
    onUndo?: () => void;
    onRedo?: () => void;
  }

  let { message, active, canToggle = false, onUndo, onRedo }: Props = $props();

  let open = $state(false);
  let meta = $derived(message.compaction);
  let undone = $derived(!!meta?.undone);

  function fmt(n: number): string {
    return n.toLocaleString(locale(), { maximumFractionDigits: 0 });
  }
</script>

{#if meta}
  <Collapsible.Root bind:open class="flex flex-col gap-2" role="group" aria-label={t('cmp.divider_label')}>
    <div class="flex items-center gap-3">
      <div class="h-px flex-1 bg-border" aria-hidden="true"></div>
      <div
        class={cn(
          'flex max-w-full min-w-0 flex-wrap items-center justify-center gap-x-2 gap-y-1 rounded-full border px-3 py-1 text-xs',
          undone || !active ? 'border-dashed border-border text-muted-foreground' : 'border-border bg-muted/40 text-foreground'
        )}
      >
        <FoldVertical class="size-3.5 shrink-0" aria-hidden="true" />
        <span class="font-medium">
          {#if undone}
            {t('cmp.undone', { n: meta.covered })}
          {:else if !active}
            {t('cmp.superseded', { n: meta.covered })}
          {:else}
            {t('cmp.summarized', { n: meta.covered })}
          {/if}
        </span>
        <span class="text-muted-foreground">·</span>
        <span class="text-muted-foreground">
          {meta.mid_turn ? t('cmp.trigger_mid_turn') : meta.trigger === 'auto' ? t('cmp.trigger_auto') : t('cmp.trigger_manual')}
        </span>
        <AdvancedOnly>{#if !undone}
          <span class="text-muted-foreground">·</span>
          <span class="font-mono text-muted-foreground tabular" title={meta.exact_tokens ? t('cmp.tokens_exact') : t('cmp.tokens_estimated')}>
            {t('cmp.tokens', { before: fmt(meta.tokens_before), after: fmt(meta.tokens_after) })}
          </span>
        {/if}</AdvancedOnly>
        <Collapsible.Trigger>
          {#snippet child({ props })}
            <Button {...props} variant="ghost" size="xs" class="h-6 px-1.5 text-xs" aria-expanded={open}>
              {open ? t('cmp.hide_summary') : t('cmp.show_summary')}
              <ChevronDown class={cn('transition-transform', open && 'rotate-180')} />
            </Button>
          {/snippet}
        </Collapsible.Trigger>
        {#if canToggle}
          {#if undone}
            <Button variant="ghost" size="xs" class="h-6 px-1.5 text-xs" onclick={onRedo}>
              <Redo2 />
              {t('cmp.redo')}
            </Button>
          {:else if active}
            <Button variant="ghost" size="xs" class="h-6 px-1.5 text-xs" onclick={onUndo} title={t('cmp.undo_hint')}>
              <Undo2 />
              {t('cmp.undo')}
            </Button>
          {/if}
        {/if}
      </div>
      <div class="h-px flex-1 bg-border" aria-hidden="true"></div>
    </div>
    <Collapsible.Content>
      <div class="rounded-lg border border-border bg-card px-4 py-3 text-sm">
        <AdvancedOnly><p class="mb-2 text-xs text-muted-foreground">
          {t('cmp.summary_meta', { turns: meta.turns, kept: meta.kept_turns, tokens: fmt(meta.summary_tokens) })}
          {#if meta.model}<span class="font-mono"> · {meta.model}</span>{/if}
        </p></AdvancedOnly>
        {#if meta.tool_results_cleared}
          <p class="mb-2 text-xs text-muted-foreground">{t('cmp.tools_cleared', { n: meta.tool_results_cleared })}</p>
        {/if}
        {#if meta.mid_turn}
          <p class="mb-2 text-xs text-muted-foreground">{t('cmp.mid_turn_note')}</p>
        {/if}
        {#if meta.instruction}
          <p class="mb-2 text-xs text-muted-foreground">{t('cmp.instruction_used', { text: meta.instruction })}</p>
        {/if}
        <MessageContent text={message.content} />
        {#if undone}
          <p class="mt-2 text-xs text-muted-foreground">{t('cmp.undone_hint')}</p>
        {/if}
      </div>
    </Collapsible.Content>
  </Collapsible.Root>
{/if}
