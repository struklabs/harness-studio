<script lang="ts">
  import { untrack } from 'svelte';
  // The review step of an n8n proposal in the Playground: the model staged a change through
  // `n8n_propose_workflow` / `n8n_propose_run` and the tool call waits here. The user sees the
  // node changes, warnings and a line diff, and only the Approve button writes to n8n (a
  // command the model cannot call). After that the result and the execution are shown.
  import { invoke } from '#lib/ipc.js';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { toast } from 'svelte-sonner';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { cn } from '#lib/utils.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import type { N8nProposal, N8nPermission } from '#lib/types.js';
  import type { PgToolCall } from './types';
  import { CircleAlert, ExternalLink, FileDiff, TriangleAlert } from '@lucide/svelte';

  let { call }: { call: PgToolCall } = $props();

  let proposal = $state<N8nProposal | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let diffOpen = $state(false);

  async function load() {
    if (!call.runId) return;
    try {
      proposal = await invoke<N8nProposal | null>('n8n_proposal_for_call', { runId: call.runId });
    } catch {
      // The card stays as it was; the tool result still tells the story.
    }
  }

  // Follow the proposal while the tool call runs or the proposal is undecided. `active` is
  // derived so the timer is only restarted when it really flips.
  let active = $derived(call.status === 'running' || proposal?.status === 'pending' || proposal?.status === 'applying');
  $effect(() => {
    void call.status;
    untrack(load);
  });
  $effect(() => {
    if (!active) return;
    const timer = setInterval(load, 1000);
    return () => clearInterval(timer);
  });

  async function decide(kind: 'apply' | 'reject') {
    if (!proposal) return;
    busy = true;
    error = null;
    try {
      proposal = await invoke<N8nProposal>(kind === 'apply' ? 'n8n_apply_proposal' : 'n8n_reject_proposal', {
        id: proposal.id,
      });
    } catch (e) {
      error = localizeError(e);
      await load();
    } finally {
      busy = false;
    }
  }

  const open = (link: string) => openUrl(link).catch((e) => toast.error(localizeError(e)));
  const titleKey = (p: N8nProposal): TKey =>
    p.kind === 'create_workflow'
      ? 'n8np.title_create'
      : p.kind === 'run_workflow'
        ? 'n8np.title_run'
        : p.diff.lines.length || p.node_changes.length
          ? 'n8np.title_update'
          : 'n8np.title_status';
  const levelLabel = (l: N8nPermission) => t(`n8nui.perm_${l}` as TKey);
  const statusTone: Record<string, string> = {
    pending: 'text-warning',
    applying: 'text-info',
    applied: 'text-success',
    rejected: 'text-muted-foreground',
    failed: 'text-destructive',
    expired: 'text-muted-foreground',
  };
  const execTone = (s: string) => (s === 'success' ? 'text-success' : s === 'running' ? 'text-info' : 'text-destructive');
  let pending = $derived(proposal?.status === 'pending');
  let isRun = $derived(proposal?.kind === 'run_workflow');
</script>

{#if proposal}
  <div
    class={cn('flex min-w-0 flex-col gap-2.5 rounded-md border p-2.5', pending ? 'border-warning/50 bg-warning/5' : 'border-border bg-muted/30')}
    role="group"
    aria-label={t('n8np.aria')}
  >
    <div class="flex min-w-0 flex-wrap items-center gap-2">
      <FileDiff class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
      <span class="min-w-0 text-xs font-medium [overflow-wrap:anywhere]">{t(titleKey(proposal), { name: proposal.workflow_name })}</span>
      <Badge variant="outline" class="shrink-0 text-[11px]">{t('n8np.perm_needed', { level: levelLabel(proposal.required_permission) })}</Badge>
      <span class={cn('ml-auto shrink-0 text-xs', statusTone[proposal.status])} aria-live="polite">
        {t(`n8np.status_${proposal.status}` as TKey)}
      </span>
    </div>

    {#if proposal.note}
      <p class="text-xs text-muted-foreground [overflow-wrap:anywhere]">{t('n8np.reason', { reason: proposal.note })}</p>
    {/if}

    {#each proposal.warnings as w (w.code + (w.node ?? ''))}
      <p class="flex items-start gap-1.5 text-xs text-warning [overflow-wrap:anywhere]">
        <TriangleAlert class="mt-px size-3.5 shrink-0" aria-hidden="true" />
        {t(`n8np.warn_${w.code}` as TKey, { node: w.node ?? '' })}
      </p>
    {/each}

    {#if isRun}
      {#if proposal.webhook}
        <p class="font-mono text-xs [overflow-wrap:anywhere]">
          <span class="font-sans text-muted-foreground">{t('n8np.webhook')}:</span>
          {proposal.webhook.method}
          {proposal.webhook.url}
        </p>
      {/if}
      {#if proposal.payload !== null && proposal.payload !== undefined}
        <div class="flex flex-col gap-1">
          <span class="text-xs text-muted-foreground">{t('n8np.payload')}</span>
          <pre class="max-h-40 overflow-auto rounded-md border border-border bg-muted p-2 font-mono text-xs break-words whitespace-pre-wrap select-text">{JSON.stringify(proposal.payload, null, 2)}</pre>
        </div>
      {/if}
    {:else}
      {#if proposal.activate === true}
        <p class="text-xs">{t('n8np.activate_on')}</p>
      {:else if proposal.activate === false}
        <p class="text-xs">{t('n8np.activate_off')}</p>
      {/if}
      {#if proposal.node_changes.length}
        <div class="flex flex-col gap-1">
          <span class="text-xs font-medium text-muted-foreground">{t('n8np.changes')}</span>
          <ul class="flex flex-wrap gap-1.5">
            {#each proposal.node_changes as c (c.name)}
              <li>
                <Badge variant="secondary" class="gap-1 font-mono text-[11px]">
                  <span
                    class={c.kind === 'added' ? 'text-success' : c.kind === 'removed' ? 'text-destructive' : 'text-warning'}
                  >{t(`n8np.node_${c.kind}` as TKey)}</span>
                  {c.name}
                </Badge>
              </li>
            {/each}
          </ul>
        </div>
      {:else if !proposal.diff.lines.length}
        <p class="text-xs text-muted-foreground">{t('n8np.no_content_change')}</p>
      {/if}
      {#if proposal.diff.lines.length}
        <div class="flex flex-col gap-1.5">
          <div class="flex flex-wrap items-center gap-2">
            <Button variant="ghost" size="sm" class="h-7 px-2" onclick={() => (diffOpen = !diffOpen)} aria-expanded={diffOpen}>
              {diffOpen ? t('n8np.hide_diff') : t('n8np.show_diff')}
            </Button>
            <span class="font-num text-xs text-muted-foreground">
              {t('n8np.diff_summary', { added: proposal.diff.added, removed: proposal.diff.removed })}
            </span>
          </div>
          {#if diffOpen}
            <pre
              class="max-h-72 overflow-auto rounded-md border border-border bg-muted py-1 font-mono text-xs leading-relaxed select-text"
            >{#each proposal.diff.lines as line, i (i)}<span
                  class={cn(
                    'block min-w-max px-2 whitespace-pre',
                    line.op === 'add' && 'bg-success/15 text-success',
                    line.op === 'remove' && 'bg-destructive/15 text-destructive',
                    line.op === 'gap' && 'text-muted-foreground'
                  )}
                >{line.op === 'add' ? '+ ' : line.op === 'remove' ? '- ' : line.op === 'gap' ? '…' : '  '}{line.text}</span>{/each}</pre>
            {#if proposal.diff.truncated}
              <p class="text-xs text-muted-foreground">{t('n8np.diff_truncated')}</p>
            {/if}
          {/if}
        </div>
      {/if}
    {/if}

    {#if pending}
      <div class="flex flex-col gap-2 rounded-md border border-warning/40 bg-warning/10 p-2.5" role="alert">
        <p class="text-xs leading-snug">{isRun ? t('n8np.run_hint') : t('n8np.review_hint')}</p>
        <div class="flex flex-wrap items-center gap-2">
          <Button size="sm" disabled={busy} onclick={() => decide('apply')}>
            {isRun ? t('n8np.approve_run') : t('n8np.approve')}
          </Button>
          <Button size="sm" variant="outline" disabled={busy} onclick={() => decide('reject')}>{t('n8np.reject')}</Button>
        </div>
      </div>
    {/if}

    {#if error}
      <p class="flex items-start gap-1.5 text-xs break-words text-destructive" role="alert">
        <CircleAlert class="mt-px size-3.5 shrink-0" aria-hidden="true" />
        {error}
      </p>
    {/if}
    {#if proposal.status === 'failed' && proposal.error}
      <p class="flex items-start gap-1.5 text-xs break-words text-destructive" role="alert">
        <CircleAlert class="mt-px size-3.5 shrink-0" aria-hidden="true" />
        {localizeError(proposal.error)}
      </p>
    {/if}

    {#if proposal.result}
      {@const r = proposal.result}
      <div class="flex flex-col gap-1.5 border-t border-border pt-2">
        <span class="text-xs font-medium text-muted-foreground">{t('n8np.result')}</span>
        <ul class="flex flex-col gap-0.5 text-xs">
          {#each r.steps as step (step)}
            <li>{t(`n8np.step_${step}` as TKey)}</li>
          {/each}
          {#if r.trigger}
            <li class={r.trigger.ok ? '' : 'text-destructive'}>{t('n8np.webhook_answer', { status: r.trigger.status })}</li>
          {/if}
        </ul>
        {#if r.execution}
          {@const ex = r.execution}
          <div class="flex flex-col gap-1.5 rounded-md border border-border bg-background/60 p-2">
            <div class="flex flex-wrap items-center gap-2 text-xs">
              <span class="font-medium">{t('n8np.execution', { id: ex.id })}</span>
              <span class={execTone(ex.status)}>{t(`n8nui.status_${['success', 'error', 'crashed', 'running', 'waiting', 'canceled', 'new'].includes(ex.status) ? ex.status : 'unknown'}` as TKey)}</span>
              <Button variant="ghost" size="sm" class="ml-auto h-7 px-2" onclick={() => open(ex.editor_url)}>
                <ExternalLink />
                {t('n8nui.open_editor')}
              </Button>
            </div>
            {#if ex.status === 'running' || ex.status === 'waiting' || ex.status === 'new'}
              <p class="text-xs text-muted-foreground">{t('n8np.running_note')}</p>
            {/if}
            {#if ex.error}
              <p class="rounded-md bg-destructive/5 p-2 text-xs break-words text-destructive">
                {ex.error.message ?? t('n8nui.error_none')}
                {#if ex.error.node}<span class="block text-muted-foreground">{t('n8nui.error_node', { node: ex.error.node })}</span>{/if}
              </p>
            {/if}
            {#if ex.nodes.length}
              <div class="flex flex-col gap-0.5">
                <span class="text-xs text-muted-foreground">{t('n8np.node_results')}</span>
                <ul class="flex flex-col gap-0.5 text-xs">
                  {#each ex.nodes as n (n.name)}
                    <li class="flex flex-wrap items-center gap-x-2">
                      <span class={n.status === 'error' ? 'text-destructive' : 'text-success'}>●</span>
                      <span class="font-mono">{n.name}</span>
                      {#if n.items !== null}<span class="font-num text-muted-foreground">{t('n8np.items', { n: n.items })}</span>{/if}
                      {#if n.error}<span class="text-destructive [overflow-wrap:anywhere]">{n.error}</span>{/if}
                    </li>
                  {/each}
                </ul>
              </div>
            {/if}
            {#if ex.output}
              <div class="flex flex-col gap-1">
                <span class="text-xs text-muted-foreground">{t('n8np.output')}</span>
                <pre class="max-h-40 overflow-auto rounded-md border border-border bg-muted p-2 font-mono text-xs break-words whitespace-pre-wrap select-text">{ex.output}</pre>
              </div>
            {/if}
          </div>
        {:else if r.trigger?.ok}
          <p class="text-xs text-muted-foreground">{t('n8np.no_execution')}</p>
        {/if}
        {#if r.workflow}
          <div>
            <Button variant="ghost" size="sm" class="h-7 px-2" onclick={() => r.workflow && open(r.workflow.editor_url)}>
              <ExternalLink />
              {t('n8nui.open_editor')}
            </Button>
          </div>
        {/if}
      </div>
    {/if}
  </div>
{/if}
