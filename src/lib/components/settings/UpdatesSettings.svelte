<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { errorCode, errorText } from '#lib/errors.js';
  import { getVersion } from '@tauri-apps/api/app';
  import { Check, TriangleAlert } from '@lucide/svelte';
  import type { AppUpdateInfo, AppUpdateProgress } from '#lib/types.js';
  import { formatBytes } from '#lib/format.js';
  import { t } from '#lib/i18n.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Progress } from '#lib/components/ui/progress/index.js';
  import * as Collapsible from '#lib/components/ui/collapsible/index.js';
  import SettingGroup from './SettingGroup.svelte';
  import SettingRow from './SettingRow.svelte';

  const fmt = (b: number) => formatBytes(b, 1);

  let appVersion = $state<string | null>(null);
  let checking = $state(false);
  /** undefined = not checked yet, null = up to date. */
  let appUpdate = $state<AppUpdateInfo | null | undefined>(undefined);
  /** Informational (not an error), e.g. the update endpoint is not configured yet. */
  let infoMsg = $state<string | null>(null);
  let error = $state<string | null>(null);
  let confirmInstall = $state(false);
  let installing = $state(false);
  let progress = $state<AppUpdateProgress | null>(null);
  let progressTimer: ReturnType<typeof setInterval> | null = null;
  let destroyed = false;

  const progressPct = $derived(
    progress && progress.total ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100)) : null,
  );

  function stopPolling() {
    if (progressTimer) clearInterval(progressTimer);
    progressTimer = null;
  }

  async function checkUpdate() {
    if (checking) return;
    checking = true;
    appUpdate = undefined;
    infoMsg = null;
    error = null;
    confirmInstall = false;
    try {
      appUpdate = await invoke<AppUpdateInfo | null>('check_app_update');
    } catch (e) {
      // Not configured, or no package for this system in the release: a note, not a failure.
      // The backend's text already says what failed.
      const code = errorCode(e);
      if (code === 'update.not_configured' || code === 'update.no_platform') infoMsg = errorText(e);
      else error = errorText(e);
    } finally {
      checking = false;
    }
  }

  async function installUpdate() {
    if (installing) return;
    confirmInstall = false;
    installing = true;
    error = null;
    progress = null;
    stopPolling();
    progressTimer = setInterval(async () => {
      try {
        const p = await invoke<AppUpdateProgress | null>('get_app_update_progress');
        if (!destroyed) progress = p;
      } catch {
        /* progress is best-effort */
      }
    }, 500);
    try {
      // On success the backend restarts the app, so this normally never resolves.
      await invoke('install_app_update');
    } catch (e) {
      error = errorText(e);
      stopPolling();
      progress = null;
      installing = false;
    }
  }

  onMount(() => {
    getVersion()
      .then((v) => {
        if (!destroyed) appVersion = v;
      })
      .catch(() => {
        if (!destroyed) appVersion = null;
      });
  });

  onDestroy(() => {
    destroyed = true;
    stopPolling();
  });
</script>

<div class="flex flex-col gap-6">
  <SettingGroup title={t('settings.app_updates_title')}>
    <SettingRow label={t('settings.app_current_version', { version: appVersion ?? '—' })} hint={t('settings.app_updates_hint')}>
      {#snippet extra()}
        <div role="status" aria-live="polite" class="flex flex-col gap-2">
          {#if infoMsg}
            <p class="text-xs break-words text-muted-foreground">{infoMsg}</p>
          {:else if appUpdate === null}
            <p class="flex items-center gap-1.5 text-xs text-success"><Check class="size-3.5" /> {t('settings.app_up_to_date')}</p>
          {:else if appUpdate}
            <div class="flex flex-col gap-2 rounded-md border border-primary/30 bg-primary/5 p-3">
              <div class="flex flex-wrap items-baseline gap-x-3 gap-y-0.5">
                <strong class="text-sm">{t('settings.app_update_available', { version: appUpdate.version })}</strong>
                {#if appUpdate.date}
                  <span class="text-xs text-muted-foreground">{t('settings.app_update_date', { date: appUpdate.date })}</span>
                {/if}
              </div>
              {#if appUpdate.notes}
                <Collapsible.Root open>
                  <Collapsible.Trigger class="text-xs text-muted-foreground underline-offset-2 hover:text-foreground hover:underline">
                    {t('settings.app_release_notes')}
                  </Collapsible.Trigger>
                  <Collapsible.Content>
                    <pre class="selectable mt-1.5 max-h-40 overflow-auto font-sans text-xs whitespace-pre-wrap text-muted-foreground">{appUpdate.notes}</pre>
                  </Collapsible.Content>
                </Collapsible.Root>
              {/if}
            </div>
          {/if}

          {#if installing}
            <div class="flex flex-col gap-1.5">
              <Progress
                value={progress?.finished ? 100 : (progressPct ?? 0)}
                aria-label={t('settings.app_install_progress_label')}
              />
              <span class="font-num text-xs text-muted-foreground">
                {progress?.finished ? t('settings.app_install_finished') : t('settings.app_installing')}
                {#if progress}
                  {fmt(progress.downloaded)}{progress.total ? ` / ${fmt(progress.total)}` : ''}
                  {#if progressPct !== null}({progressPct}%){/if}
                {/if}
              </span>
            </div>
          {/if}

          {#if error}<p class="text-xs break-words text-destructive" role="alert">{error}</p>{/if}

          {#if appUpdate && !installing && confirmInstall}
            <p class="flex items-start gap-1.5 text-xs text-warning">
              <TriangleAlert class="mt-0.5 size-3.5 shrink-0" /> <span>{t('settings.app_install_confirm')}</span>
            </p>
          {/if}
        </div>
      {/snippet}
      <Button variant="outline" size="sm" onclick={checkUpdate} disabled={checking || installing}>
        {checking ? t('settings.app_checking') : t('settings.app_check_updates')}
      </Button>
      {#if appUpdate && !installing}
        {#if confirmInstall}
          <Button size="sm" onclick={installUpdate}>{t('settings.app_install_confirm_yes')}</Button>
          <Button variant="ghost" size="sm" onclick={() => (confirmInstall = false)}>{t('settings.cancel')}</Button>
        {:else}
          <Button size="sm" onclick={() => (confirmInstall = true)}>{t('settings.app_install_update')}</Button>
        {/if}
      {/if}
    </SettingRow>
  </SettingGroup>
</div>
