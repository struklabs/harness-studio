<script lang="ts">
  // Settings > Runtime > GGUF Conversion Environment: the throwaway Python environment
  // (uv + Python + torch CPU + the converter's packages) Harness builds to convert safetensors
  // repos to GGUF. Built on the first conversion or here; deletable at any time.
  import { invoke } from '$lib/ipc';
  import { onDestroy, onMount } from 'svelte';
  import { toast } from 'svelte-sonner';
  import type { ConvertEnvStatus } from '$lib/types';
  import { t, type TKey } from '$lib/i18n.svelte';
  import { formatBytes } from '$lib/format';
  import { localizeError } from '$lib/network';
  import { Button } from '$lib/components/ui/button';
  import { Badge } from '$lib/components/ui/badge';
  import { Progress } from '$lib/components/ui/progress';
  import SettingGroup from './SettingGroup.svelte';

  const fmt = (b: number) => formatBytes(b, 1);

  let status = $state<ConvertEnvStatus | null>(null);
  let busy = $state<'install' | 'remove' | null>(null);
  let timer: ReturnType<typeof setInterval> | null = null;
  onDestroy(() => timer && clearInterval(timer));

  let progress = $derived(status?.progress ?? null);
  let pct = $derived(progress && progress.total_bytes > 0 ? (progress.downloaded_bytes / progress.total_bytes) * 100 : 0);
  /** A build started elsewhere (a conversion) is shown too. */
  let building = $derived(busy === 'install' || !!progress);

  async function load() {
    try {
      status = await invoke<ConvertEnvStatus>('get_convert_env');
    } catch (e) {
      toast.error(localizeError(e));
    }
  }

  function poll(on: boolean) {
    if (on && !timer) timer = setInterval(load, 600);
    if (!on && timer) {
      clearInterval(timer);
      timer = null;
    }
  }

  onMount(async () => {
    await load();
    poll(!!status?.progress);
  });

  $effect(() => poll(building));

  async function install() {
    busy = 'install';
    try {
      status = await invoke<ConvertEnvStatus>('install_convert_env');
      toast.success(t('cvt.env_installed_ok'));
    } catch (e) {
      toast.error(localizeError(e));
      await load();
    } finally {
      busy = null;
    }
  }

  async function remove() {
    busy = 'remove';
    try {
      await invoke('remove_convert_env');
      toast.success(t('cvt.env_removed'));
      await load();
    } catch (e) {
      toast.error(localizeError(e));
    } finally {
      busy = null;
    }
  }

  async function stop() {
    try {
      await invoke('cancel_convert_env_install');
    } catch {
      /* best effort */
    }
  }

  function stateText(s: ConvertEnvStatus): string {
    if (!s.supported) return t('cvt.env_unsupported');
    if (!s.runtime_tag && !s.installed) return t('cvt.env_no_runtime');
    if (!s.installed || !s.manifest) return t('cvt.env_missing', { size: fmt(s.estimated_size_bytes) });
    if (s.ready) return t('cvt.env_ready', { tag: s.manifest.tag, python: s.manifest.python });
    return t('cvt.env_stale', { tag: s.manifest.tag, runtime: s.runtime_tag ?? '-' });
  }
</script>

{#if status}
  <SettingGroup title={t('cvt.env_title')} hint={t('cvt.env_hint')}>
    <div class="flex flex-col gap-1.5 px-4 py-3">
      <div class="flex min-w-0 items-center justify-between gap-3">
        <div class="flex min-w-0 flex-col gap-0.5">
          <span class="flex min-w-0 items-center gap-1.5 text-sm font-medium">
            <span class="truncate">{stateText(status)}</span>
            {#if status.ready}<Badge variant="secondary" class="text-xs">{status.manifest?.tag}</Badge>{/if}
          </span>
          {#if status.installed}
            <span class="truncate font-mono text-xs text-muted-foreground" title={status.dir}>{t('cvt.env_size', { size: fmt(status.size_bytes), dir: status.dir })}</span>
          {/if}
        </div>
        <div class="flex shrink-0 items-center gap-1.5">
          {#if building}
            <Button variant="outline" size="sm" onclick={stop}>{t('cvt.env_cancel')}</Button>
          {:else}
            {#if status.supported && status.runtime_tag && !status.ready}
              <Button size="sm" disabled={busy !== null} onclick={install}>{t('cvt.env_install')}</Button>
            {/if}
            {#if status.installed}
              <Button variant="ghost" size="sm" class="text-destructive" disabled={busy !== null} onclick={remove}>
                {t('cvt.env_remove')}
              </Button>
            {/if}
          {/if}
        </div>
      </div>
      {#if building}
        <Progress value={pct} aria-label={t('cvt.env_installing')} class={progress && progress.total_bytes === 0 ? 'animate-pulse' : ''} />
        <span class="flex min-w-0 gap-2 font-num text-xs text-muted-foreground">
          <span class="shrink-0">{progress ? t(`cvt.env_step_${progress.step}` as TKey) : t('cvt.env_installing')}</span>
          {#if progress}
            <span class="min-w-0 truncate font-mono" title={progress.detail}>{progress.detail}</span>
          {/if}
        </span>
      {/if}
    </div>
  </SettingGroup>
{/if}
