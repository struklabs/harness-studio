<script lang="ts">
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  import FieldHelp from '$lib/components/common/FieldHelp.svelte';
  import { onDestroy } from 'svelte';
  import { invoke } from '$lib/ipc';
  import { toast } from 'svelte-sonner';
  import { FolderOpen, Plus, Trash2, Star } from '@lucide/svelte';
  import type { AppSettings, StorageDirInfo } from '$lib/types';
  import { formatBytes } from '$lib/format';
  import { t } from '$lib/i18n.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Input } from '$lib/components/ui/input';
  import { Badge } from '$lib/components/ui/badge';
  import * as Tooltip from '$lib/components/ui/tooltip';
  import { cn } from '$lib/utils';
  import ImportModal from '../ImportModal.svelte';
  import HfAccountSettings from './HfAccountSettings.svelte';
  import SettingGroup from './SettingGroup.svelte';
  import SettingRow from './SettingRow.svelte';
  import SwitchRow from './SwitchRow.svelte';
  import type { SettingsDraft } from './form.svelte';
  import { pathExample } from '$lib/platform';
  import { library } from '$lib/stores/library.svelte';

  interface Props {
    page?: string;
    settings: AppSettings;
    form: SettingsDraft;
    onSettingsUpdated: (updated: AppSettings) => void;
  }

  let { page = 'storage-directories', settings, form, onSettingsUpdated }: Props = $props();

  const fmt = (b: number) => formatBytes(b, 1);

  let storageDirs = $state<StorageDirInfo[]>([]);
  let newDirPath = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);
  let showImportModal = $state(false);
  let destroyed = false;
  onDestroy(() => (destroyed = true));

  let primary = $derived(storageDirs.find((d) => d.is_primary));
  let extras = $derived(storageDirs.filter((d) => !d.is_primary));

  async function loadStorageDirs() {
    try {
      const dirs: StorageDirInfo[] = await invoke('get_storage_directories');
      if (!destroyed) storageDirs = Array.isArray(dirs) ? dirs : [];
    } catch (e) {
      if (!destroyed) {
        storageDirs = [];
        error = t('settings.storage_load_failed', { error: String(e) });
      }
    }
  }

  // Re-read the list whenever the saved folders change (here, in the import dialog or elsewhere).
  $effect(() => {
    void settings.models_dir;
    void settings.extra_models_dirs.join('\n');
    loadStorageDirs();
  });

  /** Runs one folder command, shows the outcome and refreshes the list. */
  async function run(command: string, args: Record<string, unknown>, ok: (s: AppSettings) => string, failKey: 'settings.dir_add_failed' | 'settings.dir_remove_failed' | 'status.error_detail') {
    if (busy) return false;
    busy = true;
    error = null;
    try {
      const updated: AppSettings = await invoke(command, args);
      onSettingsUpdated(updated);
      // Dashboard and My library list models from these folders: rescan them.
      library.markChanged();
      toast.success(ok(updated));
      await loadStorageDirs();
      return true;
    } catch (e) {
      error = failKey === 'status.error_detail' ? t('status.error_detail', { message: String(e) }) : t(failKey, { error: String(e) });
      return false;
    } finally {
      busy = false;
    }
  }

  async function pickDir(): Promise<string | null> {
    error = null;
    try {
      return await invoke<string | null>('pick_directory_dialog');
    } catch (e) {
      error = t('settings.folder_pick_failed', { error: String(e) });
      return null;
    }
  }

  async function setPrimaryDir(path: string) {
    await run('set_primary_storage_directory', { dirPath: path }, (u) => t('settings.primary_dir_updated', { path: u.models_dir }), 'status.error_detail');
  }

  async function pickPrimaryDir() {
    const picked = await pickDir();
    if (picked) await setPrimaryDir(picked);
  }

  async function addExtraDir(path = newDirPath) {
    const dir = path.trim();
    if (!dir) return;
    const ok = await run('add_storage_directory', { dirPath: dir }, () => t('settings.dir_added'), 'settings.dir_add_failed');
    if (ok) newDirPath = '';
  }

  async function pickExtraDir() {
    const picked = await pickDir();
    if (picked) await addExtraDir(picked);
  }

  async function removeExtraDir(path: string) {
    await run('remove_storage_directory', { dirPath: path }, () => t('settings.dir_removed'), 'settings.dir_remove_failed');
  }

  const usedPct = (d: StorageDirInfo) => (d.total_bytes > 0 ? Math.min(100, ((d.total_bytes - d.free_bytes) / d.total_bytes) * 100) : 0);
</script>

{#snippet stats(d: StorageDirInfo)}
  <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
    <Badge variant="outline" class={d.exists ? 'text-success' : 'text-destructive'}>
      {d.exists ? t('settings.accessible') : t('settings.not_accessible')}
    </Badge>
    <span class="font-num">{t('settings.free_of', { free: fmt(d.free_bytes), total: fmt(d.total_bytes) })}</span>
    <AdvancedOnly><span class="font-num">{t('settings.models_count', { count: d.model_count })}</span></AdvancedOnly>
  </div>
{/snippet}

<div hidden={page !== 'storage-directories'} class="flex flex-col gap-6">
  {#if error}
    <p class="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs break-words text-destructive" role="alert">{error}</p>
  {/if}

  <SettingGroup title={t('settings.primary_dir_title')} hint={t('settings.primary_dir_desc')}>
    <div class="flex min-w-0 flex-col gap-2 px-4 py-3">
      {#if primary}
        <div class="flex min-w-0 items-center justify-between gap-4">
          <span class="min-w-0 truncate font-mono text-sm" title={primary.path}>{primary.path}</span>
          <Button variant="outline" size="sm" onclick={pickPrimaryDir} disabled={busy}>
            <FolderOpen /> {t('settings.pick_change_folder')}
          </Button>
        </div>
        {@render stats(primary)}
        <div class="h-1 overflow-hidden rounded-full bg-muted" aria-hidden="true">
          <div class={cn('h-full', usedPct(primary) >= 92 ? 'bg-destructive' : usedPct(primary) >= 80 ? 'bg-warning' : 'bg-foreground/60')} style="width: {usedPct(primary)}%"></div>
        </div>
      {:else}
        <div class="flex min-w-0 items-center justify-between gap-4">
          <span class="min-w-0 truncate font-mono text-sm" title={settings.models_dir}>{settings.models_dir}</span>
          <Button variant="outline" size="sm" onclick={pickPrimaryDir} disabled={busy}>
            <FolderOpen /> {t('settings.pick_change_folder')}
          </Button>
        </div>
      {/if}
    </div>
  </SettingGroup>

  <SettingGroup title={t('settings.extra_dirs_title')} hint={t('settings.extra_dirs_desc')}>
    {#each extras as dir (dir.path)}
      <div class="flex min-w-0 items-center justify-between gap-4 px-4 py-3">
        <div class="flex min-w-0 flex-col gap-1.5">
          <span class="truncate font-mono text-sm" title={dir.path}>{dir.path}</span>
          {@render stats(dir)}
        </div>
        <div class="flex shrink-0 items-center gap-1">
          <Tooltip.Root>
            <Tooltip.Trigger>
              {#snippet child({ props })}
                <Button {...props} variant="ghost" size="icon-sm" aria-label={t('settings.make_primary_title')} disabled={busy} onclick={() => setPrimaryDir(dir.path)}>
                  <Star />
                </Button>
              {/snippet}
            </Tooltip.Trigger>
            <Tooltip.Content>{t('settings.make_primary_title')}</Tooltip.Content>
          </Tooltip.Root>
          <Tooltip.Root>
            <Tooltip.Trigger>
              {#snippet child({ props })}
                <Button {...props} variant="ghost" size="icon-sm" class="text-destructive hover:text-destructive" aria-label={t('settings.remove_title')} disabled={busy} onclick={() => removeExtraDir(dir.path)}>
                  <Trash2 />
                </Button>
              {/snippet}
            </Tooltip.Trigger>
            <Tooltip.Content>{t('settings.remove_title')}</Tooltip.Content>
          </Tooltip.Root>
        </div>
      </div>
    {:else}
      <p class="px-4 py-4 text-center text-xs text-muted-foreground">{t('settings.no_extra_dirs')}</p>
    {/each}

    <div class="flex items-center gap-2 bg-muted/30 px-4 py-3">
      <FieldHelp text={t('help.storage_dir')}><Input
        class="h-8 min-w-0 flex-1 font-mono text-xs"
        placeholder={t('settings.dir_path_placeholder', { example: pathExample('D:\\Models', '~/models') })}
        aria-label={t('settings.add_dir')}
        bind:value={newDirPath}
        onkeydown={(e) => {
          if (e.key === 'Enter') addExtraDir();
        }}
      /></FieldHelp>
      <Button variant="outline" size="sm" onclick={pickExtraDir} disabled={busy}>{t('settings.browse')}</Button>
      <Button variant="secondary" size="sm" onclick={() => addExtraDir()} disabled={busy || !newDirPath.trim()}>
        <Plus /> {t('settings.add_dir')}
      </Button>
    </div>
  </SettingGroup>

  <SettingGroup title={t('settings.downloads_title')}>
    <SwitchRow
      id="auto-load-after-download"
      label={t('settings.auto_load_after_download')}
      hint={t('settings.auto_load_after_download_desc')}
      checked={form.get('auto_load_after_download')}
      dirty={form.isDirty('auto_load_after_download')}
      onchange={(v) => form.set('auto_load_after_download', v)}
    />
  </SettingGroup>

  <SettingGroup title={t('settings.import_title')}>
    <SettingRow label={t('imp.settings_btn')} hint={t('imp.open_title')}>
      <Button variant="outline" size="sm" onclick={() => (showImportModal = true)}>{t('settings.import_open')}</Button>
    </SettingRow>
  </SettingGroup>
</div>

<div hidden={page !== 'storage-hf'}><HfAccountSettings {settings} {onSettingsUpdated} /></div>

{#if showImportModal}
  <ImportModal
    onClose={() => (showImportModal = false)}
    onImported={loadStorageDirs}
    onSettingsChanged={(updated) => onSettingsUpdated(updated)}
  />
{/if}
