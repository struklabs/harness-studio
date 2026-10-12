<script lang="ts">
  import { onMount } from 'svelte';
  import { FolderOpen, Check, TriangleAlert } from '@lucide/svelte';
  import { toast } from 'svelte-sonner';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
  import { Button } from '$lib/components/ui/button';
  import { invoke } from '$lib/ipc';
  import { t } from '$lib/i18n.svelte';
  import { localizeError } from '$lib/network';
  import { security } from '$lib/stores/security.svelte';
  import { getToolSettings, listCoreTools, setToolSettings } from '$lib/tools-client';
  import { recentFolders, folderName, sameFolder, NO_WORKSPACE } from '$lib/workspace-folders';

  let { value = $bindable(''), disabled = false, conversationKey }: { value: string; disabled?: boolean; conversationKey: number } = $props();
  let folders = $state<string[]>([]);
  let missing = $state<string[]>([]);
  let busy = $state(false);
  let defaultFolder = $state('');
  let effectiveFolder = $derived(value === NO_WORKSPACE ? '' : value || defaultFolder);
  let label = $derived(effectiveFolder ? `${folderName(effectiveFolder)}${!value ? ` (${t('tools.workspace_default_suffix')})` : ''}` : t('tools.workspace_none'));
  async function load() {
    try {
      const listing = await listCoreTools();
      folders = listing.workspaces;
      missing = listing.missing_workspaces;
      defaultFolder = listing.workdir;
    } catch (e) { toast.error(localizeError(e)); }
  }
  onMount(() => {
    void load();
    const refresh = () => void load();
    window.addEventListener('workspace-folders-changed', refresh);
    return () => window.removeEventListener('workspace-folders-changed', refresh);
  });
  async function select(path: string) {
    if (busy) return;
    busy = true;
    // Commit synchronously to the current chat. No value writes after awaits: a chat
    // switched during MRU persistence must never receive the previous chat's choice.
    value = path;
    try {
      if (path && path !== NO_WORKSPACE) {
        const cfg = await getToolSettings();
        const saved = await setToolSettings({ ...cfg, workspaces: recentFolders(cfg.workspaces ?? [], path) });
        folders = saved.workspaces ?? [];
        window.dispatchEvent(new Event('workspace-folders-changed'));
      }
    } catch (e) { toast.error(localizeError(e)); }
    finally { busy = false; }
  }
  async function pick() {
    const initialConversation = conversationKey;
    try {
      const path = await invoke<string | null>('pick_directory_dialog');
      if (path && conversationKey === initialConversation) await select(path);
    } catch (e) { toast.error(localizeError(e)); }
  }
</script>

<DropdownMenu.Root onOpenChange={(open) => { if (open) void load(); }}>
  <DropdownMenu.Trigger>
    {#snippet child({ props })}
      <Button {...props} type="button" variant="ghost" size="xs" disabled={disabled || busy}
        data-workspace-picker title={effectiveFolder || t('tools.workspace_none')}
        aria-label={`${t('pg.tools_workdir')}: ${label}`}
        class="max-w-48 min-w-0 gap-1 text-muted-foreground">
        <FolderOpen class="size-3.5 shrink-0" />
        <span class="truncate">{label}</span>
      </Button>
    {/snippet}
  </DropdownMenu.Trigger>
  <DropdownMenu.Content side="top" align="start" class="max-h-80 w-80 max-w-[calc(100vw-2rem)] overflow-y-auto">
    <DropdownMenu.Label>{t('tools.workspaces_title')}</DropdownMenu.Label>
    {#each folders as path (path)}
      <DropdownMenu.Item onclick={() => void select(path)} title={path} class="min-w-0" disabled={busy}>
        <FolderOpen class="size-4 shrink-0" />
        <span class="min-w-0 flex-1 truncate">{folderName(path)}</span>
        {#if missing.includes(path)}<TriangleAlert class="size-4 shrink-0 text-warning" aria-label={t('tools.workspace_missing')} />{/if}
        {#if value && value !== NO_WORKSPACE && sameFolder(value, path)}<Check class="size-4 shrink-0" aria-label={t('tools.workspace_selected')} />{/if}
      </DropdownMenu.Item>
    {/each}
    <DropdownMenu.Separator />
    <DropdownMenu.Item onclick={() => void pick()} disabled={busy}><FolderOpen class="size-4" />{t('tools.workdir_pick')}</DropdownMenu.Item>
    <DropdownMenu.Item onclick={() => void select('')} disabled={busy}>{t('tools.workspace_follow_default')}{#if !value}<Check class="ml-auto size-4" />{/if}</DropdownMenu.Item>
    <DropdownMenu.Item onclick={() => void select(NO_WORKSPACE)} disabled={busy}>{t('tools.workspace_none')}{#if value === NO_WORKSPACE}<Check class="ml-auto size-4" />{/if}</DropdownMenu.Item>
    <DropdownMenu.Separator />
    <DropdownMenu.Item onclick={() => security.openSettings('tools')}>{t('tools.workspaces_manage')}</DropdownMenu.Item>
  </DropdownMenu.Content>
</DropdownMenu.Root>
