<script lang="ts">
  import HelpPopover from '../common/HelpPopover.svelte';
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  import { onDestroy, onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { toast } from 'svelte-sonner';
  import { Braces, Download, Info, Lock, Pencil, Plus, Server, Trash2 } from '@lucide/svelte';
  import SectionLoading from '#lib/components/common/SectionLoading.svelte';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import * as Empty from '#lib/components/ui/empty/index.js';
  import { Switch } from '#lib/components/ui/switch/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import { t } from '#lib/i18n.svelte.js';
  import { coerceMcpConfig, validateMcpConfig, type McpValidationError } from '#lib/mcp-client.js';
  import type { McpConfig, McpServerDef } from '#lib/mcp-types.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { cn } from '#lib/utils.js';
  import McpImport from './McpImport.svelte';
  import McpClientStatus from './McpClientStatus.svelte';
  import McpServerForm from './McpServerForm.svelte';

  let { page = 'servers', onNavigate }: { page?: 'servers' | 'import' | 'status'; onNavigate: (page: string) => void } = $props();
  let importVisited = $state(false);
  let statusVisited = $state(false);
  $effect(() => {
    if (page === 'import') importVisited = true;
    if (page === 'status') statusVisited = true;
  });
  let config = $state<McpConfig>({ mcpServers: {} });
  let loaded = $state(false);
  let loadError = $state('');
  let saving = $state(false);
  let saveError = $state('');
  let alive = true;

  // Dialog state. Targets are kept while a dialog animates closed so its content does not flicker.
  let formOpen = $state(false);
  /** null = new server, string = editing that server. */
  let formTarget = $state<string | null>(null);
  let importOpen = $state(false);
  let deleteOpen = $state(false);
  let deleteName = $state('');
  let rawOpen = $state(false);
  let rawText = $state('');
  let rawError = $state('');
  /** Bumped after every save: the MCP client's state is read again. */
  let statusKey = $state(0);

  let entries = $derived(Object.entries(config.mcpServers).sort(([a], [b]) => a.localeCompare(b)));
  let names = $derived(entries.map(([n]) => n));
  let enabledCount = $derived(entries.filter(([, d]) => !d.disabled).length);
  let formInitial = $derived.by(() => {
    if (formTarget === null) return null;
    const def = config.mcpServers[formTarget];
    return def ? { name: formTarget, def: $state.snapshot(def) as McpServerDef } : null;
  });

  onDestroy(() => (alive = false));
  onMount(load);

  function setConfig(next: McpConfig) {
    config = next;
  }

  async function load() {
    loadError = '';
    try {
      const cfg = await invoke<McpConfig>('get_mcp_config');
      if (!alive) return;
      setConfig({ mcpServers: cfg?.mcpServers ?? {} });
      loaded = true;
    } catch (e) {
      if (alive) loadError = String(e);
    }
  }

  function describeValidation(errors: McpValidationError[]): string {
    return errors
      .map((e) => (e.kind === 'name' ? t('conn.mcp_err_bad_name', { name: e.name }) : t('conn.mcp_err_transport', { name: e.name })))
      .join('\n');
  }

  /** Validates and saves; on success replaces the local config. */
  async function save(next: McpConfig): Promise<boolean> {
    saveError = '';
    const errors = validateMcpConfig(next);
    if (errors.length) {
      saveError = describeValidation(errors);
      return false;
    }
    saving = true;
    try {
      await invoke('save_mcp_config', { config: next });
      if (!alive) return true;
      await load();
      statusKey++;
      toast.success(t('conn.mcp_saved'));
      return true;
    } catch (e) {
      if (alive) saveError = String(e);
      return false;
    } finally {
      if (alive) saving = false;
    }
  }

  function snapshot(): McpConfig {
    return $state.snapshot(config) as McpConfig;
  }

  function openForm(target: string | null) {
    formTarget = target;
    saveError = '';
    formOpen = true;
  }

  async function submitForm(name: string, def: McpServerDef): Promise<boolean> {
    const next = snapshot();
    if (formTarget !== null && formTarget !== name) delete next.mcpServers[formTarget];
    next.mcpServers[name] = def;
    const ok = await save(next);
    if (ok) formOpen = false;
    return ok;
  }

  async function toggleEnabled(name: string, enabled: boolean) {
    const next = snapshot();
    const def = next.mcpServers[name];
    if (!def) return;
    if (enabled) delete def.disabled;
    else def.disabled = true;
    if (!(await save(next)) && saveError) toast.error(saveError);
  }

  function askDelete(name: string) {
    deleteName = name;
    saveError = '';
    deleteOpen = true;
  }

  async function confirmDelete() {
    const next = snapshot();
    delete next.mcpServers[deleteName];
    if (await save(next)) deleteOpen = false;
  }

  function openImport() {
    saveError = '';
    importOpen = true;
  }

  async function importServers(servers: Record<string, McpServerDef>): Promise<boolean> {
    const next = snapshot();
    Object.assign(next.mcpServers, servers);
    return save(next);
  }

  function openRaw() {
    rawText = JSON.stringify(snapshot(), null, 2);
    rawError = '';
    saveError = '';
    rawOpen = true;
  }

  async function saveRaw() {
    rawError = '';
    let parsed: unknown;
    try {
      parsed = JSON.parse(rawText);
    } catch (e) {
      rawError = t('conn.mcp_raw_invalid_json', { message: e instanceof Error ? e.message : String(e) });
      return;
    }
    const coerced = coerceMcpConfig(parsed);
    if (!coerced.ok) {
      rawError = t('conn.mcp_raw_invalid_shape', { path: coerced.error });
      return;
    }
    const errors = validateMcpConfig(coerced.config);
    if (errors.length) {
      rawError = describeValidation(errors);
      return;
    }
    if (await save(coerced.config)) rawOpen = false;
    else rawError = saveError;
  }

  function describe(def: McpServerDef): string {
    if (def.url && !def.command) return def.url;
    return [def.command ?? '', ...(def.args ?? [])].join(' ');
  }
</script>

{#snippet iconAction(label: string, onclick: () => void, icon: import('svelte').Snippet, destructive = false)}
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <Button {...props} variant="ghost" size="icon-sm" aria-label={label} {onclick} class={cn(destructive && 'hover:text-destructive')}>
          {@render icon()}
        </Button>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content>{label}</Tooltip.Content>
  </Tooltip.Root>
{/snippet}

{#snippet editIcon()}<Pencil />{/snippet}
{#snippet deleteIcon()}<Trash2 />{/snippet}

<section class="flex min-w-0 flex-col gap-3" aria-labelledby="mcp-title" hidden={page !== 'servers'}>
  <div class="flex flex-wrap items-center justify-between gap-2">
    <div class="flex items-center gap-2">
      <h2 id="mcp-title" class="text-base font-medium">{t('conn.mcp_title')}<HelpPopover text={t('conn.mcp_desc')} label={t('conn.mcp_title')} /></h2>
      {#if loaded}
        <Badge variant="outline" class="font-num text-muted-foreground">{t('conn.mcp_count', { enabled: enabledCount, total: entries.length })}</Badge>
      {/if}
    </div>
    <div class="flex items-center gap-1.5">
      <Button variant="ghost" size="sm" onclick={openRaw} disabled={!loaded}><Braces /> {t('conn.mcp_raw_open')}</Button>
      <Button variant="outline" size="sm" onclick={openImport} disabled={!loaded}><Download /> {t('conn.mcp_import')}</Button>
      <Button size="sm" onclick={() => openForm(null)} disabled={!loaded}><Plus /> {t('conn.mcp_add')}</Button>
    </div>
  </div>

  <div class="flex flex-col gap-1 text-xs text-muted-foreground">
        <p class="flex items-center gap-1.5"><Lock class="size-3 shrink-0" /> {t('conn.mcp_note_plaintext')}</p>
    {#if app.isRunning}
      <p class="flex items-center gap-1.5 text-warning"><Info class="size-3 shrink-0" /> {t('conn.mcp_note_restart')}</p>
    {/if}
  </div>

  {#if !loaded && !loadError}
    <SectionLoading shape="rows" rows={2} label={t('sl.conn_label')} phases={[t('sl.conn_2'), t('sl.conn_1')]} />
  {/if}

  {#if loadError}
    <p class="flex flex-wrap items-center gap-2 text-xs text-destructive" role="alert">
      {t('conn.mcp_load_error', { message: loadError })}
      <Button variant="outline" size="xs" onclick={load}>{t('conn.mcp_retry')}</Button>
    </p>
  {/if}

  {#if !formOpen && !importOpen && !rawOpen && !deleteOpen && saveError}
    <p class="text-xs whitespace-pre-line text-destructive" role="alert">{saveError}</p>
  {/if}

  {#if loaded && entries.length === 0}
    <Empty.Root class="border">
      <Empty.Header>
        <Empty.Media variant="icon"><Server /></Empty.Media>
        <Empty.Description>{t('conn.mcp_empty')}</Empty.Description>
      </Empty.Header>
      <Button variant="secondary" size="sm" onclick={() => openForm(null)}><Plus /> {t('conn.mcp_add')}</Button>
    </Empty.Root>
  {:else if entries.length > 0}
    <ul class="flex flex-col divide-y divide-border overflow-hidden rounded-lg border border-border bg-card" aria-label={t('conn.mcp_list_label')}>
      {#each entries as [name, def] (name)}
        <li class={cn('flex items-center gap-3 p-3', def.disabled && 'opacity-60')}>
          <div class="flex min-w-0 flex-1 flex-col gap-1">
            <div class="flex flex-wrap items-center gap-1.5">
              <span class="font-mono text-sm font-medium">{name}</span>
              <Badge variant="outline" class="text-muted-foreground">{def.url && !def.command ? 'HTTP' : 'stdio'}</Badge>
              {#if def.env && Object.keys(def.env).length}
                <Badge variant="outline" class="text-muted-foreground">{t('conn.mcp_env_count', { n: Object.keys(def.env).length })}</Badge>
              {/if}
              {#if def.headers && Object.keys(def.headers).length}
                <Badge variant="outline" class="text-muted-foreground">{t('conn.mcp_headers_count', { n: Object.keys(def.headers).length })}</Badge>
              {/if}
            </div>
            <code class="truncate font-mono text-xs text-muted-foreground" title={describe(def)}>{describe(def)}</code>
          </div>
          <div class="flex shrink-0 items-center gap-1">
            <Tooltip.Root>
              <Tooltip.Trigger>
                {#snippet child({ props })}
                  <span {...props} class="mr-1 flex items-center">
                    <FieldHelp text={t('help.mcp_toggle')}><Switch
                      size="sm"
                      checked={!def.disabled}
                      disabled={saving}
                      onCheckedChange={(v) => toggleEnabled(name, v)}
                      aria-label={`${t('conn.mcp_enabled')}: ${name}`}
                    /></FieldHelp>
                  </span>
                {/snippet}
              </Tooltip.Trigger>
              <Tooltip.Content>{t('conn.mcp_enabled')}</Tooltip.Content>
            </Tooltip.Root>
            {@render iconAction(t('conn.mcp_edit_aria', { name }), () => openForm(name), editIcon)}
            {@render iconAction(t('conn.mcp_delete_aria', { name }), () => askDelete(name), deleteIcon, true)}
          </div>
        </li>
      {/each}
    </ul>
  {/if}

</section>

<div hidden={page !== 'import'}>
  {#if loaded && importVisited}
    <McpImport standalone existingNames={names} {saving} error={saveError} onImport={importServers} onClose={() => onNavigate('servers')} />
  {:else if loadError}<p role="alert" class="text-xs text-destructive">{loadError}</p><Button onclick={load}>{t('conn.mcp_retry')}</Button>
  {:else}<SectionLoading shape="rows" rows={2} label={t('sl.conn_label')} />{/if}
</div>
<div hidden={page !== 'status'}>
  {#if loaded && statusVisited}<McpClientStatus reloadKey={statusKey} />{/if}
</div>

<!-- Add / edit -->
<Dialog.Root bind:open={formOpen}>
  <Dialog.Content class="sm:max-w-lg">
    <McpServerForm
      initial={formInitial}
      takenNames={formTarget === null ? names : names.filter((n) => n !== formTarget)}
      {saving}
      error={saveError}
      onSubmit={submitForm}
      onCancel={() => (formOpen = false)}
    />
  </Dialog.Content>
</Dialog.Root>

<!-- Import -->
<Dialog.Root bind:open={importOpen}>
  <Dialog.Content class="sm:max-w-lg">
    <McpImport existingNames={names} {saving} error={saveError} onImport={importServers} onClose={() => (importOpen = false)} />
  </Dialog.Content>
</Dialog.Root>

<!-- Raw JSON -->
<Dialog.Root bind:open={rawOpen}>
  <Dialog.Content class="sm:max-w-2xl">
    <Dialog.Header>
      <Dialog.Title>{t('conn.mcp_raw_title')}</Dialog.Title>
      <Dialog.Description class="font-mono text-xs">{t('conn.mcp_raw_label')}</Dialog.Description>
    </Dialog.Header>
    <FieldHelp text={t('help.mcp_raw')}><Textarea
      bind:value={rawText}
      rows={14}
      spellcheck="false"
      class="max-h-[50vh] font-mono text-xs"
      aria-label={t('conn.mcp_raw_label')}
      aria-invalid={!!rawError}
    /></FieldHelp>
    {#if rawError}<p class="text-xs whitespace-pre-line text-destructive" role="alert">{rawError}</p>{/if}
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (rawOpen = false)}>{t('conn.mcp_cancel')}</Button>
      <Button onclick={saveRaw} disabled={saving}>{saving ? t('conn.mcp_saving') : t('conn.mcp_raw_save')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- Delete confirmation -->
<Dialog.Root bind:open={deleteOpen}>
  <Dialog.Content class="sm:max-w-sm">
    <Dialog.Header>
      <Dialog.Title>{t('conn.mcp_delete_confirm', { name: deleteName })}</Dialog.Title>
    </Dialog.Header>
    {#if saveError}<p class="text-xs whitespace-pre-line text-destructive" role="alert">{saveError}</p>{/if}
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (deleteOpen = false)}>{t('conn.mcp_cancel')}</Button>
      <Button variant="destructive" onclick={confirmDelete} disabled={saving}>{t('conn.mcp_delete_yes')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
