<script lang="ts">
  import { capsStore } from '#lib/stores/caps.svelte.js';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { onDestroy } from 'svelte';
  import type { LocalModel, ServerStatus, AppSettings, RecoveryOption } from '#lib/types.js';
  import { canRecover, serverErrorText } from '#lib/failure.js';
  import RecoveryDialog from './RecoveryDialog.svelte';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { invoke } from '#lib/ipc.js';
  import { toast } from 'svelte-sonner';
  import { app } from '#lib/stores/app.svelte.js';
  import { engineInstall } from '#lib/stores/engineInstall.svelte.js';
  import { t } from '#lib/i18n.svelte.js';
  import { Play, Square, ExternalLink, CircleAlert, Search, LoaderCircle, Bookmark, LifeBuoy, TimerReset, MoreHorizontal } from '@lucide/svelte';
  import { Button, buttonVariants } from '#lib/components/ui/button/index.js';
  import * as DropdownMenu from '#lib/components/ui/dropdown-menu/index.js';
  import { shortWindow } from '#lib/stores/panes.svelte.js';
  import { cn } from '#lib/utils.js';
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import { engineLabel } from '#lib/engines.js';

  interface Props {
    serverStatus: ServerStatus;
    routerMode: boolean;
    selectedModel: LocalModel | undefined;
    /** Number of fully downloaded models (router mode needs at least one). */
    readyModelCount: number;
    actionError: string | null;
    /** Result line of the last "Save as profile" (shown under the buttons). */
    profileMsg?: string | null;
    /** A saved server is selected: the button updates it instead of creating a profile. */
    isProfile?: boolean;
    onStart: () => void;
    onStop: () => void;
    onSaveProfile: () => void;
    /** A recovery option was retried (the form can show the values it used). */
    onRecovered?: (option: RecoveryOption, saved: boolean) => void;
  }

  let { serverStatus, routerMode, selectedModel, readyModelCount, actionError, profileMsg = null, isProfile = false, onStart, onStop, onSaveProfile, onRecovered }: Props =
    $props();

  let failure = $derived(app.selectedServer?.failure ?? null);
  let restart = $derived(app.selectedServer?.restart ?? null);
  let showRecovery = $state(false);
  // Countdown of a scheduled automatic restart.
  let now = $state(Date.now());
  $effect(() => {
    if (!restart) return;
    const timer = setInterval(() => (now = Date.now()), 250);
    return () => clearInterval(timer);
  });
  let restartIn = $derived(restart ? Math.max(0, Math.ceil((restart.restart_at_ms - now) / 1000)) : 0);

  let isAutoConfiguring = $state<boolean>(false);
  let autoConfigMsg = $state<string | null>(null);
  let autoConfigTimer: ReturnType<typeof setTimeout> | undefined;

  onDestroy(() => clearTimeout(autoConfigTimer));

  async function handleAutoConnectSystemRuntime() {
    isAutoConfiguring = true;
    autoConfigMsg = null;
    try {
      const updated: AppSettings = await invoke('auto_configure_system_runtime');
      app.setSettings(updated);
      autoConfigMsg = t('dash.sa_auto_connected');
      clearTimeout(autoConfigTimer);
      autoConfigTimer = setTimeout(() => {
        autoConfigMsg = null;
        onStart();
      }, 700);
    } catch (e: any) {
      autoConfigMsg = t('dash.sa_auto_not_found', { error: String(e) });
    } finally {
      isAutoConfiguring = false;
    }
  }

  async function openWebUi() {
    const url = app.hostUrl;
    try {
      await openUrl(url);
    } catch {
      try {
        window.open(url, '_blank');
      } catch (e) {
        toast.error(t('dash.n_open_failed', { url, error: String(e) }));
      }
    }
  }

  let effectiveError = $derived(
    actionError ||
      (typeof serverStatus === 'object' && 'Error' in serverStatus
        ? (serverErrorText(app.selectedServer) ?? serverStatus.Error)
        : null)
  );
  /** The error on screen is the server's classified failure (not a refused start). */
  let showsFailure = $derived(!actionError && !!failure && typeof serverStatus === 'object');
  let isRuntimeError = $derived(
    !!effectiveError &&
      (showsFailure
        ? failure?.params.detail === 'runtime_missing'
        : /runtime|llama\.cpp|llama-server|kurulu değil|bulunamadı/i.test(effectiveError))
  );

  let cannotStart = $derived(
    (!routerMode && (!selectedModel || !selectedModel.is_ready)) || (routerMode && readyModelCount === 0)
  );
  let startBlockedReason = $derived(
    !cannotStart
      ? ''
      : routerMode
        ? t('dash.err_router_no_ready')
        : !selectedModel
          ? t('dash.err_select_model')
          : t('dash.err_incomplete_model')
  );
  let running = $derived(serverStatus === 'Running');
</script>

<div class="flex flex-col gap-2">
  {#if effectiveError}
    <div class="flex flex-col gap-2 rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-2 text-xs short:max-h-32 short:overflow-y-auto" role="alert">
      <div class="flex items-start gap-2 text-destructive">
        <CircleAlert class="mt-0.5 size-3.5 shrink-0" />
        <span class="min-w-0 break-words">{effectiveError}</span>
      </div>
      {#if showsFailure && failure}
        {#if failure.params.exit}
          <span class="font-num text-xs text-muted-foreground">{t('failure.exit_code', { exit: failure.params.exit })}</span>
        {/if}
        {#if failure.evidence.length}
          <details class="text-xs text-muted-foreground">
            <summary class="cursor-pointer select-none">{t('failure.evidence')}</summary>
            <ul class="mt-1 flex flex-col gap-0.5">
              {#each failure.evidence as line, i (i)}<li class="font-mono break-all">{line}</li>{/each}
            </ul>
          </details>
        {/if}
        {#if canRecover(failure)}
          <Button variant="outline" size="sm" class="self-start" onclick={() => (showRecovery = true)}>
            <LifeBuoy />
            {t('failure.recover')}
          </Button>
        {/if}
      {/if}
      {#if restart}
        <div class="flex flex-wrap items-center gap-2 text-foreground" role="status">
          <TimerReset class="size-3.5 shrink-0" />
          <span class="tabular">{t('restart.pending', { seconds: restartIn, attempt: restart.attempt, max: restart.max_attempts })}</span>
          <Button variant="ghost" size="sm" class="h-6 px-2" onclick={onStop}>{t('restart.cancel')}</Button>
        </div>
      {/if}
      {#if isRuntimeError}
        <Button variant="outline" size="sm" class="self-start" disabled={isAutoConfiguring} onclick={handleAutoConnectSystemRuntime}>
          <Search />
          {isAutoConfiguring ? t('dash.sa_auto_searching') : t('dash.sa_auto_connect')}
        </Button>
      {/if}
      {#if autoConfigMsg}
        <div class="text-muted-foreground" role="status">{autoConfigMsg}</div>
      {/if}
    </div>
  {/if}

  {#if engineInstall.autoFamily}
    {@const family = engineInstall.autoFamily}
    {@const pct = engineInstall.state(family).pct}
    <div class="flex items-center gap-2 rounded-md border border-info/30 bg-info/10 px-2.5 py-2 text-xs text-info" role="status">
      <LoaderCircle class="size-3.5 shrink-0 animate-spin" />
      <span class="min-w-0 break-words">{t(pct === null ? 'eng.auto_installing' : 'eng.auto_installing_pct', { runtime: engineLabel(family), pct: pct ?? 0 })}</span>
    </div>
  {/if}

  <!-- Short window: the secondary actions move into a menu next to Start (always visible). -->
  <div class="flex gap-2">
    <div class="min-w-0 flex-1">
      {#if running}
        <Button variant="destructive" class="h-9 w-full" onclick={onStop}>
          <Square fill="currentColor" />
          {t('dash.stop_server')}
        </Button>
      {:else if serverStatus === 'Starting'}
        <div class="flex gap-2">
          <Button class="h-9 flex-1" disabled>
            <LoaderCircle class="animate-spin" />
            {t('dash.sa_starting')}
          </Button>
          <Button variant="outline" class="h-9" onclick={onStop}>{t('dash.sa_cancel')}</Button>
        </div>
      {:else if serverStatus === 'Stopping'}
        <Button class="h-9 w-full" disabled>
          <LoaderCircle class="animate-spin" />
          {t('dash.sa_stopping')}
        </Button>
      {:else if cannotStart}
        <Tooltip.Root>
          <Tooltip.Trigger>
            {#snippet child({ props })}
              <!-- A disabled button gets no pointer events: the focusable wrapper explains why. -->
              <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
              <span {...props} role="note" tabindex="0" class="block rounded-lg focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none">
                <Button class="pointer-events-none h-9 w-full" disabled tabindex={-1}>
                  <Play fill="currentColor" />
                  {t('dash.start_server')}
                </Button>
              </span>
            {/snippet}
          </Tooltip.Trigger>
          <Tooltip.Content>{startBlockedReason}</Tooltip.Content>
        </Tooltip.Root>
      {:else}
        <Button class="h-9 w-full" onclick={onStart}>
          <Play fill="currentColor" />
          {t('dash.start_server')}
        </Button>
      {/if}
    </div>
    {#if uiLevel.advanced && shortWindow.current}
      <DropdownMenu.Root>
        <DropdownMenu.Trigger class={cn(buttonVariants({ variant: 'outline', size: 'icon' }), 'size-9 shrink-0')} title={t('srv.more_actions')}>
          <MoreHorizontal />
          <span class="sr-only">{t('srv.more_actions')}</span>
        </DropdownMenu.Trigger>
        <DropdownMenu.Content align="end" class="w-56">
          {#if !capsStore.otherEngine}
            <DropdownMenu.Item disabled={!running} onclick={openWebUi}>
              <ExternalLink />
              {t('dash.open_webui')}
            </DropdownMenu.Item>
          {/if}
          <DropdownMenu.Item onclick={onSaveProfile}>
            <Bookmark />
            {isProfile ? t('srv.save_changes_btn') : t('srv.save_profile_btn')}
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Root>
    {/if}
  </div>

  {#if uiLevel.advanced && !shortWindow.current}
    <div class="flex flex-wrap items-center gap-2">
      <!-- llama-server's own web UI: other engines (ONNX, whisper.cpp, sd.cpp...) have none. -->
      {#if !capsStore.otherEngine}
        <Button variant="outline" size="sm" class="flex-1" disabled={!running} onclick={openWebUi}>
          <ExternalLink />
          {t('dash.open_webui')}
        </Button>
      {/if}
      <Button variant="ghost" size="sm" onclick={onSaveProfile}>
        <Bookmark />
        {isProfile ? t('srv.save_changes_btn') : t('srv.save_profile_btn')}
      </Button>
    </div>
  {/if}

  {#if profileMsg}
    <p class="text-xs text-success" role="status">{profileMsg}</p>
  {/if}
</div>

{#if showRecovery}
  <RecoveryDialog serverId={app.selectedServerId} onRetried={onRecovered} onClose={() => (showRecovery = false)} />
{/if}
