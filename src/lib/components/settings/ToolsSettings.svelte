<script lang="ts">
  let { page = 'tools-builtin' }: { page?: string } = $props();
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  // Settings > Tools: how much the user lets tools do without asking (policy), which tools they
  // trust, and the default working directory of file and shell tools. Saved at once through
  // `tools_set_settings` (the core's `settings.tools`); the Playground, the CLI and the Control
  // API all read the same values.
  import { onMount } from 'svelte';
  import { toast } from 'svelte-sonner';
  import { FolderOpen, ShieldAlert, X } from '@lucide/svelte';
  import { invoke } from '#lib/ipc.js';
  import { recentFolders, sameFolder } from '#lib/workspace-folders.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { cn } from '#lib/utils.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Switch } from '#lib/components/ui/switch/index.js';
  import * as RadioGroup from '#lib/components/ui/radio-group/index.js';
  import {
    TOOL_POLICIES,
    listCoreTools,
    setToolSettings,
    type CoreTool,
    type PermissionClass,
    type ToolPolicy,
    type ToolSettings,
    getWebSearchSettings,
    setWebSearchSettings,
    type SearchProvider,
    type WebSearchSettings,
  } from '#lib/tools-client.js';
  import SettingGroup from './SettingGroup.svelte';
  import SettingRow from './SettingRow.svelte';
  import InstalledToolsSettings from './InstalledToolsSettings.svelte';
  import { Input } from '#lib/components/ui/input/index.js';
  import * as Select from '#lib/components/ui/select/index.js';

  let cfg = $state<ToolSettings | null>(null);
  let tools = $state<CoreTool[]>([]);
  let missingFolders = $state<string[]>([]);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let search = $state<WebSearchSettings | null>(null);
  let searchKey = $state('');

  const policyLabel: Record<ToolPolicy, TKey> = {
    off: 'tools.policy_off',
    ask_every_time: 'tools.policy_ask',
    allow_trusted: 'tools.policy_trusted',
    allow_all: 'tools.policy_all',
  };
  const policyDesc: Record<ToolPolicy, TKey> = {
    off: 'tools.policy_off_desc',
    ask_every_time: 'tools.policy_ask_desc',
    allow_trusted: 'tools.policy_trusted_desc',
    allow_all: 'tools.policy_all_desc',
  };
  const classLabel: Record<PermissionClass, TKey> = {
    safe: 'tools.class_safe',
    restricted: 'tools.class_restricted',
    dangerous: 'tools.class_dangerous',
  };
  const classTone: Record<PermissionClass, string> = {
    safe: 'text-success',
    restricted: 'text-info',
    dangerous: 'text-warning',
  };

  async function load() {
    try {
      const listing = await listCoreTools();
      tools = listing.tools;
      missingFolders = listing.missing_workspaces;
      cfg = { policy: listing.policy, trusted: listing.trusted, workdir: listing.workdir, workspaces: listing.workspaces, web_search: listing.web_search, mcp_expose: listing.mcp_expose ?? false };
      search = await getWebSearchSettings();
      error = null;
    } catch (e) {
      error = localizeError(e);
    }
  }

  async function saveSearch() {
    if (!search || busy) return;
    busy = true;
    try {
      search = await setWebSearchSettings({ provider: search.provider, searxng_url: search.searxng_url, ...(searchKey ? { api_key: searchKey } : {}) });
      if (cfg) cfg.web_search = { provider: search.provider, searxng_url: search.searxng_url };
      searchKey = '';
      toast.success(t('tools.search_saved'));
    } catch (e) { toast.error(t('tools.save_failed', { error: localizeError(e) })); }
    finally { busy = false; }
  }

  onMount(() => {
    void load();
    const refresh = () => void load();
    window.addEventListener('workspace-folders-changed', refresh);
    return () => window.removeEventListener('workspace-folders-changed', refresh);
  });

  async function save(patch: Partial<ToolSettings>) {
    if (!cfg || busy) return;
    busy = true;
    try {
      cfg = await setToolSettings({ ...$state.snapshot(cfg), ...patch });
      error = null;
      window.dispatchEvent(new Event('workspace-folders-changed'));
    } catch (e) {
      toast.error(t('tools.save_failed', { error: localizeError(e) }));
    } finally {
      busy = false;
    }
  }

  function setTrusted(name: string, on: boolean) {
    if (!cfg) return;
    const rest = cfg.trusted.filter((n) => n !== name);
    void save({ trusted: on ? [...rest, name] : rest });
  }

  async function pickWorkdir() {
    try {
      const dir = await invoke<string | null>('pick_directory_dialog');
      if (dir && cfg) await save({ workspaces: recentFolders(cfg.workspaces ?? [], dir) });
    } catch (e) { toast.error(localizeError(e)); }
  }
</script>

{#if error}
  <p class="text-xs text-destructive" role="alert">{t('tools.load_failed', { error })}</p>
{:else if cfg}
  <div class="flex flex-col gap-6">
<div hidden={page !== 'tools-builtin'} class="flex flex-col gap-3">
    <SettingGroup title={t('tools.workspaces_title')} note={t('tools.workdir_desc')}>
      <div class="flex min-w-0 flex-col gap-2 p-3" data-workspace-settings>
        {#each cfg.workspaces ?? [] as path (path)}
          <div class="flex min-w-0 flex-wrap items-center gap-2 rounded-md border border-border p-2">
            <div class="min-w-0 flex-1 basis-48">
              <code class="block break-all font-mono text-xs">{path}</code>
              {#if missingFolders.includes(path)}<p class="mt-1 text-xs text-warning" role="status">{t('tools.workspace_missing')}</p>{/if}
            </div>
            {#if sameFolder(cfg.workdir, path)}
              <Badge variant="secondary">{t('tools.workdir_label')}</Badge>
              <Button variant="ghost" size="xs" disabled={busy} aria-label={`${t('tools.workdir_clear')}: ${path}`} onclick={() => save({ workdir: '' })}>{t('tools.workdir_clear')}</Button>
            {:else}
              <Button variant="outline" size="xs" disabled={busy || missingFolders.includes(path)} aria-label={`${t('tools.workspace_default')}: ${path}`} onclick={() => save({ workdir: path })}>{t('tools.workspace_default')}</Button>
            {/if}
            <Button variant="ghost" size="icon-sm" disabled={busy} aria-label={`${t('tools.workspace_remove')}: ${path}`} title={t('tools.workspace_remove')}
              onclick={() => cfg && save({ workspaces: (cfg.workspaces ?? []).filter(p => !sameFolder(p, path)), workdir: sameFolder(cfg.workdir, path) ? '' : cfg.workdir })}><X /></Button>
          </div>
        {/each}
        {#if !cfg.workdir}<p class="text-xs text-muted-foreground">{t('tools.workdir_none')}</p>{/if}
        <Button variant="outline" size="sm" class="w-fit" disabled={busy} onclick={pickWorkdir}><FolderOpen />{t('tools.workdir_pick')}</Button>
      </div>
    </SettingGroup>

    <SettingGroup title={t('tools.policy_title')} note={t('tools.policy_desc')}>
      <div class="p-3">
        <RadioGroup.Root
          class="flex flex-col gap-2"
          aria-label={t('tools.policy_title')}
          disabled={busy}
          bind:value={() => cfg!.policy, (v) => void save({ policy: v as ToolPolicy })}
        >
          {#each TOOL_POLICIES as p (p)}
            <label
              for="tool-policy-{p}"
              class={cn(
                'flex min-w-0 cursor-pointer items-start gap-2.5 rounded-md border p-3 transition-colors hover:bg-muted/50 has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring/50',
                cfg.policy === p ? 'border-primary/40 bg-primary/5' : 'border-border',
              )}
            >
              <RadioGroup.Item id="tool-policy-{p}" value={p} class="mt-0.5" />
              <span class="flex min-w-0 flex-col gap-0.5">
                <span class="text-sm font-medium">{t(policyLabel[p])}</span>
                <span class="text-xs text-muted-foreground">{t(policyDesc[p])}</span>
              </span>
            </label>
          {/each}
        </RadioGroup.Root>
      </div>
    </SettingGroup>

    <SettingGroup title={t('tools.trusted_title')} note={t('tools.trusted_desc')}>
      {#if cfg.policy !== 'allow_trusted'}
        <p class="px-4 py-2 text-xs text-muted-foreground" role="status">{t('tools.trusted_inactive')}</p>
      {/if}
      {#each tools as tool (tool.name)}
        <SettingRow label={tool.name} hint={tool.description.split('\n')[0]} for="tool-trusted-{tool.name}">
          <Badge variant="outline" class={cn('gap-1', classTone[tool.class])}>
            {#if tool.class === 'dangerous'}<ShieldAlert />{/if}
            {t(classLabel[tool.class])}
          </Badge>
          <Switch
            id="tool-trusted-{tool.name}"
            disabled={busy}
            checked={cfg.trusted.includes(tool.name)}
            onCheckedChange={(v) => setTrusted(tool.name, v)}
          />
        </SettingRow>
      {/each}
    </SettingGroup>



    <SettingGroup title={t('tools.mcp_title')} hint={t('tools.mcp_desc')}>
      <SettingRow label={t('tools.mcp_label')} hint={t('tools.mcp_hint')} for="tool-mcp-expose">
        <Switch
          id="tool-mcp-expose"
          disabled={busy}
          checked={cfg.mcp_expose ?? false}
          onCheckedChange={(v) => void save({ mcp_expose: v })}
        />
      </SettingRow>
    </SettingGroup>

</div>
    <div hidden={page !== 'tools-installed'}><InstalledToolsSettings showTitle={false} /></div>

<div hidden={page !== 'tools-builtin'}>    {#if search}
      <SettingGroup title={t('tools.search_title')} hint={t('tools.search_desc')}>
        <div class="flex flex-col gap-3 p-4">
          <label class="flex flex-col gap-1 text-xs text-muted-foreground">
            {t('tools.search_provider')}
            <FieldHelp text={t('help.search_provider')}><Select.Root type="single" value={search.provider} onValueChange={(v) => { if (v) search!.provider = v as SearchProvider; }}>
              <Select.Trigger class="w-full"><span>{t(`tools.search_${search.provider}` as TKey)}</span></Select.Trigger>
              <Select.Content>
                {#each ['none', 'duckduckgo', 'searxng', 'brave', 'tavily'] as provider}
                  <Select.Item value={provider} label={t(`tools.search_${provider}` as TKey)} />
                {/each}
              </Select.Content>
            </Select.Root></FieldHelp>
          </label>
          {#if search.provider === 'duckduckgo'}
            <p class="rounded-md border border-border p-2 text-xs text-muted-foreground">{t('tools.search_keyless_warning')}</p>
          {:else if search.provider === 'searxng'}
            <label class="flex flex-col gap-1 text-xs text-muted-foreground">{t('tools.search_url')}<FieldHelp text={t('help.search_url')}><Input bind:value={search.searxng_url} placeholder="https://search.example.org" /></FieldHelp></label>
          {:else if search.provider === 'brave' || search.provider === 'tavily'}
            <label class="flex flex-col gap-1 text-xs text-muted-foreground">{t('tools.search_key')}<FieldHelp text={t('help.search_key')}><Input type="password" bind:value={searchKey} autocomplete="new-password" placeholder={search.api_key_set ? t('tools.search_key_saved') : ''} /></FieldHelp></label>
            <p class="text-xs text-muted-foreground">{search.api_key_set ? t('tools.search_key_saved') : t('tools.search_key_missing')}</p>
          {/if}
          <Button class="self-end" size="sm" disabled={busy} onclick={saveSearch}>{t('tools.search_save')}</Button>
        </div>
      </SettingGroup>
    {/if}
</div>
  </div>
{/if}
