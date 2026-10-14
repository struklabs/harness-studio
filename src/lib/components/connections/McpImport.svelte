<script lang="ts">
  import HelpPopover from '../common/HelpPopover.svelte';
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  import { onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Checkbox } from '#lib/components/ui/checkbox/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { ShieldAlert } from '@lucide/svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { uniqueServerName } from '#lib/mcp-client.js';
  import type { McpConfig, McpImportSource, McpImportSourceInfo, McpServerDef } from '#lib/mcp-types.js';
  import { cn } from '#lib/utils.js';

  // Rendered inside <Dialog.Content>; the parent owns the dialog's open state.
  interface Props {
    standalone?: boolean;
    existingNames: string[];
    saving: boolean;
    /** Save error reported by the parent (shown inside the dialog). */
    error?: string;
    /** Merges and saves; returns false on failure (error shown via `error`). */
    onImport: (servers: Record<string, McpServerDef>) => Promise<boolean>;
    onClose: () => void;
  }

  let { existingNames, saving, error = '', onImport, onClose, standalone = false }: Props = $props();

  const SOURCE_LABEL: Record<McpImportSource, string> = {
    claude: 'Claude Desktop',
    'claude-code': 'Claude Code',
    codex: 'Codex',
    cursor: 'Cursor',
    lmstudio: 'LM Studio',
  };

  let sources = $state<McpImportSourceInfo[]>([]);
  let sourcesError = $state('');
  let active = $state<McpImportSource | null>(null);
  let preview = $state<[string, McpServerDef][]>([]);
  let selected = $state<Record<string, boolean>>({});
  let loading = $state(false);
  let previewError = $state('');
  let alive = true;

  onMount(() => {
    void (async () => {
      try {
        const list = await invoke<McpImportSourceInfo[]>('get_mcp_import_sources');
        if (alive) sources = list;
      } catch (e) {
        if (alive) sourcesError = String(e);
      }
    })();
    return () => (alive = false);
  });

  async function openSource(source: McpImportSource) {
    active = source;
    loading = true;
    previewError = '';
    preview = [];
    try {
      const cfg = await invoke<McpConfig>('read_mcp_import_source', { source });
      if (!alive || active !== source) return;
      preview = Object.entries(cfg.mcpServers ?? {}).sort(([a], [b]) => a.localeCompare(b));
      selected = Object.fromEntries(preview.map(([n]) => [n, true]));
    } catch (e) {
      if (alive) previewError = String(e);
    } finally {
      if (alive) loading = false;
    }
  }

  /** Final names for the selected servers (clashes get a -2, -3… suffix). */
  let finalNames = $derived.by(() => {
    const taken = new Set(existingNames);
    const out: Record<string, string> = {};
    for (const [name] of preview) {
      if (!selected[name]) continue;
      const n = uniqueServerName(name, taken);
      taken.add(n);
      out[name] = n;
    }
    return out;
  });
  let selectedCount = $derived(Object.keys(finalNames).length);

  function describe(def: McpServerDef): string {
    if (def.url) return def.url;
    return [def.command ?? '', ...(def.args ?? [])].join(' ');
  }

  async function doImport() {
    const servers: Record<string, McpServerDef> = {};
    for (const [name, def] of preview) {
      if (finalNames[name]) servers[finalNames[name]] = $state.snapshot(def) as McpServerDef;
    }
    if (await onImport(servers)) onClose();
  }
</script>

<div class="flex min-w-0 flex-col gap-4">
  {#if standalone}
    <header class="flex flex-col gap-1">
      <h2 class="text-base font-medium">{t('conn.mcp_import_title')}<HelpPopover text={t('conn.mcp_import_desc')} /></h2>

    </header>
  {:else}
  <Dialog.Header>
    <Dialog.Title>{t('conn.mcp_import_title')}</Dialog.Title>
    <Dialog.Description>{t('conn.mcp_import_desc')}</Dialog.Description>
  </Dialog.Header>
  {/if}

  {#if sourcesError}
    <p class="text-xs text-destructive" role="alert">{sourcesError}</p>
  {/if}

  <div class="flex flex-col gap-1.5" role="group" aria-label={t('conn.mcp_import_sources')}>
    {#each sources as [source, path, exists] (source)}
      <FieldHelp text={t('help.mcp_import_mode')}><button
        type="button"
        class={cn(
          'flex min-w-0 items-center justify-between gap-3 rounded-md border px-3 py-2 text-left text-xs transition-colors outline-none focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50',
          active === source ? 'border-primary/50 bg-primary/5' : 'border-border bg-card hover:bg-muted/50'
        )}
        disabled={!exists || loading}
        aria-pressed={active === source}
        onclick={() => openSource(source)}
      >
        <span class="font-medium">{SOURCE_LABEL[source] ?? source}</span>
        <span class="min-w-0 truncate font-mono text-xs text-muted-foreground" title={path ?? ''}>
          {exists ? path : t('conn.mcp_import_not_found')}
        </span>
      </button></FieldHelp>
    {/each}
  </div>

  <div aria-live="polite">
    {#if loading}
      <p class="text-xs text-muted-foreground">{t('conn.mcp_import_loading')}</p>
    {:else if previewError}
      <p class="text-xs text-destructive" role="alert">{previewError}</p>
    {:else if active && preview.length === 0}
      <p class="text-xs text-muted-foreground">{t('conn.mcp_import_empty')}</p>
    {/if}
  </div>

  {#if preview.length > 0}
    <fieldset class="flex max-h-64 min-w-0 flex-col gap-1 overflow-y-auto">
      <legend class="mb-1 text-xs font-medium text-muted-foreground">
        {t('conn.mcp_import_choose', { source: active ? SOURCE_LABEL[active] : '' })}
      </legend>
      {#each preview as [name, def] (name)}
        <label class="flex min-w-0 cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-xs hover:bg-muted/50">
          <FieldHelp text={t('help.mcp_import')}><Checkbox bind:checked={selected[name]} /></FieldHelp>
          <span class="shrink-0 font-medium">{name}</span>
          {#if finalNames[name] && finalNames[name] !== name}
            <span class="shrink-0 text-warning">{t('conn.mcp_import_renamed', { name: finalNames[name] })}</span>
          {/if}
          <code class="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground">{describe(def)}</code>
          {#if def.env && Object.keys(def.env).length}
            <Badge variant="outline" class="shrink-0 text-muted-foreground">{t('conn.mcp_env_count', { n: Object.keys(def.env).length })}</Badge>
          {/if}
        </label>
      {/each}
    </fieldset>
    {#if preview.some(([n, d]) => selected[n] && d.command)}
      <p class="flex items-start gap-1.5 text-xs text-warning"><ShieldAlert class="mt-0.5 size-3 shrink-0" /> {t('conn.mcp_stdio_notice')}</p>
    {/if}
  {/if}

  {#if error}
    <p class="text-xs whitespace-pre-line text-destructive" role="alert">{error}</p>
  {/if}

  <div class="flex justify-end gap-2">
    {#if !standalone}<Button type="button" variant="outline" onclick={onClose}>{t('conn.mcp_cancel')}</Button>{/if}
    <Button type="button" disabled={selectedCount === 0 || saving} onclick={doImport}>
      {t('conn.mcp_import_add', { n: selectedCount })}
    </Button>
  </div>
</div>
