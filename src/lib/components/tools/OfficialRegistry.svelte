<script lang="ts">
  import { app } from '#lib/stores/app.svelte.js';
  import TrustPermissions from "./TrustPermissions.svelte";
  import { onMount } from 'svelte';
  import { emit } from '@tauri-apps/api/event';
  import { invoke } from '#lib/ipc.js';
  import { getLanguage, t, type TKey } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import type { InstalledTool } from '#lib/tools-client.js';
  import type { McpConfig } from '#lib/mcp-types.js';
  import { validateMcpConfig } from '#lib/mcp-client.js';
  import SettingGroup from '../settings/SettingGroup.svelte';
  import type { Skill } from './SkillsSettings.svelte';
  import { filterCatalog, mergeRegistryServers, type RegistryEntry as Entry } from './registryCatalog';

  interface Snapshot { commit: string; offline: boolean; index: { entries: Entry[] } }
  let { installed, onTool, onSkill = () => {}, skillsRevision = 0, reviewing = false }: { installed: InstalledTool[]; onTool: (path: string) => Promise<void>; onSkill?: () => void; skillsRevision?: number; reviewing?: boolean } = $props();
  let skills = $state<Skill[]>([]);
  $effect(() => { void skillsRevision; void invoke<Skill[]>('skills_list').then(value => skills = value).catch(e => error = localizeError(e)); });
  let pendingSkill = $state<{ entry: Entry; text: string } | null>(null);
  let snapshot = $state<Snapshot | null>(null);
  let config = $state<McpConfig>({ mcpServers: {} });
  let pending = $state<{ entry: Entry; config: McpConfig } | null>(null);
  let busy = $state(false);
  let error = $state('');
  let query = $state('');
  let kind = $state('all');
  $effect(() => {
    if (app.requestedRegistryKind) { kind = app.requestedRegistryKind; query = ''; app.requestedRegistryKind = null; }
  });
  let sort = $state('name');
  let visible = $derived(filterCatalog(snapshot?.index.entries ?? [], query, kind, sort, getLanguage()));
  const key = (e: Entry) => e.id.split('.').at(-1)!;
  const isInstalled = (e: Entry) => e.type === 'tool' ? installed.some(i => i.name === key(e)) : e.type === 'skill' ? skills.some(s => s.name === key(e)) : !!config.mcpServers[key(e)];

  async function act(work: () => Promise<void>) {
    if (busy) return;
    busy = true; error = '';
    try { await work(); } catch (e) { error = localizeError(e); } finally { busy = false; }
  }
  const load = () => act(async () => {
    const [catalog, current] = await Promise.all([invoke<Snapshot>('official_registry_list'), invoke<McpConfig>('get_mcp_config')]);
    snapshot = catalog; config = current;
    skills = await invoke<Skill[]>('skills_list');
  });
  onMount(() => void load());
  const prepare = (entry: Entry) => act(async () => {
    if (!snapshot) return;
    const result = await invoke<{ path: string | null; config: McpConfig | null; skill: string | null }>('official_registry_prepare', { commit: snapshot.commit, id: entry.id });
    if (result.skill) pendingSkill = { entry, text: result.skill };
    if (result.path) await onTool(result.path);
    if (result.config) pending = { entry, config: result.config };
  });
  const installSkill = () => act(async () => {
    if (!pendingSkill || !snapshot) return;
    await invoke('official_skill_install', { commit: snapshot.commit, id: pendingSkill.entry.id });
    await emit('tools-changed');
    skills = await invoke<Skill[]>('skills_list'); pendingSkill = null; onSkill();
  });
  const confirm = () => act(async () => {
    if (!pending) return;
    const current = await invoke<McpConfig>('get_mcp_config');
    // Never overwrite a server (or its credentials) that was added during review.
    const servers = mergeRegistryServers(current.mcpServers, pending.config.mcpServers);
    if (!servers) throw new Error(t('tools.registry_conflict'));
    const next = { mcpServers: servers };
    if (validateMcpConfig(next).length) throw new Error(t('tools.registry_invalid'));
    await invoke('save_mcp_config', { config: next });
    config = next; pending = null;
  });
  const removeMcp = (entry: Entry) => act(async () => {
    const current = await invoke<McpConfig>('get_mcp_config');
    delete current.mcpServers[key(entry)];
    await invoke('save_mcp_config', { config: current });
    config = current; pending = null;
  });
</script>

<SettingGroup title={t('tools.registry_title')} hint={t('tools.registry_desc')}>
  <div class="flex flex-col gap-3 p-4">
    <p class="text-xs text-muted-foreground">{t('discover.registry_scope')}</p>
    <div class="flex flex-wrap items-center gap-2">
      <Input class="min-w-40 flex-1" aria-label={t('tools.registry_search')} placeholder={t('tools.registry_search')} bind:value={query} />
      <label class="text-xs">{t('tools.registry_type')}
        <select class="rounded-md border bg-background p-2" bind:value={kind}>
          {#each ['all', 'tool', 'skill', 'mcp'] as value}<option value={value}>{t(('tools.registry_' + value) as TKey)}</option>{/each}
        </select>
      </label>
      <label class="text-xs">{t('tools.registry_sort')}
        <select class="rounded-md border bg-background p-2" bind:value={sort}>
          <option value="name">{t('tools.registry_name')}</option><option value="type">{t('tools.registry_type')}</option>
        </select>
      </label>
      <Button size="sm" variant="outline" disabled={busy || pending !== null || pendingSkill !== null || reviewing} onclick={load}>{t('create.refresh')}</Button>
    </div>
    {#if error}<p class="text-xs text-destructive" role="alert">{t('tools.load_failed', { error })}</p>{/if}
    {#if busy}<p class="text-xs text-muted-foreground" role="status">{t('tools.registry_loading')}</p>{/if}
    {#if snapshot?.offline}<p class="text-xs text-warning" role="status">{t('tools.registry_offline')}</p>{/if}
    {#each visible as entry (entry.id)}
      <article class="flex flex-col gap-2 rounded-md border p-3">
        <div class="flex flex-wrap items-center gap-2"><h3 class="text-sm font-medium">{entry.name}</h3><span class="text-xs text-muted-foreground">{t(('tools.registry_' + entry.type) as TKey)} · {t('tools.registry_version')}: {entry.version} · {t('tools.registry_license')}: {entry.license}</span></div>
        <p class="text-xs text-muted-foreground">{entry.description[getLanguage()]}</p>
        {#if entry.type === 'tool'}<TrustPermissions kind="scoped" />{/if}
        {#if entry.type === 'mcp' && config.mcpServers[key(entry)]}
          <TrustPermissions kind={config.mcpServers[key(entry)].command ? 'native' : 'remote'} />
        {/if}
        <p class="text-xs font-medium">{t('tools.registry_permissions')}</p>
        <ul class="list-disc pl-4 text-xs text-muted-foreground">
          <li>{entry.permissions.network.length ? t('tools.inst_perm_network', { hosts: entry.permissions.network.join(', ') }) : t('tools.inst_perm_no_network')}</li>
          {#if entry.permissions.fs.read.length}<li>{t('tools.inst_perm_read', { folders: entry.permissions.fs.read.join(', ') })}</li>{/if}
          {#if entry.permissions.fs.write.length}<li>{t('tools.inst_perm_write', { folders: entry.permissions.fs.write.join(', ') })}</li>{/if}
          {#if !entry.permissions.fs.read.length && !entry.permissions.fs.write.length}<li>{t('tools.inst_perm_no_files')}</li>{/if}
          {#if entry.type !== 'mcp'}<li>{t('tools.registry_no_shell')}</li>{/if}
        </ul>
        {#if entry.type === 'mcp'}<p class="text-xs text-warning">{t('tools.registry_mcp_warning')}</p>{/if}
        <div class="flex flex-wrap items-center gap-2">
          <Button variant="link" size="sm" onclick={() => act(() => openUrl(entry.source))}>{t('tools.registry_source')}</Button>
          {#if isInstalled(entry)}
            <span class="text-xs text-success">{t('tools.registry_installed')}</span>
            {#if entry.type === 'mcp'}<Button variant="outline" size="sm" disabled={busy} onclick={() => pending = { entry, config: { mcpServers: {} } }}>{t('tools.inst_remove')}</Button>{/if}
          {:else}<Button size="sm" disabled={busy || pending !== null || pendingSkill !== null || reviewing} onclick={() => prepare(entry)}>{t('tools.registry_install')}</Button>{/if}
        </div>
        {#if pendingSkill?.entry.id === entry.id}
          <div class="flex flex-col gap-2 rounded-md border p-3">
            <p class="text-sm font-medium">{t('tools.registry_confirm')}</p>
            <p class="text-xs text-muted-foreground">{t('tools.skills_review')}</p>
            <pre class="max-h-64 overflow-auto text-xs whitespace-pre-wrap break-words">{pendingSkill.text}</pre>
            <div class="flex gap-2"><Button size="sm" variant="ghost" disabled={busy} onclick={() => pendingSkill = null}>{t('tools.inst_cancel')}</Button><Button size="sm" disabled={busy} onclick={installSkill}>{t('tools.inst_confirm_ok')}</Button></div>
          </div>
        {/if}
        {#if pending?.entry.id === entry.id}
          {@const removing = Object.keys(pending.config.mcpServers).length === 0}
          <div class="flex flex-col gap-2 rounded-md border p-3" role="group" aria-label={t('tools.registry_confirm')}>
            <p class="text-sm font-medium">{t('tools.registry_confirm')}</p>
            {#if !removing}
              {#each Object.entries(pending.config.mcpServers) as [name, server] (name)}
                <p class="text-xs font-medium">{name}</p><TrustPermissions kind={server.command ? 'native' : 'remote'} />
              {/each}
              <pre class="overflow-auto text-xs whitespace-pre-wrap break-all">{JSON.stringify(pending.config, null, 2)}</pre>
            {/if}
            <div class="flex gap-2"><Button size="sm" variant="ghost" disabled={busy} onclick={() => pending = null}>{t('tools.inst_cancel')}</Button><Button size="sm" disabled={busy} onclick={() => removing ? removeMcp(entry) : confirm()}>{removing ? t('tools.inst_remove') : t('tools.inst_confirm_ok')}</Button></div>
          </div>
        {/if}
      </article>
    {:else}
      {#if snapshot && !busy}<p class="text-xs text-muted-foreground">{t('tools.registry_empty')}</p>{/if}
    {/each}
  </div>
</SettingGroup>
