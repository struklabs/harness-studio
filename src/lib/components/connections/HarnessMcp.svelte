<script lang="ts">
  import SettingRow from '../settings/SettingRow.svelte';
  import HelpPopover from '../common/HelpPopover.svelte';
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  // "Harness MCP'yi ekle": registers the Harness MCP server in Claude Code, Codex,
  // OpenCode and Cursor (preview → backup + write; removal restores the file as it was).
  import { onDestroy, onMount } from 'svelte';
  import { toast } from 'svelte-sonner';
  import { ExternalLink, Plug, RefreshCw, TriangleAlert, Trash2 } from '@lucide/svelte';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Checkbox } from '#lib/components/ui/checkbox/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import * as ToggleGroup from '#lib/components/ui/toggle-group/index.js';
  import { t } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { cn } from '#lib/utils.js';
  import CopyButton from './CopyButton.svelte';
  import type { FileChange } from './integrations';
  import { diffStats, lineDiff } from './lineDiff';
  import {
    applyMcp,
    controlApiMcpUrl,
    DEFAULT_MCP_URL,
    entryText,
    manualCommand,
    mcpOverview,
    previewMcp,
    removeMcp,
    type McpMode,
    type McpRequest,
    type McpTarget,
  } from './harnessMcp';

  /** Above this many lines a full diff is too slow (the LCS table is n×m); show the entry. */
  const DIFF_LIMIT = 1500;

  let targets = $state<McpTarget[]>([]);
  let cliPath = $state<string | null>(null);
  let apiUrl = $state<string | null>(null);
  let loading = $state(true);
  let loadError = $state('');
  let busy = $state<string | null>(null);

  let mode = $state<McpMode>('stdio');
  let command = $state('');
  let allowModelsWrite = $state(false);
  let url = $state(DEFAULT_MCP_URL);
  let token = $state('');

  let preview = $state<{ target: McpTarget; changes: FileChange[]; request: McpRequest } | null>(null);
  let alive = true;
  onDestroy(() => (alive = false));

  let request = $derived<McpRequest>({ mode, command: command.trim(), allowModelsWrite, url: url.trim(), token: token.trim() });
  let ready = $derived(mode === 'stdio' ? request.command.length > 0 : request.url.length > 0 && request.token.length > 0);

  async function load() {
    loading = true;
    loadError = '';
    try {
      const [overview, api] = await Promise.all([mcpOverview(), controlApiMcpUrl().catch(() => null)]);
      if (!alive) return;
      targets = overview.targets;
      cliPath = overview.cli_path;
      if (!command && overview.cli_path) command = overview.cli_path;
      apiUrl = api;
      if (api && url === DEFAULT_MCP_URL) url = api;
    } catch (e) {
      if (alive) loadError = localizeError(e instanceof Error ? e.message : e) || String(e);
    } finally {
      if (alive) loading = false;
    }
  }
  onMount(load);

  function fail(e: unknown) {
    toast.error(localizeError(e instanceof Error ? e.message : e) || String(e));
  }

  async function openPreview(target: McpTarget) {
    if (busy || !ready) return;
    busy = target.id;
    try {
      const snapshot = { ...request };
      const changes = await previewMcp(target.id, snapshot);
      if (alive) preview = { target, changes, request: snapshot };
    } catch (e) {
      if (alive) fail(e);
    } finally {
      if (alive) busy = null;
    }
  }

  async function confirmApply() {
    if (!preview || busy) return;
    const p = preview;
    busy = p.target.id;
    try {
      await applyMcp(p.target.id, p.request);
      if (!alive) return;
      preview = null;
      toast.success(t('hmcp.added'));
      await load();
    } catch (e) {
      if (alive) {
        preview = null;
        fail(e);
      }
    } finally {
      if (alive) busy = null;
    }
  }

  async function remove(target: McpTarget) {
    if (busy) return;
    busy = target.id;
    try {
      await removeMcp(target.id);
      if (!alive) return;
      toast.success(t('hmcp.removed'));
      await load();
    } catch (e) {
      if (alive) fail(e);
    } finally {
      if (alive) busy = null;
    }
  }

  function lineCount(s: string | null) {
    return s ? s.split('\n').length : 0;
  }
  const existingHelpId = $props.id();
</script>

<section class="flex min-w-0 flex-col gap-3" aria-labelledby="conn-hmcp-title">
  <div class="flex items-center justify-between gap-3">
    <div class="flex min-w-0 flex-col">
      <h2 id="conn-hmcp-title" class="flex items-center gap-1.5 text-base font-medium"><Plug class="size-4" />{t('hmcp.title')}<HelpPopover text={t('hmcp.desc')} /></h2>
    </div>
    <Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} aria-label={t('conn.clients_rescan')} title={t('conn.clients_rescan')}>
      <RefreshCw class={cn(loading && 'animate-spin')} />
    </Button>
  </div>

  <div class="flex flex-col gap-3 rounded-lg border border-border bg-card p-3">
    <div class="flex flex-wrap items-center gap-3">
      <span class="text-xs text-muted-foreground">{t('hmcp.mode_label')}</span>
      <FieldHelp text={t('help.mcp_transport')}><ToggleGroup.Root
        type="single"
        variant="outline"
        size="sm"
        value={mode}
        onValueChange={(v) => {
          if (v === 'stdio' || v === 'http') mode = v;
        }}
        aria-label={t('hmcp.mode_label')}
      >
        <ToggleGroup.Item value="stdio">{t('hmcp.mode_stdio')}</ToggleGroup.Item>
        <ToggleGroup.Item value="http">{t('hmcp.mode_http')}</ToggleGroup.Item>
      </ToggleGroup.Root></FieldHelp>
    </div>

    {#if mode === 'stdio'}
      <label class="flex min-w-0 flex-col gap-1 text-xs">
        <span class="text-muted-foreground">{t('hmcp.command_label')}</span>
        <Input aria-describedby={existingHelpId + '-existing-hmcp-stdio_note'} class="h-8 font-mono text-xs" bind:value={command} spellcheck="false" placeholder="harness" />
      </label>
      {#if !cliPath && !loading}
        <p class="flex items-start gap-1.5 text-xs text-foreground" role="status">
          <TriangleAlert class="mt-0.5 size-3.5 shrink-0 text-warning" />
          <span class="min-w-0">{t('hmcp.command_missing')}</span>
        </p>
      {/if}
      <label class="flex cursor-pointer items-center gap-2 text-xs">
        <FieldHelp text={t('help.mcp_write')}><Checkbox bind:checked={allowModelsWrite} /></FieldHelp>
        <span>{t('hmcp.models_write')}</span>
      </label>
      <p id={existingHelpId + '-existing-hmcp-stdio_note'} data-field-help-text class="text-xs text-muted-foreground">{t('hmcp.stdio_note')}</p>
    {:else}
      <label class="flex min-w-0 flex-col gap-1 text-xs">
        <span class="text-muted-foreground">{t('hmcp.url_label')}</span>
        <FieldHelp text={t('help.mcp_url')}><Input class="h-8 font-mono text-xs" bind:value={url} spellcheck="false" /></FieldHelp>
      </label>
      {#if !apiUrl && !loading}
        <p class="flex items-start gap-1.5 text-xs text-foreground" role="status">
          <TriangleAlert class="mt-0.5 size-3.5 shrink-0 text-warning" />
          <span class="min-w-0">{t('hmcp.api_off')}</span>
        </p>
      {/if}
      <SettingRow class="px-0 py-0" label={t('hmcp.token_label')} hint={t('hmcp.token_hint')} note={t('hmcp.token_risk')}>
        <Input class="h-8 font-mono text-xs" type="password" bind:value={token} placeholder={t('hmcp.token_placeholder')} autocomplete="off" />
      </SettingRow>
      {#if !request.token}
        <p class="text-xs text-warning" role="status">{t('hmcp.token_required')}</p>
      {/if}
    {/if}
  </div>

  {#if loadError}
    <p class="text-xs text-destructive" role="alert">{t('conn.clients_detect_failed', { message: loadError })}</p>
  {/if}

  <ul class="flex flex-col divide-y divide-border overflow-hidden rounded-lg border border-border bg-card">
    {#each targets as target (target.id)}
      {@const manual = manualCommand(target.id, request)}
      <li class="flex flex-col gap-2 p-3">
        <div class="flex items-center gap-3">
          <div class="flex min-w-0 flex-1 flex-col gap-1">
            <div class="flex flex-wrap items-center gap-1.5">
              <span class="text-sm font-medium">{target.name}</span>
              {#if target.installed}
                <Badge variant="outline" class="text-success">{t('conn.client_installed')}</Badge>
              {:else}
                <Badge variant="outline" class="text-muted-foreground">{t('conn.client_not_installed')}</Badge>
              {/if}
              {#if target.configured}
                <Badge variant="outline" class="border-success/40 text-success">{t('hmcp.configured')}</Badge>
              {/if}
              <a class="inline-flex items-center gap-0.5 text-xs text-muted-foreground hover:text-foreground" href={target.docs_url} target="_blank" rel="noreferrer">
                {t('hmcp.docs')}<ExternalLink class="size-3" />
              </a>
            </div>
            {#if target.config_path}
              <span class="truncate font-mono text-xs text-muted-foreground" title={target.config_path}>{target.config_path}</span>
            {/if}
          </div>
          <div class="flex shrink-0 items-center gap-1.5">
            {#if target.configured}
              <Button variant="ghost" size="sm" onclick={() => remove(target)} disabled={busy !== null}>
                <Trash2 />{busy === target.id ? t('hmcp.removing') : t('hmcp.remove')}
              </Button>
            {/if}
            <Button size="sm" variant={target.configured ? 'outline' : 'default'} onclick={() => openPreview(target)} disabled={busy !== null || !ready || !!target.error}>
              {target.configured ? t('hmcp.update') : t('hmcp.add')}
            </Button>
          </div>
        </div>
        {#if target.error}
          <p class="text-xs break-words text-destructive" role="alert">{t('hmcp.config_error', { message: localizeError(target.error) })}</p>
        {/if}
        {#if target.id === 'claude-code'}
          <p class="text-xs text-muted-foreground">{t('hmcp.claude_restart_note')}</p>
        {/if}
        {#if manual}
          <div class="flex min-w-0 items-center gap-1.5">
            <span class="shrink-0 text-xs text-muted-foreground">{t('hmcp.manual_title')}:</span>
            <code class="min-w-0 flex-1 truncate font-mono text-xs" title={manual}>{manual}</code>
            <CopyButton text={manual} ariaLabel={t('hmcp.manual_title')} variant="ghost" size="icon-sm" />
          </div>
        {/if}
      </li>
    {/each}
  </ul>
</section>

<Dialog.Root
  bind:open={
    () => preview !== null,
    (v) => {
      if (!v && !busy) preview = null;
    }
  }
>
  <Dialog.Content class="sm:max-w-2xl">
    {#if preview}
      <Dialog.Header>
        <Dialog.Title>{t('hmcp.preview_title', { name: preview.target.name })}</Dialog.Title>
        <Dialog.Description>{t('hmcp.preview_desc')}</Dialog.Description>
      </Dialog.Header>
      <div class="flex max-h-[55vh] min-w-0 flex-col gap-3 overflow-auto">
        {#each preview.changes as change (change.path)}
          {@const large = lineCount(change.before) > DIFF_LIMIT || lineCount(change.after) > DIFF_LIMIT}
          <div class="flex min-w-0 flex-col gap-1.5">
            <div class="flex min-w-0 items-center gap-2">
              <span class="truncate font-mono text-xs text-muted-foreground" title={change.path}>{change.path}</span>
              <Badge variant="outline" class={cn('shrink-0', change.before === null ? 'text-success' : 'text-info')}>
                {change.before === null ? t('conn.preview_new') : t('conn.preview_modified')}
              </Badge>
            </div>
            {#if large}
              <p class="text-xs text-muted-foreground">{t('hmcp.preview_large')}</p>
              <pre class="max-h-72 overflow-auto rounded-md border border-border bg-success/10 p-3 font-mono text-xs leading-relaxed break-all whitespace-pre-wrap text-success"><code>{entryText(preview.target.id, change.after)}</code></pre>
            {:else}
              {@const diff = lineDiff(change.before, change.after)}
              {@const stats = diffStats(diff)}
              <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs">
                <span class="font-num text-diff-add">+{stats.added}</span>
                <span class="font-num text-diff-remove">−{stats.removed}</span>
                <span class="text-muted-foreground">{t('conn.preview_legend')}</span>
              </div>
              <div class="max-h-72 overflow-auto rounded-md border border-border bg-muted/40 py-1 font-mono text-xs leading-relaxed">
                {#each diff as line, idx (idx)}
                  {#if line.kind === 'skip'}
                    <div class="px-3 py-0.5 text-xs text-muted-foreground select-none">⋯ {t('conn.preview_unchanged', { count: line.count })}</div>
                  {:else}
                    <div
                      class={cn(
                        'flex gap-2 px-3 break-all whitespace-pre-wrap',
                        line.kind === 'add' && 'bg-diff-add/15 text-diff-add',
                        line.kind === 'del' && 'bg-diff-remove/15 text-diff-remove line-through decoration-diff-remove/40',
                        line.kind === 'same' && 'text-muted-foreground'
                      )}
                    >
                      <span class="w-3 shrink-0 select-none" aria-hidden="true">{line.kind === 'add' ? '+' : line.kind === 'del' ? '−' : ' '}</span>
                      <span class="min-w-0">{line.text || ' '}</span>
                    </div>
                  {/if}
                {/each}
              </div>
            {/if}
            {#if change.before !== null}
              <p class="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
                <span class="shrink-0">{t('hmcp.preview_backup')}</span>
                <span class="truncate font-mono text-foreground" title={change.path}>{t('intg.backup_pattern', { path: change.path })}</span>
              </p>
            {/if}
          </div>
        {/each}
      </div>
      <Dialog.Footer>
        <Button variant="outline" onclick={() => (preview = null)} disabled={busy !== null}>{t('conn.mcp_cancel')}</Button>
        <Button onclick={confirmApply} disabled={busy !== null}>
          {busy ? t('conn.client_configuring') : t('conn.preview_apply')}
        </Button>
      </Dialog.Footer>
    {/if}
  </Dialog.Content>
</Dialog.Root>
