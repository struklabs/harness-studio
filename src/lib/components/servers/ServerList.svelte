<script lang="ts">
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  // One list of servers: every saved server (profile) plus any instance that runs without being
  // saved (the default server, ad-hoc launches). Each row shows its run state and whether it is
  // saved, so "instance" vs "profile" never has to be explained.
  import { invoke } from '#lib/ipc.js';
  import type { LaunchConfig, ServerProfile, ServerSummary } from '#lib/types.js';
  import { app, DEFAULT_SERVER_ID } from '#lib/stores/app.svelte.js';
  import { t } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { serverErrorText } from '#lib/failure.js';
  import StatusBadge from './StatusBadge.svelte';
  import RouterModels from './RouterModels.svelte';
  import { fileName, isAutoPort, isLive, makeProfileId, statusError } from './utils';
  import { AlertCircle, Copy, LayoutDashboard, MessageSquare, MoreHorizontal, Pencil, Play, RotateCcw, Square, Trash2 } from '@lucide/svelte';
  import * as Table from '#lib/components/ui/table/index.js';
  import { Button, buttonVariants } from '#lib/components/ui/button/index.js';
  import { Switch } from '#lib/components/ui/switch/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import * as DropdownMenu from '#lib/components/ui/dropdown-menu/index.js';
  import { Empty, EmptyTitle, EmptyDescription } from '#lib/components/ui/empty/index.js';
  import { toast } from 'svelte-sonner';
  import { cn } from '#lib/utils.js';

  interface Props {
    onOpenDashboard: (id: string) => void;
    onOpenPlayground: (id: string) => void;
  }

  let { onOpenDashboard, onOpenPlayground }: Props = $props();

  interface Row {
    id: string;
    profile: ServerProfile | null;
    summary: ServerSummary | null;
    /** Saved launch of the Dashboard's own server (the "default" row only). */
    session: LaunchConfig | null;
  }

  // The Dashboard's own server is saved separately from the profiles (its last launch; see
  // `app.defaultServer`, re-read whenever that server's state changes).
  let defaultSession = $derived<LaunchConfig | null>(app.defaultServer?.saved ? app.defaultServer.config : null);

  // Saved servers first (in their saved order), then running/stopped instances nobody saved.
  let rows = $derived.by<Row[]>(() => {
    const list: Row[] = app.profiles.map((p) => ({
      id: p.id,
      profile: p,
      summary: app.servers.find((s) => s.id === p.id) ?? null,
      session: null,
    }));
    for (const s of app.servers) {
      if (!app.profiles.some((p) => p.id === s.id)) {
        list.push({ id: s.id, profile: null, summary: s, session: s.id === DEFAULT_SERVER_ID ? defaultSession : null });
      }
    }
    if (defaultSession && !list.some((r) => r.id === DEFAULT_SERVER_ID)) {
      list.push({ id: DEFAULT_SERVER_ID, profile: null, summary: null, session: defaultSession });
    }
    return list;
  });

  let errors = $state<Record<string, string>>({});
  let busy = $state<Record<string, boolean>>({});
  let renamingId = $state<string | null>(null);
  let renameValue = $state('');
  let profileToDelete = $state<ServerProfile | null>(null);

  async function run(id: string, action: () => Promise<unknown>): Promise<boolean> {
    busy[id] = true;
    delete errors[id];
    try {
      await action();
      return true;
    } catch (e) {
      const msg = e ? localizeError(e) : t('srv.error_generic');
      errors[id] = msg;
      toast.error(msg);
      return false;
    } finally {
      busy[id] = false;
    }
  }

  function plain(p: ServerProfile): ServerProfile {
    return $state.snapshot(p) as ServerProfile;
  }

  async function save(profile: ServerProfile) {
    app.setProfiles(await invoke<ServerProfile[]>('save_server_profile', { profile }));
  }

  const withRefresh = (action: () => Promise<unknown>) => async () => {
    try {
      await action();
    } finally {
      await app.refreshServer();
    }
  };

  const start = (id: string) => run(id, withRefresh(() => invoke('start_server_profile', { profileId: id })));
  const stop = (id: string) => run(id, withRefresh(() => invoke('stop_server', { serverId: id })));
  const remove = (id: string) =>
    run(
      id,
      withRefresh(async () => {
        await invoke('remove_server', { serverId: id });
        if (app.selectedServerId === id) app.selectServer(DEFAULT_SERVER_ID);
      })
    );

  async function toggleAutoStart(p: ServerProfile, value: boolean) {
    const prev = p.auto_start;
    p.auto_start = value;
    const ok = await run(p.id, () => save({ ...plain(p), auto_start: value }));
    if (!ok) p.auto_start = prev;
  }

  function beginRename(p: ServerProfile) {
    renamingId = p.id;
    renameValue = app.serverLabel(p.id);
  }

  async function commitRename(p: ServerProfile, e: SubmitEvent) {
    e.preventDefault();
    const name = renameValue.trim();
    if (!name) {
      errors[p.id] = t('srv.name_required');
      toast.error(t('srv.name_required'));
      return;
    }
    if (await run(p.id, () => save({ ...plain(p), name }))) renamingId = null;
  }

  const duplicate = (p: ServerProfile) =>
    run(p.id, () => {
      const copy = plain(p);
      const name = t('srv.copy_suffix', { name: app.serverLabel(p.id) });
      return save({
        ...copy,
        id: makeProfileId(name, [...app.profiles.map((x) => x.id), ...app.servers.map((s) => s.id)]),
        name,
        auto_start: false,
        // The copy never shares the original's port: Harness picks one at its start.
        config: { ...copy.config, port: 0, port_auto: true },
      });
    });

  async function confirmDelete() {
    if (!profileToDelete) return;
    const p = profileToDelete;
    const ok = await run(p.id, async () => {
      app.setProfiles(await invoke<ServerProfile[]>('delete_server_profile', { profileId: p.id }));
      // The stopped instance would otherwise linger in the list under its raw id.
      await invoke('remove_server', { serverId: p.id }).catch(() => {});
      if (app.selectedServerId === p.id) app.selectServer(DEFAULT_SERVER_ID);
      await app.refreshServer();
    });
    if (ok) profileToDelete = null;
  }

  function stoppedPort(cfg: { port: number; port_auto?: boolean } | null): string | number | null {
    if (!cfg) return null;
    return isAutoPort(cfg) ? t('dash.port_auto') : cfg.port;
  }

  function describe(r: Row) {
    const a = r.summary?.active ?? null;
    const cfg = r.profile?.config ?? r.session;
    const router = a ? a.router_mode : !!cfg?.router_mode;
    const path = a?.model_path ?? cfg?.model_path ?? null;
    return {
      model: path ? (router ? t('srv.router_dir', { dir: fileName(path) }) : fileName(path)) : '',
      modelPath: path ?? '',
      address: a ? `${a.connect_host}:${a.port}` : cfg && !isAutoPort(cfg) ? `${cfg.host}:${cfg.port}` : (cfg?.host ?? ''),
      // An unsaved Dashboard server listens on the settings default: the same port the title bar shows.
      // A stopped server with an automatic port has none until it starts.
      port: a?.port ?? stoppedPort(cfg ?? (r.id === DEFAULT_SERVER_ID ? (app.defaultServer?.config ?? null) : null)),
      hasKey: a ? a.api_key_set : !!cfg?.api_key_ref,
      params: cfg ? t('srv.ngl_ctx', { ngl: cfg.gpu_layers, ctx: cfg.context_size }) : '',
    };
  }
</script>

<section class="flex flex-col gap-2" aria-labelledby="srv-list-title">
  <div class="flex items-baseline justify-between gap-3">
    <h2 id="srv-list-title" class="text-sm font-semibold text-foreground">{t('ui.srv_list_title')}</h2>
    {#if app.runningCount > 0}
      <span class="text-xs text-muted-foreground">{t('srv.running_count', { count: app.runningCount })}</span>
    {/if}
  </div>

  <div class="overflow-hidden rounded-lg border border-border bg-card">
    {#if rows.length === 0}
      <div class="p-8">
        <Empty>
          <EmptyTitle>{t('ui.srv_empty_title')}</EmptyTitle>
          <EmptyDescription>{t('ui.srv_empty_desc')}</EmptyDescription>
        </Empty>
      </div>
    {:else}
      <Table.Root class="w-full table-fixed">
        <Table.Header>
          <Table.Row class="border-b border-border hover:bg-transparent">
            <Table.Head class="min-w-0 pl-4">{t('ui.srv_col_server')}</Table.Head>
            <Table.Head class="w-[84px] text-right">{t('srv.col_port')}</Table.Head>
            <Table.Head class="w-[120px]">{t('srv.col_status')}</Table.Head>
            <Table.Head class="w-[124px] text-center" title={t('srv.auto_start_note')}>{t('ui.srv_col_autostart')}</Table.Head>
            <Table.Head class="w-[88px] pr-4 text-right"><span class="sr-only">{t('srv.col_actions')}</span></Table.Head>
          </Table.Row>
        </Table.Header>
        <Table.Body>
          {#each rows as r (r.id)}
            {@const info = describe(r)}
            {@const live = isLive(r.summary?.status)}
            {@const isSelected = app.selectedServerId === r.id}
            {@const failure = r.summary && statusError(r.summary.status) ? serverErrorText(r.summary) : null}
            <Table.Row class={isSelected ? 'bg-accent/60 hover:bg-accent' : ''} aria-current={isSelected ? 'true' : undefined}>
              <Table.Cell class="min-w-0 py-2.5 pl-4 align-top">
                {#if r.profile && renamingId === r.id}
                  {@const p = r.profile}
                  <form class="flex items-center gap-1.5" onsubmit={(e) => commitRename(p, e)}>
                    <label class="sr-only" for="rename-{p.id}">{t('srv.rename_label')}</label>
                    <FieldHelp text={t('help.server_rename')}><Input id="rename-{p.id}" type="text" maxlength={80} bind:value={renameValue} class="h-7 min-w-0 flex-1 text-sm" /></FieldHelp>
                    <Button type="submit" size="xs" disabled={busy[p.id]}>{t('srv.save')}</Button>
                    <Button type="button" variant="ghost" size="xs" onclick={() => (renamingId = null)}>{t('srv.cancel')}</Button>
                  </form>
                {:else}
                  <div class="flex min-w-0 flex-col gap-0.5">
                    <div class="flex min-w-0 flex-wrap items-baseline gap-x-2">
                      <span class="max-w-full truncate text-sm font-medium text-foreground">{app.serverLabel(r.id)}</span>
                      {#if !r.profile && !r.session}
                        <span class="shrink-0 text-xs text-muted-foreground" title={r.id === DEFAULT_SERVER_ID ? t('srv.default_unsaved_tip') : t('ui.srv_unsaved_tip')}>{t('ui.srv_unsaved')}</span>
                      {/if}
                    </div>
                    {#if r.profile?.description}
                      <span class="truncate text-xs text-muted-foreground" title={r.profile.description}>{r.profile.description}</span>
                    {/if}
                    {#if info.model}
                      <span class="truncate font-mono text-xs text-muted-foreground" title={info.modelPath}>
                        {info.params ? `${info.model} · ${info.params}` : info.model}
                      </span>
                    {/if}
                    {#if failure}
                      <span class="flex items-center gap-1 text-xs text-destructive" role="alert">
                        <AlertCircle class="size-3 shrink-0" />
                        <span class="truncate" title={failure}>{failure}</span>
                      </span>
                    {/if}
                    {#if r.summary?.restart}
                      <span class="text-xs text-muted-foreground" role="status">
                        {t('restart.pending', { seconds: r.summary.restart.delay_secs, attempt: r.summary.restart.attempt, max: r.summary.restart.max_attempts })}
                      </span>
                    {/if}
                    {#if errors[r.id]}
                      <span class="flex items-center gap-1 text-xs text-destructive" role="alert">
                        <AlertCircle class="size-3 shrink-0" />
                        <span class="truncate">{errors[r.id]}</span>
                      </span>
                    {/if}
                  </div>
                {/if}
              </Table.Cell>

              <Table.Cell class="py-2.5 text-right align-top font-num text-xs text-muted-foreground" title={[info.address, info.hasKey ? t('srv.api_key_set') : t('srv.api_key_none')].filter(Boolean).join(' · ')}>
                {info.port ?? '—'}
              </Table.Cell>

              <Table.Cell class="py-2.5 align-top">
                <StatusBadge status={r.summary?.status} />
              </Table.Cell>

              <Table.Cell class="py-2.5 text-center align-top">
                {#if r.profile}
                  {@const p = r.profile}
                  <FieldHelp text={t('help.auto_start')}><Switch
                    bind:checked={p.auto_start}
                    onCheckedChange={(val) => toggleAutoStart(p, val)}
                    disabled={busy[p.id]}
                    aria-label={t('srv.auto_start')}
                    class="mt-0.5"
                  /></FieldHelp>
                {:else}
                  <span class="text-xs text-muted-foreground" aria-hidden="true">—</span>
                {/if}
              </Table.Cell>

              <Table.Cell class="py-2 pr-4 text-right align-top">
                <div class="flex items-center justify-end gap-1">
                  {#if live}
                    <Button
                      variant="outline"
                      size="icon-sm"
                      title={t('srv.stop')}
                      aria-label={t('srv.stop')}
                      onclick={() => stop(r.id)}
                      disabled={busy[r.id] || r.summary?.status === 'Stopping'}
                    >
                      <Square class="size-3.5" />
                    </Button>
                  {:else if r.profile}
                    <Button
                      variant="outline"
                      size="icon-sm"
                      title={t('srv.start')}
                      aria-label={t('srv.start')}
                      onclick={() => start(r.id)}
                      disabled={busy[r.id]}
                    >
                      <Play class="size-3.5" />
                    </Button>
                  {:else}
                    <span class="size-7" aria-hidden="true"></span>
                  {/if}

                  <DropdownMenu.Root>
                    <DropdownMenu.Trigger class={cn(buttonVariants({ variant: 'ghost', size: 'icon-sm' }))}>
                      <MoreHorizontal class="size-4" />
                      <span class="sr-only">{t('srv.more_actions')}</span>
                    </DropdownMenu.Trigger>
                    <DropdownMenu.Content align="end" class="w-48">
                      <DropdownMenu.Item onclick={() => onOpenDashboard(r.id)}>
                        <LayoutDashboard class="size-4" />
                        {r.profile || r.session ? t('srv.open_dashboard') : t('ui.srv_save_in_dashboard')}
                      </DropdownMenu.Item>
                      <DropdownMenu.Item onclick={() => onOpenPlayground(r.id)}>
                        <MessageSquare class="size-4" />
                        {t('srv.open_playground')}
                      </DropdownMenu.Item>
                      {#if live && r.profile}
                        <DropdownMenu.Item onclick={() => start(r.id)}>
                          <RotateCcw class="size-4" />
                          {t('srv.restart')}
                        </DropdownMenu.Item>
                      {/if}
                      {#if r.profile}
                        {@const p = r.profile}
                        <DropdownMenu.Separator />
                        <DropdownMenu.Item onclick={() => beginRename(p)} disabled={renamingId === p.id}>
                          <Pencil class="size-4" />
                          {t('srv.rename')}
                        </DropdownMenu.Item>
                        <DropdownMenu.Item onclick={() => duplicate(p)} disabled={busy[p.id]}>
                          <Copy class="size-4" />
                          {t('srv.duplicate')}
                        </DropdownMenu.Item>
                        <DropdownMenu.Separator />
                        <DropdownMenu.Item
                          variant="destructive"
                          onclick={() => {
                            renamingId = null;
                            profileToDelete = p;
                          }}
                          disabled={live || busy[p.id]}
                          title={live ? t('srv.delete_running_title') : undefined}
                        >
                          <Trash2 class="size-4" />
                          {t('srv.delete')}
                        </DropdownMenu.Item>
                      {:else if !live && r.id !== DEFAULT_SERVER_ID}
                        <DropdownMenu.Separator />
                        <DropdownMenu.Item variant="destructive" onclick={() => remove(r.id)} title={t('srv.remove_title')}>
                          <Trash2 class="size-4" />
                          {t('srv.remove')}
                        </DropdownMenu.Item>
                      {/if}
                    </DropdownMenu.Content>
                  </DropdownMenu.Root>
                </div>
              </Table.Cell>
            </Table.Row>
            {#if r.summary?.status === 'Running' && r.summary.active?.router_mode}
              <Table.Row class="border-t-0 hover:bg-transparent">
                <Table.Cell colspan={5} class="pb-3 pl-4 pr-4 pt-0">
                  <RouterModels serverId={r.id} />
                </Table.Cell>
              </Table.Row>
            {/if}
          {/each}
        </Table.Body>
      </Table.Root>
    {/if}
  </div>
</section>

<Dialog.Root open={!!profileToDelete} onOpenChange={(open) => { if (!open) profileToDelete = null; }}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{t('srv.delete_confirm', { name: profileToDelete ? app.serverLabel(profileToDelete.id) : '' })}</Dialog.Title>
      <Dialog.Description>{t('srv.delete_dialog_desc')}</Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer class="mt-4 gap-2 sm:gap-0">
      <Button variant="outline" onclick={() => (profileToDelete = null)}>{t('srv.cancel')}</Button>
      <Button variant="destructive" onclick={confirmDelete} disabled={profileToDelete ? busy[profileToDelete.id] : false}>
        {t('srv.delete_yes')}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
