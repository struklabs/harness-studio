<script lang="ts">
  import { ChevronsUpDown, Plus } from '@lucide/svelte';
  import { app, DEFAULT_SERVER_ID } from '#lib/stores/app.svelte.js';
  import { t } from '#lib/i18n.svelte.js';
  import { cn } from '#lib/utils.js';
  import { buttonVariants } from '#lib/components/ui/button/index.js';
  import * as DropdownMenu from '#lib/components/ui/dropdown-menu/index.js';
  import { savedPortText, statusKind, type StatusKind } from '../servers/utils';

  const dot: Record<StatusKind, string> = {
    running: 'bg-success',
    starting: 'bg-warning animate-pulse',
    stopping: 'bg-warning animate-pulse',
    stopped: 'bg-muted-foreground/50',
    error: 'bg-destructive',
  };

  /** Every instance plus saved profiles that have not been started yet. */
  let entries = $derived([
    ...app.servers.map((s) => ({ id: s.id, kind: statusKind(s.status), port: s.active?.port ?? null })),
    ...app.profiles
      .filter((p) => !app.servers.some((s) => s.id === p.id))
      .map((p) => ({ id: p.id, kind: 'stopped' as StatusKind, port: null })),
  ]);

  interface Props {
    /** Title bar use: the trigger carries the selected server's status dot and says it is global. */
    global?: boolean;
  }
  let { global = false }: Props = $props();

  let selectedKind = $derived<StatusKind>(statusKind(app.serverStatus));

  /** Trigger: only the live port. A stopped server's next port is whatever the Dashboard form
   *  holds (saved or not), so showing the saved one there could contradict the form. */
  let triggerPort = $derived(app.activeServer?.port ? `:${app.activeServer.port}` : '');
  let triggerLabel = $derived(app.serverLabel(app.selectedServerId));

  /** `:port` while running, else the saved port ("Automatic" when Harness picks it at start). */
  function portOf(id: string, live: number | null): string {
    if (live) return `:${live}`;
    const cfg = id === DEFAULT_SERVER_ID ? app.defaultServer?.config : app.profiles.find((p) => p.id === id)?.config;
    return cfg ? savedPortText(cfg, t('dash.port_auto')) : ':—';
  }
</script>

<DropdownMenu.Root>
  <DropdownMenu.Trigger
    class={cn(buttonVariants({ variant: 'outline', size: 'sm' }), 'h-7 max-w-56 gap-1.5 px-2 text-xs', global && 'max-w-72 shrink-0')}
    title={`${triggerLabel}${triggerPort} — ${global ? t('shell.global_server_tip') : t('srv.switcher_title')}`}
    aria-label={global ? t('shell.global_server') : t('srv.switcher_label')}
  >
    {#if global}<span class={cn('size-2 shrink-0 rounded-full', dot[selectedKind])} aria-hidden="true"></span>{/if}
    <span class="truncate font-medium">{triggerLabel}</span>
    {#if triggerPort}<span class="shrink-0 font-num text-muted-foreground">{triggerPort}</span>{/if}
    <ChevronsUpDown class="size-3 shrink-0 text-muted-foreground" />
  </DropdownMenu.Trigger>
  <DropdownMenu.Content align="end" class="w-64">
    <DropdownMenu.Label class="text-xs text-muted-foreground">{t('srv.switcher_label')}</DropdownMenu.Label>
    {#each entries as e (e.id)}
      {@const desc = app.serverDescription(e.id)}
      <DropdownMenu.Item onclick={() => app.selectServer(e.id)} class={e.id === app.selectedServerId ? 'bg-accent' : ''}>
        <span class={cn('size-2 shrink-0 rounded-full', dot[e.kind])} aria-hidden="true"></span>
        <span class="flex min-w-0 flex-1 flex-col leading-tight">
          <span class="truncate text-xs">{e.id === DEFAULT_SERVER_ID ? t('srv.default_label') : app.serverLabel(e.id)}</span>
          {#if desc}<span class="truncate text-xs text-muted-foreground">{desc}</span>{/if}
        </span>
        <span class="shrink-0 font-num text-xs text-muted-foreground">{portOf(e.id, e.port)}</span>
      </DropdownMenu.Item>
    {/each}
    <DropdownMenu.Separator />
    <DropdownMenu.Item onclick={() => (app.newServerPending = true)}>
      <Plus class="size-3.5" />
      <span class="text-xs">{t('srv.new_server')}</span>
    </DropdownMenu.Item>
  </DropdownMenu.Content>
</DropdownMenu.Root>
