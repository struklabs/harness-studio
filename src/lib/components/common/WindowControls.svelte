<script lang="ts">
  // Minimize / maximize-restore / close for the frameless window on Windows and Linux.
  // macOS keeps its native traffic lights (tauri.macos.conf.json), so this renders nothing there.
  import { onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { t } from '$lib/i18n.svelte';
  import { cn } from '$lib/utils';

  let maximized = $state(false);

  onMount(() => {
    let unlisten: (() => void) | null = null;
    let destroyed = false;
    try {
      const win = getCurrentWindow();
      const sync = () => win.isMaximized().then((m) => (maximized = m)).catch(() => {});
      void sync();
      win
        .onResized(sync)
        .then((fn) => {
          if (destroyed) fn();
          else unlisten = fn;
        })
        .catch(() => {});
    } catch {
      // Not running inside Tauri (plain browser preview).
    }
    return () => {
      destroyed = true;
      unlisten?.();
    };
  });

  function run(action: 'minimize' | 'toggleMaximize' | 'close') {
    try {
      // `close` goes through CloseRequested, so "close to tray" still applies.
      void getCurrentWindow()[action]().catch((e) => console.error(`[Window] ${action} failed:`, e));
    } catch (e) {
      console.error(`[Window] ${action} unavailable:`, e);
    }
  }

  const base =
    'flex h-full w-[46px] items-center justify-center text-foreground/80 outline-none transition-colors duration-100 focus-visible:ring-2 focus-visible:ring-ring/60 focus-visible:ring-inset';
</script>

<div class="flex h-full shrink-0 items-stretch" role="group" aria-label={t('win.controls')}>
  <button
    type="button"
    class={cn(base, 'hover:bg-foreground/[0.07] active:bg-foreground/[0.12]')}
    aria-label={t('win.minimize')}
    title={t('win.minimize')}
    onclick={() => run('minimize')}
  >
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5.5h10" stroke="currentColor" /></svg>
  </button>
  <button
    type="button"
    class={cn(base, 'hover:bg-foreground/[0.07] active:bg-foreground/[0.12]')}
    aria-label={maximized ? t('win.restore') : t('win.maximize')}
    title={maximized ? t('win.restore') : t('win.maximize')}
    onclick={() => run('toggleMaximize')}
  >
    {#if maximized}
      <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
        <path d="M2.5 2.5V1.2a.7.7 0 0 1 .7-.7h5.6a.7.7 0 0 1 .7.7v5.6a.7.7 0 0 1-.7.7H7.5" stroke="currentColor" />
        <rect x="0.5" y="2.5" width="7" height="7" rx="0.7" stroke="currentColor" />
      </svg>
    {:else}
      <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
        <rect x="0.5" y="0.5" width="9" height="9" rx="0.7" stroke="currentColor" />
      </svg>
    {/if}
  </button>
  <!-- Close uses the platform's own red (`--window-close`, same in every theme; the × shape carries it without colour). -->
  <button
    type="button"
    class={cn(base, 'hover:bg-window-close hover:text-window-close-foreground active:bg-window-close/85 active:text-window-close-foreground/90')}
    aria-label={t('win.close')}
    title={t('win.close')}
    onclick={() => run('close')}
  >
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M.5.5l9 9M9.5.5l-9 9" stroke="currentColor" stroke-linecap="round" /></svg>
  </button>
</div>
