<script lang="ts">
  // Quick-action icon buttons in the title bar (left of the window controls). Sidebar, Downloads,
  // Server status, Logs and the models folder; the user can hide each one in Settings > Appearance.
  // Narrow windows move the last buttons into the overflow menu. Buttons sit outside the drag region.
  import { PanelLeft, Download, ScrollText, FolderOpen, EllipsisVertical, Settings, Search } from '@lucide/svelte';
  import { revealItemInDir } from '@tauri-apps/plugin-opener';
  import { toast } from 'svelte-sonner';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { downloads } from '#lib/stores/downloads.svelte.js';
  import { isPending } from '#lib/components/hub/downloadQueue.svelte.js';
  import { panes, viewportWidth } from '#lib/stores/panes.svelte.js';
  import { toolbarPrefs } from '#lib/stores/toolbar.svelte.js';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { shownItems, splitItems, type ToolbarId } from '#lib/toolbarLayout.js';
  import * as DropdownMenu from '#lib/components/ui/dropdown-menu/index.js';
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import DownloadQueuePanel from '#lib/components/hub/DownloadQueuePanel.svelte';
  import { cn } from '#lib/utils.js';

  type Kind = 'running' | 'starting' | 'stopping' | 'stopped' | 'error';
  interface Props {
    serverKind: Kind;
    onOpenCommand?: () => void;
  }

  let { serverKind, onOpenCommand }: Props = $props();
  let downloadsOpen = $state(false);

  const isMac = typeof navigator !== 'undefined' && /Mac/i.test(navigator.userAgent + navigator.platform);
  const mod = isMac ? '⌘' : 'Ctrl';

  let pending = $derived(downloads.jobs.filter((j) => isPending(j.status)).length);
  let layout = $derived(splitItems(shownItems(toolbarPrefs.hidden, uiLevel.advanced), viewportWidth()));

  const labelKeys: Record<ToolbarId, TKey> = {
    sidebar: 'toolbar.sidebar',
    downloads: 'toolbar.downloads',
    server: 'toolbar.server',
    logs: 'toolbar.logs',
    folder: 'toolbar.folder',
  };
  const icons = { sidebar: PanelLeft, downloads: Download, server: null, logs: ScrollText, folder: FolderOpen } as const;

  const serverKeys: Record<Kind, TKey> = {
    running: 'toolbar.server_running',
    starting: 'status.starting',
    stopping: 'status.stopping',
    stopped: 'toolbar.server_stopped',
    error: 'status.error',
  };

  function label(id: ToolbarId): string {
    if (id === 'server') return `${t('toolbar.server')}: ${t(serverKeys[serverKind])}`;
    if (id === 'downloads' && pending > 0) return `${t('toolbar.downloads')} (${pending})`;
    if (id === 'sidebar') return `${t('toolbar.sidebar')} (${mod}+B)`;
    return t(labelKeys[id]);
  }

  async function revealModels() {
    const dir = app.settings?.models_dir;
    if (!dir) return toast.error(t('toolbar.folder_missing'));
    try {
      await revealItemInDir(dir);
    } catch (e) {
      toast.error(t('toolbar.folder_failed', { error: String(e) }));
    }
  }

  function run(id: ToolbarId) {
    if (id === 'sidebar') panes.nav.toggle();
    else if (id === 'downloads') downloadsOpen = true;
    else if (id === 'server') app.openTab('dashboard');
    else if (id === 'logs') {
      app.openTab('dashboard');
      app.logsRequest++;
    } else void revealModels();
  }

  const btn =
    'relative flex size-7 items-center justify-center rounded-md text-muted-foreground outline-none transition-colors hover:bg-foreground/[0.07] hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/60 active:bg-foreground/[0.12] data-[state=open]:bg-foreground/[0.07] data-[state=open]:text-foreground';
  const dotClass: Record<Kind, string> = {
    running: 'bg-success',
    starting: 'border-[1.5px] border-warning',
    stopping: 'border-[1.5px] border-warning',
    stopped: 'bg-muted-foreground/50',
    error: 'bg-destructive',
  };
</script>

{#snippet icon(id: ToolbarId)}
  {@const Icon = icons[id]}
  {#if Icon}
    <Icon class="size-4" strokeWidth={1.8} />
  {:else}
    <span class="flex size-4 items-center justify-center" aria-hidden="true"><span class={cn('size-2.5 rounded-full', dotClass[serverKind])}></span></span>
  {/if}
{/snippet}

<div class="flex items-center gap-0.5 px-1" role="group" aria-label={t('toolbar.group')}>
  {#each layout.inline as id (id)}
    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}
          <button
            {...props}
            type="button"
            class={btn}
            aria-label={label(id)}
            aria-expanded={id === 'sidebar' ? panes.nav.expanded : undefined}
            aria-controls={id === 'sidebar' ? panes.nav.elementId : undefined}
            onclick={() => run(id)}
          >
            {@render icon(id)}
            {#if id === 'downloads' && pending > 0}
              <span class="absolute -top-0.5 -right-0.5 flex h-3.5 min-w-3.5 items-center justify-center rounded-full bg-primary px-1 font-num text-[10px] leading-none text-primary-foreground" aria-hidden="true">{pending}</span>
            {/if}
          </button>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content side="bottom">{label(id)}</Tooltip.Content>
    </Tooltip.Root>
  {/each}

  <DropdownMenu.Root>
    <DropdownMenu.Trigger class={btn} aria-label={t('toolbar.more')} title={t('toolbar.more')}>
      <EllipsisVertical class="size-4" strokeWidth={1.8} />
      {#if layout.overflow.includes('downloads') && pending > 0}
        <span class="absolute top-0.5 right-0.5 size-2 rounded-full bg-primary" aria-hidden="true"></span>
      {/if}
    </DropdownMenu.Trigger>
    <DropdownMenu.Content align="end" class="min-w-48">
      {#each layout.overflow as id (id)}
        <DropdownMenu.Item onclick={() => run(id)}>
          {@render icon(id)}
          {label(id)}
        </DropdownMenu.Item>
      {/each}
      {#if layout.overflow.length > 0}<DropdownMenu.Separator />{/if}
      <DropdownMenu.Item onclick={onOpenCommand}><Search />{t('cmd.open')}<DropdownMenu.Shortcut>{mod}+K</DropdownMenu.Shortcut></DropdownMenu.Item>
      <DropdownMenu.Item onclick={() => app.openTab('settings')}><Settings />{t('nav.settings')}</DropdownMenu.Item>
    </DropdownMenu.Content>
  </DropdownMenu.Root>
</div>

<Dialog.Root bind:open={downloadsOpen}>
  <Dialog.Content class="max-h-[80vh] overflow-y-auto sm:max-w-xl">
    <Dialog.Header>
      <Dialog.Title>{t('toolbar.downloads')}</Dialog.Title>
    </Dialog.Header>
    <DownloadQueuePanel queue={downloads} showEmpty />
  </Dialog.Content>
</Dialog.Root>
