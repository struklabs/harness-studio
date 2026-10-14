<script lang="ts">
  // Settings > Developer > CLI > "Terminalde Kullan": where the bundled `harness` CLI is, its
  // version, and an optional PATH entry (Windows: user PATH; macOS/Linux: ~/.local/bin symlink).
  // Nothing is changed until the button is clicked; "Kaldır" undoes it.
  import { onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { toast } from 'svelte-sonner';
  import { Copy } from '@lucide/svelte';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import SettingGroup from './SettingGroup.svelte';
  import SettingRow from './SettingRow.svelte';

  interface CliStatus {
    path: string | null;
    source: 'app' | 'path' | 'user' | null;
    version: string | null;
    mode: 'windows' | 'symlink';
    target: string | null;
    registered: boolean;
    target_dir_on_path: boolean;
    translocated: boolean;
    appimage: boolean;
    on_path: boolean;
    profile: string | null;
  }

  let status = $state<CliStatus | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);

  const SOURCE: Record<string, TKey> = {
    app: 'cli.source_app',
    path: 'cli.source_path',
    user: 'cli.source_user',
  };

  async function refresh() {
    try {
      status = await invoke<CliStatus>('cli_status', { userPath: null });
    } catch (e) {
      error = String(e);
    }
  }
  onMount(refresh);

  async function run(cmd: 'cli_path_install' | 'cli_path_uninstall') {
    busy = true;
    error = null;
    try {
      await invoke(cmd, { userPath: null });
      await refresh();
      toast.success(t(cmd === 'cli_path_install' ? 'cli.added' : 'cli.removed'));
    } catch (e) {
      error = t('cli.failed', { message: String(e) });
    } finally {
      busy = false;
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      toast.success(t('dev.copied'));
    } catch (e) {
      toast.error(String(e));
    }
  }

  let exportLine = $derived(
    status?.target ? `export PATH="${status.target.replace(/[\\/][^\\/]*$/, '')}:$PATH"` : '',
  );
</script>

<SettingGroup title={t('cli.title')} hint={t('cli.desc')}>
  {#if status && status.path}
    <SettingRow label={t('cli.location')} hint={t(SOURCE[status.source ?? 'app'])}>
      <span class="max-w-[22rem] truncate font-mono text-xs" title={status.path}>{status.path}</span>
      <Button variant="ghost" size="icon-sm" onclick={() => copy(status?.path ?? '')} aria-label={t('dev.copy')} title={t('dev.copy')}>
        <Copy class="size-3.5" />
      </Button>
    </SettingRow>
    <SettingRow label={t('cli.version')}>
      <span class="font-num text-xs">{status.version ?? t('cli.version_unknown')}</span>
    </SettingRow>
    <SettingRow
      label={t('cli.path_label')}
      hint={status.mode === 'windows'
        ? t('cli.path_hint_windows')
        : [t('cli.path_hint_symlink', { target: status.target ?? '' }), status.profile ? t('cli.path_hint_profile', { profile: status.profile }) : '']
            .filter(Boolean)
            .join(' ')}
    >
      {#if status.on_path && !status.registered}
        <!-- A package install (the .deb's /usr/bin): already reachable, nothing to link. -->
        <Badge variant="outline" class="text-success">{t('cli.on_path')}</Badge>
      {:else if status.registered}
        <Badge variant="outline" class="text-success">{t('cli.on_path')}</Badge>
        <Button size="sm" variant="outline" disabled={busy} onclick={() => run('cli_path_uninstall')}>{t('cli.remove')}</Button>
      {:else}
        <Button size="sm" disabled={busy || status.translocated || status.appimage} onclick={() => run('cli_path_install')}>{t('cli.add')}</Button>
      {/if}
    </SettingRow>
    {#if (status.translocated || status.appimage) && !status.registered}
      <p class="px-4 pb-3 text-xs text-muted-foreground">{t(status.appimage ? 'cli_install.appimage' : 'cli_install.translocated')}</p>
    {/if}
    {#if status.registered && status.mode === 'windows'}
      <p class="px-4 pb-3 text-xs text-muted-foreground">{t('cli.reopen_terminal')}</p>
    {/if}
    {#if status.registered && status.mode === 'symlink' && !status.target_dir_on_path}
      <div class="flex flex-col gap-1.5 px-4 pb-3 text-xs text-muted-foreground">
        <p>{t('cli.not_on_path_hint')}</p>
        <div class="flex items-center gap-2">
          <code class="rounded bg-muted px-1.5 py-0.5 font-mono">{exportLine}</code>
          <Button variant="ghost" size="icon-sm" onclick={() => copy(exportLine)} aria-label={t('dev.copy')}>
            <Copy class="size-3.5" />
          </Button>
        </div>
      </div>
    {/if}
  {:else if status}
    <p class="px-4 py-3 text-xs text-muted-foreground">{t('cli.missing')}</p>
  {/if}
  {#if error}
    <p class="px-4 pb-3 text-xs text-destructive" role="alert">{error}</p>
  {/if}
</SettingGroup>
