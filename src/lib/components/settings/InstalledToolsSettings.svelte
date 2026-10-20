<script lang="ts">
  import TrustPermissions from "#lib/components/tools/TrustPermissions.svelte";
  // Settings > Tools > Installed tools: packages kept in
  // `<data dir>/tools/`. Installing shows what the manifest asks for and waits for the user's
  // confirmation; the core validates the package again when it installs it. Switching and
  // removing re-register the tools in the core's registry, so the Playground, the CLI and the
  // Control API see the change at once.
  import { onMount } from 'svelte';
  import { toast } from 'svelte-sonner';
  import { FlaskConical, FolderOpen, ShieldAlert, Trash2 } from '@lucide/svelte';
  import { invoke } from '#lib/ipc.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { cn } from '#lib/utils.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Switch } from '#lib/components/ui/switch/index.js';
  import {
    inspectInstalledTool,
    installInstalledTool,
    listInstalledTools,
    removeInstalledTool,
    setInstalledToolEnabled,
    testInstalledTool,
    type InstalledTool,
    type PermissionClass,
    type TestReport,
  } from '#lib/tools-client.js';
  import ToolTestReport from '#lib/components/tools/ToolTestReport.svelte';
  import OfficialRegistry from '#lib/components/tools/OfficialRegistry.svelte';
  import SkillsSettings from '#lib/components/tools/SkillsSettings.svelte';
  import SettingGroup from './SettingGroup.svelte';
  import SettingRow from './SettingRow.svelte';

  let { onChanged = () => {}, showTitle = true }: { onChanged?: () => void; showTitle?: boolean } = $props();

  let files = $state<Record<string, { manifest_json: string; code: string; test: string }>>({});
  const inspect = (name: string) => act(async () => { if (files[name]) delete files[name]; else files[name] = await invoke('installed_tools_files', { name }); });

  let items = $state<InstalledTool[]>([]);
  let skillsRevision = $state(0);
  let pending = $state<{ path: string; tool: InstalledTool } | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  /** The last test result per tool name (a removed or switched tool keeps none). */
  let reports = $state<Record<string, TestReport>>({});
  let testing = $state<string | null>(null);

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
      items = await listInstalledTools();
      error = null;
    } catch (e) {
      error = localizeError(e);
    }
  }

  onMount(() => void load());

  async function act(work: () => Promise<void>) {
    if (busy) return;
    busy = true;
    try {
      await work();
    } catch (e) {
      toast.error(t('tools.inst_failed', { error: localizeError(e) }));
    } finally {
      busy = false;
    }
  }

  const pick = () =>
    act(async () => {
      const dir = await invoke<string | null>('pick_directory_dialog').catch(() => null);
      if (!dir) return;
      pending = { path: dir, tool: await inspectInstalledTool(dir) };
    });

  const confirmInstall = () =>
    act(async () => {
      if (!pending) return;
      const done = await installInstalledTool(pending.path);
      pending = null;
      toast.success(t('tools.inst_installed', { name: done.name }));
      await load();
      onChanged();
    });

  const toggle = (name: string, on: boolean) =>
    act(async () => {
      await setInstalledToolEnabled(name, on);
      await load();
      onChanged();
    });

  const runTest = (name: string) =>
    act(async () => {
      testing = name;
      try {
        reports[name] = await testInstalledTool(name);
      } finally {
        testing = null;
      }
    });

  const remove = (name: string) =>
    act(async () => {
      await removeInstalledTool(name);
      delete reports[name];
      delete files[name];
      toast.success(t('tools.inst_removed', { name }));
      await load();
      onChanged();
    });

  /** The permission lines of a package, as the install prompt and the list show them. */
  function permissionLines(tool: InstalledTool): string[] {
    const lines = [
      tool.network.includes('*') ? t('tools.network_any') : tool.network.length
        ? t('tools.inst_perm_network', { hosts: tool.network.join(', ') })
        : t('tools.inst_perm_no_network'),
    ];
    if (tool.fs_read.length) lines.push(t('tools.inst_perm_read', { folders: tool.fs_read.join(', ') }));
    if (tool.fs_write.length) lines.push(t('tools.inst_perm_write', { folders: tool.fs_write.join(', ') }));
    if (!tool.fs_read.length && !tool.fs_write.length) lines.push(t('tools.inst_perm_no_files'));
    lines.push(t('tools.inst_perm_shell'));
    return lines;
  }
</script>

<TrustPermissions kind="scoped" />
<SkillsSettings revision={skillsRevision} onChanged={() => skillsRevision++} />
<OfficialRegistry installed={items} {skillsRevision} onSkill={() => skillsRevision++} reviewing={pending !== null} onTool={async path => { pending = { path, tool: await inspectInstalledTool(path) }; }} />

<SettingGroup title={t('tools.inst_title')} {showTitle} hint={t('tools.inst_desc')}>
  {#if error}
    <p class="px-4 py-2 text-xs text-destructive" role="alert">{t('tools.load_failed', { error })}</p>
  {/if}

  {#if pending}
    <div class="flex flex-col gap-2 border-b p-4" role="group" aria-label={t('tools.inst_confirm_title', { name: pending.tool.name, version: pending.tool.version })}>
      <p class="text-sm font-medium">{t('tools.inst_confirm_title', { name: pending.tool.name, version: pending.tool.version })}</p>
      <p class="text-xs text-muted-foreground">{pending.tool.description}</p>
      <TrustPermissions kind="scoped" />
      <p class="text-xs text-muted-foreground">{t('tools.inst_confirm_asks')}</p>
      <ul class="list-disc pl-5 text-xs">
        {#each permissionLines(pending.tool) as line (line)}
          <li>{line}</li>
        {/each}
      </ul>
      <div class="flex items-center justify-end gap-2">
        <Badge variant="outline" class={cn('mr-auto gap-1', classTone[pending.tool.class])}>
          {#if pending.tool.class === 'dangerous'}<ShieldAlert />{/if}
          {t(classLabel[pending.tool.class])}
        </Badge>
        <Button variant="ghost" size="sm" disabled={busy} onclick={() => (pending = null)}>{t('tools.inst_cancel')}</Button>
        <Button size="sm" disabled={busy} onclick={confirmInstall}>{t('tools.inst_confirm_ok')}</Button>
      </div>
    </div>
  {/if}

  {#each items as tool (tool.name)}
    <SettingRow
      label={tool.name + (tool.version ? ` ${tool.version}` : '') + ' · ' + t(tool.user_created ? 'tools.origin_created' : 'tools.origin_installed')}
      note={tool.error ? t('tools.inst_broken', { error: localizeError(tool.error) }) : `${tool.description} · ${permissionLines(tool).join(' · ')}`}
      for="inst-tool-{tool.name}"
    >
      {#if !tool.error}
        <Button variant="ghost" size="sm" disabled={busy} onclick={() => inspect(tool.name)}>{t('create.details')}</Button>
        <Badge variant="outline" class={cn('gap-1', classTone[tool.class])}>
          {#if tool.class === 'dangerous'}<ShieldAlert />{/if}
          {t(classLabel[tool.class])}
        </Badge>
        <Switch
          id="inst-tool-{tool.name}"
          disabled={busy}
          checked={tool.enabled}
          aria-label={t('tools.inst_enabled_aria', { name: tool.name })}
          onCheckedChange={(v) => void toggle(tool.name, v)}
        />
        {#if tool.has_test}
          <Button variant="ghost" size="icon-sm" disabled={busy} aria-label={t('tools.inst_test')} title={t('tools.inst_test')} onclick={() => runTest(tool.name)}>
            <FlaskConical class={cn(testing === tool.name && 'animate-pulse')} />
          </Button>
        {/if}
      {/if}
      <Button variant="ghost" size="icon-sm" disabled={busy} aria-label={t('tools.inst_remove')} title={t('tools.inst_remove')} onclick={() => remove(tool.name)}>
        <Trash2 />
      </Button>
    </SettingRow>
    {#if files[tool.name]}
      <div class="flex min-w-0 flex-col gap-2 border-b px-4 py-3">
        {#each [['tools.gen_manifest', files[tool.name].manifest_json], ['tools.gen_code', files[tool.name].code], ['tools.gen_test_code', files[tool.name].test]] as [label, code]}
          <p class="text-xs font-medium">{t(label as TKey)}</p><pre class="max-h-56 overflow-auto rounded-md bg-muted p-2 font-mono text-xs whitespace-pre">{code}</pre>
        {/each}
        {#if tool.readme}<pre class="overflow-auto text-xs whitespace-pre-wrap">{tool.readme}</pre>{/if}
      </div>
    {/if}
    {#if reports[tool.name]}
      <div class="border-b px-4 py-2.5">
        <ToolTestReport report={reports[tool.name]} />
      </div>
    {/if}
  {:else}
    {#if !error}
      <p class="px-4 py-3 text-xs text-muted-foreground">{t('tools.inst_empty')}</p>
    {/if}
  {/each}

  <div class="flex justify-end p-3">
    <Button variant="outline" size="sm" disabled={busy || pending !== null} onclick={pick}>
      <FolderOpen />
      {t('tools.inst_install')}
    </Button>
  </div>
</SettingGroup>
