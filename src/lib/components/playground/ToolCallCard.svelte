<script lang="ts">
  import { t, type TKey } from '$lib/i18n.svelte';
  import { prettyArgs } from '$lib/mcp-client';
  import { cn } from '$lib/utils';
  import { Button } from '$lib/components/ui/button';
  import { Badge } from '$lib/components/ui/badge';
  import { Checkbox } from '$lib/components/ui/checkbox';
  import { Label } from '$lib/components/ui/label';
  import * as Collapsible from '$lib/components/ui/collapsible';
  import * as Tooltip from '$lib/components/ui/tooltip';
  import type { PgToolCall, ToolCallStatus } from './types';
  import { ChevronRight, Wrench, LoaderCircle, Check, X, ShieldAlert } from '@lucide/svelte';

  interface Props {
    call: PgToolCall;
    /** Approve (optionally for the rest of the session) / deny a call waiting for approval. */
    onApprove?: (callId: string, always: boolean) => void;
    onDeny?: (callId: string) => void;
  }

  let { call, onApprove, onDeny }: Props = $props();

  const uid = $props.id();
  let always = $state(false);
  let argsOpen = $state(false);
  let resultOpen = $state(false);
  let args = $derived(prettyArgs(call.arguments));
  let waiting = $derived(call.status === 'approval');
  // Arguments stay visible while the user decides whether to approve.
  let showArgs = $derived(waiting || argsOpen);

  // An approval card that opens just below the visible part of the chat is scrolled into view
  // and its Approve button focused (keyboard users). A user who scrolled far up on purpose
  // (the card is more than a screen below) is left where they are, like the chat's own
  // follow-the-bottom behaviour.
  let card = $state<HTMLElement | null>(null);
  let approveButton = $state<HTMLElement | null>(null);
  function scrollParent(el: HTMLElement): HTMLElement | null {
    for (let p = el.parentElement; p; p = p.parentElement) {
      const overflowY = getComputedStyle(p).overflowY;
      if ((overflowY === 'auto' || overflowY === 'scroll') && p.scrollHeight > p.clientHeight) return p;
    }
    return null;
  }
  $effect(() => {
    if (!waiting || !card) return;
    const el = card;
    const frame = requestAnimationFrame(() => {
      const box = el.getBoundingClientRect();
      const parent = scrollParent(el);
      const view = parent?.getBoundingClientRect() ?? { top: 0, bottom: window.innerHeight };
      const hidden = box.bottom > view.bottom || box.top < view.top;
      const farBelow = box.top - view.bottom > (view.bottom - view.top);
      if (hidden && !farBelow) {
        const reduce = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;
        el.scrollIntoView({ block: 'nearest', behavior: reduce ? 'auto' : 'smooth' });
      }
      if (!farBelow) approveButton?.focus({ preventScroll: true });
    });
    return () => cancelAnimationFrame(frame);
  });

  const statusKey: Record<ToolCallStatus, TKey> = {
    pending: 'pg.tool_status_pending',
    approval: 'pg.tool_status_approval',
    running: 'pg.tool_status_running',
    done: 'pg.tool_status_done',
    error: 'pg.tool_status_error',
    denied: 'pg.tool_status_denied',
    stopped: 'pg.tool_status_stopped',
  };

  const statusTone: Record<ToolCallStatus, string> = {
    pending: 'text-muted-foreground',
    approval: 'text-warning',
    running: 'text-info',
    done: 'text-success',
    error: 'text-destructive',
    denied: 'text-destructive',
    stopped: 'text-muted-foreground',
  };
</script>

<div
  bind:this={card}
  class={cn(
    'flex min-w-0 flex-col gap-2 rounded-lg border bg-card p-3 text-xs',
    waiting ? 'border-warning/50' : 'border-border'
  )}
  role="group"
  aria-label={t('pg.tool_call_aria', { name: call.name })}
>
  <div class="flex min-w-0 items-center gap-2">
    <Wrench class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
    <code class="min-w-0 truncate font-mono text-xs font-medium text-foreground" title={call.name}>{call.name}</code>
    {#if call.dangerous}
      <Tooltip.Root>
        <Tooltip.Trigger>
          {#snippet child({ props })}
            <Badge {...props} variant="outline" class="gap-1 text-warning">
              <ShieldAlert />
              {t('pg.tool_dangerous')}
            </Badge>
          {/snippet}
        </Tooltip.Trigger>
        <Tooltip.Content class="max-w-64">{t('pg.tool_dangerous_title')}</Tooltip.Content>
      </Tooltip.Root>
    {/if}
    {#if call.autoApproved}
      <span class="shrink-0 text-xs text-muted-foreground">{t('pg.tool_auto_approved')}</span>
    {/if}
    {#if call.recoveredFromText}
      <Badge variant="outline" class="shrink-0">{t('pg.tool_recovered_text')}</Badge>
    {/if}
    <span class={cn('ml-auto flex shrink-0 items-center gap-1 text-xs', statusTone[call.status])} aria-live="polite">
      {#if call.status === 'running'}
        <LoaderCircle class="size-3 animate-spin" aria-hidden="true" />
      {:else if call.status === 'done'}
        <Check class="size-3" aria-hidden="true" />
      {:else if call.status === 'error' || call.status === 'denied'}
        <X class="size-3" aria-hidden="true" />
      {/if}
      {t(statusKey[call.status])}
    </span>
  </div>

  <Collapsible.Root open={showArgs} onOpenChange={(v) => (argsOpen = v)}>
    <Collapsible.Trigger
      class="group flex items-center gap-1 rounded-sm text-xs font-medium text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
    >
      <ChevronRight class="size-3 transition-transform group-data-[state=open]:rotate-90" aria-hidden="true" />
      {t('pg.tool_args')}
    </Collapsible.Trigger>
    <Collapsible.Content>
      <pre
        class="mt-1.5 max-h-56 overflow-auto rounded-md border border-border bg-muted p-2 font-mono text-xs leading-relaxed break-words whitespace-pre-wrap text-foreground select-text">{args || '{}'}</pre>
    </Collapsible.Content>
  </Collapsible.Root>

  {#if waiting}
    <div class="flex flex-col gap-2 rounded-md border border-warning/40 bg-warning/10 p-2.5" role="alert">
      <p class="text-xs leading-snug text-foreground">{t('pg.tool_approval_prompt')}</p>
      <div class="flex flex-wrap items-center gap-2">
        <Button size="sm" bind:ref={approveButton} onclick={() => onApprove?.(call.id, always)}>{t('pg.tool_approve')}</Button>
        <Button size="sm" variant="outline" onclick={() => onDeny?.(call.id)}>{t('pg.tool_deny')}</Button>
        <div class="flex items-center gap-1.5">
          <Checkbox id={`always-${uid}`} bind:checked={always} />
          <Label for={`always-${uid}`} class="text-xs font-normal text-muted-foreground">
            {t('pg.tool_always_allow', { name: call.name })}
          </Label>
        </div>
      </div>
    </div>
  {/if}

  {#if call.errorCode === 'no_workspace'}
    <p class="rounded-md border border-warning/40 bg-warning/10 px-2.5 py-2 text-xs leading-snug text-foreground" role="status">
      {t('pg.tool_no_workspace_hint')}
    </p>
  {/if}

  {#if call.result !== undefined}
    <Collapsible.Root bind:open={resultOpen}>
      <Collapsible.Trigger
        class="group flex items-center gap-1 rounded-sm text-xs font-medium text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
      >
        <ChevronRight class="size-3 transition-transform group-data-[state=open]:rotate-90" aria-hidden="true" />
        {call.status === 'error' ? t('pg.tool_result_error') : t('pg.tool_result')}
        <span class="font-num font-normal">({t('pg.tool_result_len', { n: call.result.length })})</span>
      </Collapsible.Trigger>
      <Collapsible.Content>
        <pre
          class={cn(
            'mt-1.5 max-h-64 overflow-auto rounded-md border border-border bg-muted p-2 font-mono text-xs leading-relaxed break-words whitespace-pre-wrap select-text',
            call.status === 'error' ? 'text-destructive' : 'text-foreground'
          )}>{call.result}</pre>
      </Collapsible.Content>
    </Collapsible.Root>
  {/if}
</div>
