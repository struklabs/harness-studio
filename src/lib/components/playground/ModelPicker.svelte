<script lang="ts">
  // Playground model picker: every model definition with its run state (downloaded / loading /
  // loaded & running / not downloaded). Load a definition on the selected server, a new server or
  // one of its profiles, unload it, or switch the conversation to a server that runs it.
  import { invoke } from '#lib/ipc.js';
  import { toast } from 'svelte-sonner';
  import type { DefinitionRunState, LoadTarget, ModelDefinition, ParamValues } from '#lib/types.js';
  import LoadSettings from './LoadSettings.svelte';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { savedPortText } from '../servers/utils';
  import { definitionsForMode } from './modelModes';
  import { library } from '#lib/stores/library.svelte.js';
  import { security } from '#lib/stores/security.svelte.js';
  import { filterRemoteModels, isRemoteModel, type RemoteModel } from './remoteModels';
  import { Input } from '#lib/components/ui/input/index.js';
  import type { ServeMode } from '#lib/types.js';
  import { cn } from '#lib/utils.js';
  import { localizeError } from '#lib/network.js';
  import { app, DEFAULT_SERVER_ID } from '#lib/stores/app.svelte.js';
  import { domain, baseName, definitionFile } from '#lib/stores/domain.svelte.js';
  import { unsavedLoad } from '#lib/stores/unsavedLoad.svelte.js';
  import * as Popover from '#lib/components/ui/popover/index.js';
  import * as DropdownMenu from '#lib/components/ui/dropdown-menu/index.js';
  import { Button, buttonVariants } from '#lib/components/ui/button/index.js';
  import { Boxes, ChevronDown, LoaderCircle, Play, Square, ArrowRightLeft, RefreshCw, SlidersHorizontal } from '@lucide/svelte';

  interface Props {
    /** Model id the conversation sends (`model`). */
    currentModel: string;
    /** Switch the conversation to `serverId` serving `alias`. */
    onUse: (serverId: string, alias: string) => void;
    /** Called after the popover closed because a model was chosen; the parent returns focus to the composer. */
    onPicked?: () => void;
    /** Label override when the conversation targets something other than `currentModel` (e.g. embedding server). */
    label?: string;
    mode?: ServeMode;
  }

  let { currentModel, onUse, onPicked, label, mode = 'chat' }: Props = $props();

  let open = $state(false);
  let query = $state('');
  let remoteModels = $state<RemoteModel[]>([]);
  let remoteError = $state('');
  let remoteRows = $derived(filterRemoteModels(remoteModels, query));
  let selectedRemote = $derived(remoteModels.find((m) => m.id === currentModel));
  $effect(() => {
    if (!open || mode !== 'chat') return;
    let alive = true;
    async function refresh() {
      try {
        const models = await invoke<RemoteModel[]>('link_models');
        if (alive) { remoteModels = models; remoteError = ''; }
      } catch (e) { if (alive) remoteError = localizeError(e); }
    }
    void refresh();
    const timer = setInterval(() => void refresh(), 10000);
    return () => { alive = false; clearInterval(timer); };
  });
  /** Closed with Escape: focus returns to the trigger (a keyboard user would land on <body>). */
  let escaped = false;
  let busy = $state<Record<string, boolean>>({});
  /** Definition whose "Load Settings" are open (one at a time). */
  let settingsFor = $state<string | null>(null);
  $effect(() => {
    if (!open) settingsFor = null;
    // A fresh open: an Escape a nested menu handled earlier must not decide this close.
    else escaped = false;
  });

  const ORDER: Record<DefinitionRunState, number> = { running: 0, loading: 1, idle: 2, missing: 3 };
  const LABEL: Record<DefinitionRunState, TKey> = {
    running: 'pgm.state_running',
    loading: 'pgm.state_loading',
    idle: 'pgm.state_idle',
    missing: 'pgm.state_missing',
  };
  const TONE: Record<DefinitionRunState, string> = {
    running: 'text-success',
    loading: 'text-warning',
    idle: 'text-muted-foreground',
    missing: 'text-muted-foreground/70',
  };

  let rows = $derived(
    definitionsForMode(domain.definitions, mode, library.models)
      .filter((d) => query.trim().toLowerCase().split(/\s+/).every((word) => `${d.alias} ${definitionFile(d) ?? ''}`.toLowerCase().includes(word)))
      .map((d) => ({ def: d, st: domain.state(d.id) }))
      .sort(
        (a, b) =>
          ORDER[a.st?.state ?? 'idle'] - ORDER[b.st?.state ?? 'idle'] || a.def.alias.localeCompare(b.def.alias)
      )
  );

  /** The selected server belongs to a profile (it runs that profile's definition only). */
  let selectedIsProfile = $derived(
    app.selectedServerId !== DEFAULT_SERVER_ID && app.profiles.some((p) => p.id === app.selectedServerId)
  );

  let current = $derived(
    domain.definitions.find((d) => d.alias.toLowerCase() === currentModel.toLowerCase()) ?? null
  );

  // Keep the states fresh while the picker is open (servers change once a second).
  let serversSig = $derived(app.servers.map((s) => `${s.id}:${JSON.stringify(s.status)}`).join('|'));
  $effect(() => {
    void serversSig;
    if (!open) return;
    void domain.refreshStates();
  });
  $effect(() => {
    if (!open) return;
    void domain.refreshDefinitions();
    void library.refresh();
  });
  // Command palette: "Start a model…" opens the picker.
  $effect(() => {
    if (!domain.pendingModelPicker) return;
    domain.pendingModelPicker = false;
    open = true;
  });

  /** Switch to a running server's model, then close the popover and hand focus back to the composer. */
  function use(serverId: string, alias: string) {
    onUse(serverId, alias);
    closeAndRestore();
  }

  function closeAndRestore() {
    open = false;
    onPicked?.();
  }

  // Escape must close the popover even when focus was lost (the clicked button can be removed
  // from the list once its model is selected, which leaves focus on <body>). Nested menus keep
  // their own Escape handling.
  function onWindowKeydown(e: KeyboardEvent) {
    if (!open || e.key !== 'Escape') return;
    if (document.querySelector('[role="menu"][data-state="open"]')) return;
    e.preventDefault();
    escaped = true;
    open = false;
  }

  function profilesOf(def: ModelDefinition) {
    return app.profiles.filter((p) => p.config.definition_id === def.id);
  }

  /**
   * Loads `def` on `target`. `overrides` (Load Settings) apply to this load only; with `save`
   * they are written to the definition first and the load then uses the definition itself.
   */
  async function load(def: ModelDefinition, target: LoadTarget, overrides?: ParamValues, save = false) {
    // Another model than the one with unsaved manual settings: ask before switching.
    if (unsavedLoad.pending && unsavedLoad.pending.key !== `playground:${def.id}`) await unsavedLoad.flush();
    busy[def.id] = true;
    try {
      if (save && overrides) {
        try {
          await invoke('save_definition_load', { definitionId: def.id, overrides });
        } catch (e) {
          toast.error(t('pgm.save_failed', { error: localizeError(e) }));
          return;
        }
        unsavedLoad.clear(`playground:${def.id}`);
        await domain.refreshDefinitions();
        toast.success(t('pgm.saved_toast', { alias: def.alias }));
      }
      const once = !save && overrides && Object.keys(overrides).length ? overrides : undefined;
      const serverId = await invoke<string>('load_definition', { definitionId: def.id, target, overrides: once });
      // Settings changed by hand and used for this load only: asked about when the model
      // changes, the server stops, the screen is left or the app closes.
      if (once) {
        unsavedLoad.register({
          key: `playground:${def.id}`,
          alias: def.alias,
          save: async () => {
            await invoke('save_definition_load', { definitionId: def.id, overrides: once });
            await domain.refreshDefinitions();
          },
        });
      } else {
        unsavedLoad.clear(`playground:${def.id}`);
      }
      await app.refreshProfiles();
      onUse(serverId, def.alias);
      closeAndRestore();
      toast.success(t('pgm.loading_toast', { alias: def.alias, server: app.serverLabel(serverId) }));
      await domain.refreshStates();
    } catch (e) {
      toast.error(t('pgm.load_failed', { error: localizeError(e) }));
    } finally {
      busy[def.id] = false;
    }
  }

  async function unload(def: ModelDefinition, serverId: string) {
    busy[def.id] = true;
    try {
      await invoke('stop_server', { serverId });
      void unsavedLoad.flush();
      await domain.refreshStates();
    } catch (e) {
      toast.error(String(e));
    } finally {
      busy[def.id] = false;
    }
  }
</script>

<svelte:window onkeydown={onWindowKeydown} />

<Popover.Root bind:open>
  <Popover.Trigger class={cn(buttonVariants({ variant: 'outline', size: 'sm' }), 'max-w-64 min-w-32 shrink gap-1.5')} aria-label={t('pgm.open')}>
    <Boxes class="size-3.5 shrink-0" />
    <span class="min-w-0 truncate font-mono text-xs">{label ?? current?.alias ?? (currentModel || t('pgm.none'))}</span>
    <span
      class={cn('size-1.5 shrink-0 rounded-full', isRemoteModel(currentModel) ? app.gatewayRunning && selectedRemote?.state === 'online' ? 'bg-success' : 'bg-muted-foreground/40' : app.isRunning ? 'bg-success' : app.isBusy ? 'bg-warning' : 'bg-muted-foreground/40')}
      aria-hidden="true"
    ></span>
    <ChevronDown class="size-3.5 shrink-0 opacity-60" />
  </Popover.Trigger>
  <Popover.Content
    align="end"
    class="w-[420px] p-0"
    onEscapeKeydown={() => (escaped = true)}
    onCloseAutoFocus={(e) => {
      // After a pick the composer takes focus (onPicked) and after a click elsewhere the clicked
      // element keeps it: do not yank it back to the trigger. After Escape it returns there.
      if (!escaped) e.preventDefault();
      escaped = false;
    }}
  >
    <div class="flex items-center justify-between gap-2 border-b border-border px-3 py-2">
      <span class="text-sm font-semibold text-foreground">{t('pgm.title')}</span>
      <span class="flex min-w-0 items-center gap-1 text-xs text-muted-foreground">
        <span class="truncate">{t('pgm.target', { server: app.serverLabel(app.selectedServerId) })}</span>
        <Button variant="ghost" size="icon-xs" aria-label={t('pgm.refresh')} onclick={() => domain.refreshStates()}>
          <RefreshCw />
        </Button>
      </span>
    </div>
    <div class="border-b border-border p-2">
      <Input bind:value={query} aria-label={t('link.model_filter')} placeholder={t('link.model_filter')} />
    </div>
    <div class="max-h-96 overflow-y-auto p-1">
      {#if mode === 'chat'}
        <div class="border-b border-border pb-1 mb-1">
          <h3 class="px-2 py-1.5 text-xs font-semibold">{t('link.remote')}</h3>
          {#if !app.gatewayRunning}
            <p class="px-2 py-1 text-xs text-muted-foreground">{t('link.gateway_off')}</p>
            <Button variant="outline" size="xs" class="m-2" onclick={() => { open = false; security.openSettings('gateway'); }}>{t('gw.ep_open_settings')}</Button>
          {:else}
            {#if remoteError}<p class="px-2 py-1 text-xs text-destructive" role="alert">{remoteError}</p>{/if}
            {#each remoteRows as model (model.id)}
              <div title={model.state !== 'online' ? t('link.unreachable') : model.id}>
                <button
                  type="button"
                  class={cn('flex w-full flex-col rounded-md px-2 py-1.5 text-left hover:bg-muted/60 disabled:opacity-50 disabled:cursor-not-allowed', currentModel === model.id && 'bg-muted/60')}
                  disabled={model.state !== 'online'}
                  onclick={() => { if (app.gatewayRunning && model.state === 'online') use(app.selectedServerId, model.id); }}
                >
                  <span class="w-full truncate font-mono text-xs">{model.model}</span>
                  <span class={cn('text-xs', model.state === 'online' ? 'text-success' : 'text-muted-foreground')}>{model.link} · {t(model.state === 'online' ? 'link.online' : 'link.offline')}</span>
                </button>
              </div>
            {:else}
              <p class="px-2 py-1 text-xs text-muted-foreground">{t('link.models_empty')}</p>
            {/each}
          {/if}
        </div>
      {/if}
      {#if rows.length === 0}
        <p class="px-3 py-4 text-xs text-muted-foreground">{t('pgm.empty')}</p>
      {/if}
      {#each rows as { def, st } (def.id)}
        {@const state = st?.state ?? 'idle'}
        {@const file = definitionFile(def)}
        <div>
        <div class={cn('flex min-w-0 items-center gap-2 rounded-md px-2 py-1.5', current?.id === def.id && 'bg-muted/60')}>
          <div class="flex min-w-0 flex-1 flex-col">
            <span class="truncate font-mono text-xs text-foreground" title={def.alias}>{def.alias}</span>
            <span class="truncate text-xs text-muted-foreground" title={file ?? ''}>
              <span class={TONE[state]}>{t(LABEL[state])}</span>
              {#if st?.servers.length}
                · {st.servers.map((s) => `${app.serverLabel(s.server_id)} :${s.port}`).join(', ')}
              {:else if file}
                · {baseName(file)}
              {/if}
            </span>
          </div>
          {#if busy[def.id] || state === 'loading'}
            <LoaderCircle class="size-3.5 shrink-0 animate-spin text-muted-foreground" aria-label={t('pgm.state_loading')} />
          {/if}
          {#if state === 'running' || state === 'idle'}
            <Button
              variant="ghost"
              size="icon-xs"
              aria-expanded={settingsFor === def.id}
              aria-label={settingsFor === def.id ? t('pgm.settings_close') : t('pgm.settings_open')}
              title={t('pgm.settings')}
              onclick={() => (settingsFor = settingsFor === def.id ? null : def.id)}
            >
              <SlidersHorizontal />
            </Button>
          {/if}
          {#if state === 'running' && st}
            {#each st.servers.filter((s) => s.status === 'Running') as s (s.server_id)}
              {#if !(s.server_id === app.selectedServerId && current?.id === def.id)}
                <Button size="xs" variant="outline" onclick={() => use(s.server_id, def.alias)} title={t('pgm.use_title')}>
                  <ArrowRightLeft />{t('pgm.use')}
                </Button>
              {/if}
              <Button size="xs" variant="ghost" disabled={busy[def.id]} onclick={() => unload(def, s.server_id)} title={t('pgm.unload_title', { server: app.serverLabel(s.server_id) })}>
                <Square />{t('pgm.unload')}
              </Button>
            {/each}
          {:else if state === 'idle'}
            <DropdownMenu.Root>
              <DropdownMenu.Trigger class={cn(buttonVariants({ variant: 'outline', size: 'xs' }))} disabled={busy[def.id]}>
                <Play />{t('pgm.load')}<ChevronDown class="opacity-60" />
              </DropdownMenu.Trigger>
              <DropdownMenu.Content align="end" class="w-64">
                <DropdownMenu.Label class="text-xs text-muted-foreground">{t('pgm.load_with_default')}</DropdownMenu.Label>
                {#if !selectedIsProfile}
                  <DropdownMenu.Item onclick={() => load(def, { kind: 'server', server_id: app.selectedServerId })}>
                    <span class="min-w-0 truncate">
                      {app.isRunning || app.isBusy
                        ? t('pgm.load_here_restart', { server: app.serverLabel(app.selectedServerId) })
                        : t('pgm.load_here', { server: app.serverLabel(app.selectedServerId) })}
                    </span>
                  </DropdownMenu.Item>
                {/if}
                <DropdownMenu.Item onclick={() => load(def, { kind: 'new_server' })}>{t('pgm.load_new')}</DropdownMenu.Item>
                {#if profilesOf(def).length}
                  <DropdownMenu.Separator />
                  <DropdownMenu.Label class="text-xs text-muted-foreground">{t('pgm.load_with_profile')}</DropdownMenu.Label>
                  {#each profilesOf(def) as p (p.id)}
                    <DropdownMenu.Item onclick={() => load(def, { kind: 'profile', profile_id: p.id })}>
                      <span class="min-w-0 truncate">{p.name} <span class="font-num text-muted-foreground">{savedPortText(p.config, t('dash.port_auto'))}</span></span>
                    </DropdownMenu.Item>
                  {/each}
                {/if}
              </DropdownMenu.Content>
            </DropdownMenu.Root>
          {:else if state === 'missing'}
            <span class="shrink-0 text-xs text-muted-foreground" title={t('pgm.missing_hint')}>{t('pgm.missing_short')}</span>
          {/if}
        </div>
        {#if settingsFor === def.id}
          <LoadSettings
            {def}
            runningServerId={st?.servers.find((s) => s.status === 'Running')?.server_id ?? null}
            busy={!!busy[def.id]}
            onLoad={(target, overrides) => load(def, target, overrides)}
            onLoadAndSave={(target, overrides) => load(def, target, overrides, true)}
          />
        {/if}
        </div>
      {/each}
    </div>
  </Popover.Content>
</Popover.Root>
