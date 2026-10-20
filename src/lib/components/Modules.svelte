<script lang="ts">
  import TrustPermissions from "./tools/TrustPermissions.svelte";
  import Agents from './Agents.svelte';
  import { onMount } from 'svelte';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { invoke } from '#lib/ipc.js';
  import { t, getLanguage, type TKey } from '#lib/i18n.svelte.js';
  import { errorText } from '#lib/errors.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { moduleActions, moduleStatusKey, progressPercent, type ModuleView, type LocaleKeys } from '#lib/modules.js';
  import { Button } from '#lib/components/ui/button/index.js';

  let modules = $state<ModuleView[]>([]);
  let selected = $state(app.requestedModule ?? 'speech');
  app.requestedModule = null;
  let error = $state('');
  let pending = $state(false);
  let removing = $state(false);
  let deleteHistory = $state(false);
  let deleteLogs = $state(false);
  let loading = $state(true);
  let reading = false;
  let disposed = false;
  let detail = $derived(modules.find(m => m.manifest.id === selected));
  let busy = $derived(pending || modules.some(m => m.state.status === 'installing' || m.progress));
  const text = (keys: LocaleKeys) => t(keys[getLanguage()] as TKey);
  const bytes = (n: number) => `${Math.ceil(n / 1024 / 1024)} MiB`;
  async function refresh() {
    if (reading) return;
    reading = true;
    try {
      const result = await invoke<ModuleView[]>('list_modules');
      if (!disposed) { modules = result; }
    } catch (e) { if (!disposed) error = errorText(e); }
    finally { reading = false; if (!disposed) loading = false; }
  }
  onMount(() => {
    disposed = false;
    void refresh();
    const timer = setInterval(() => void refresh(), 1500);
    // The first list answers at once with what is known; sizes and the agent probe finish in
    // the background, so ask again shortly until nothing is measuring.
    const soon = setInterval(() => { if (modules.some(m => m.measuring)) void refresh(); }, 400);
    return () => { disposed = true; clearInterval(timer); clearInterval(soon); };
  });
  async function action(command: string, id: string) {
    pending = true;
    error = '';
    let failure = '';
    try {
      if (command === 'remove_module_data') {
        await invoke(command, { id, options: { delete_history: deleteHistory, delete_logs: deleteLogs } });
        removing = false; deleteHistory = false; deleteLogs = false;
      } else if (['enable', 'disable', 'repair', 'update'].includes(command)) {
        await invoke('module_action', { id, action: command });
      } else await invoke(command, { id });
    }
    catch (e) { failure = errorText(e); }
    finally {
      if (!disposed) { pending = false; await refresh(); if (failure) error = failure; }
    }
  }
  /** Offline install: pick an official .hsmod bundle; nothing is downloaded. */
  async function installFromFile() {
    pending = true;
    error = '';
    let failure = '';
    try {
      const path = await invoke<string | null>('pick_module_bundle_file', { label: t('modules.bundle_filter') });
      if (path) {
        const report = await invoke<{ id: string }>('import_module_bundle', { path });
        selected = report.id;
      }
    } catch (e) { failure = errorText(e); }
    finally {
      if (!disposed) { pending = false; await refresh(); if (failure) error = failure; }
    }
  }
  function open() {
    if (selected === 'agents') return;
    app.requestedPlaygroundTab = 'transcription';
    app.requestedTab = 'playground';
  }
</script>

<div class="flex h-full min-h-0 flex-col overflow-y-auto p-4 md:p-6">
  <div class="mb-5 flex items-start justify-between gap-4">
    <div><h1 class="text-xl font-semibold">{t('modules.title')}</h1><p class="mt-1 text-sm text-muted-foreground">{t('modules.intro')}</p><p class="mt-2 text-xs text-muted-foreground">{t('discover.registry_scope')}</p></div>
    <div class="flex flex-wrap justify-end gap-2">
      <Button variant="outline" disabled={busy} title={t('modules.install_from_file_hint')} onclick={() => void installFromFile()}>{t('modules.install_from_file')}</Button>
      <Button variant="outline" onclick={() => { error = ''; void refresh(); }}>{t('modules.refresh')}</Button>
    </div>
  </div>
  {#if error}<p role="alert" class="mb-4 rounded-md border border-destructive p-3 text-sm text-destructive">{error}</p>{/if}
  {#if pending}<p role="status" aria-live="polite" class="mb-3 text-sm text-muted-foreground">{t('modules.working')}</p>{/if}
  {#if loading}<p role="status">{t('modules.loading')}</p>{/if}
  <div class="grid min-h-0 gap-5 lg:grid-cols-[15rem_1fr]">
    <nav class="flex flex-col gap-2" aria-label={t('modules.title')}>
      {#each modules as module (module.manifest.id)}
        <button class="rounded-lg border p-3 text-left hover:bg-muted" class:bg-muted={selected === module.manifest.id} aria-current={selected === module.manifest.id ? 'page' : undefined} onclick={() => { selected = module.manifest.id; removing = false; deleteHistory = false; deleteLogs = false; }}>
          <span class="block font-medium">{text(module.manifest.name)}</span>
          <span class="text-xs text-muted-foreground">{t(module.manifest.planned ? 'modules.planned' : `modules.${moduleStatusKey(module)}`)}</span>
        </button>
      {/each}
    </nav>
    {#if detail}
      {@const manifest = detail.manifest}
      {@const actions = moduleActions(detail, busy)}
      <section class="min-w-0 rounded-lg border p-4 md:p-5" aria-labelledby="module-name">
        <h2 id="module-name" class="text-lg font-semibold">{text(manifest.name)}</h2>
        <p class="mt-2 text-sm text-muted-foreground">{text(manifest.description)}</p>
        <p class="mt-2 text-xs text-muted-foreground">{t('modules.version')}: {detail.state.installed_version ?? manifest.version}</p>
        {#if !manifest.planned}
          <h3 class="mt-5 font-medium">{t('modules.functions')}</h3>
          <ul class="mt-2 list-inside list-disc text-sm">{#each manifest.functions as fn}<li>{text(fn)}</li>{/each}</ul>
          <h3 class="mt-5 font-medium">{t('modules.compatibility')}</h3>
          <p class="mt-2 text-sm">{detail.incompatibility ? errorText(detail.incompatibility) : t('modules.compatible')}</p>
          <p class="mt-1 text-sm">{t('modules.platforms')}: {manifest.platforms.join(', ')}</p>
          <p class="mt-1 text-sm">{t('modules.minimum_app')}: {manifest.min_app_version}</p>
          <h3 class="mt-4 font-medium">{t('modules.hardware')}</h3><p class="mt-2 text-sm">{text(manifest.hardware)}</p>
          <h3 class="mt-5 font-medium">{t('modules.dependencies')}</h3>
          <ul class="mt-2 space-y-3 text-sm">
            {#each manifest.components as component}
              <li class="rounded-md bg-muted/50 p-3">
                <div class="break-all font-medium">{component.id} <span class="text-xs font-normal text-muted-foreground">{manifest.shared_component_ids.includes(component.id) ? t('modules.shared') : ''}</span></div>
                <div class="mt-1 break-all text-xs">{t('modules.version')}: {detail.components[component.id]?.version || component.version}</div>
                <div class="mt-1 text-xs">{t('modules.disk_used')}: {bytes(detail.components[component.id]?.disk_bytes ?? 0)}</div>
                <div class="mt-1 text-xs">{t('modules.download')}: {bytes(component.download_bytes)} · {t('modules.estimate')}: {bytes(component.disk_bytes)}</div>
                {#each component.sources as source}<a class="mt-1 block break-all text-xs text-primary underline" href={source} onclick={(event) => { event.preventDefault(); void openUrl(source).catch(e => error = errorText(e)); }}>{source}</a>{/each}
                {#if detail.components[component.id]?.owners.length}<div class="mt-1 text-xs">{t('modules.owners')}: {detail.components[component.id].owners.join(', ')}</div>{/if}
              </li>
            {/each}
          </ul>
          <p class="mt-3 text-sm">{t('modules.estimate')}: {bytes(manifest.components.reduce((sum, c) => sum + c.disk_bytes, 0))}</p>
          <p class="mt-2 text-sm">{t('modules.disk_used')}: {bytes(detail.disk_bytes)} · {t('modules.shared_disk')}: {bytes(detail.shared_disk_bytes)}</p>
          <h3 class="mt-5 font-medium">{t('modules.permissions')}</h3>
          <TrustPermissions kind={manifest.id === "training" ? "trainer" : "native"} />
          <ul class="mt-2 list-inside list-disc space-y-1 text-sm">{#each manifest.permissions as permission}<li>{text(permission)}</li>{/each}</ul>
          <p class="mt-3 text-xs text-muted-foreground">{t('modules.approve')}</p>
          <p class="mt-2 text-xs text-muted-foreground">{t('modules.preserve')}</p>
          {#if detail.state.status === 'installing' || detail.progress}
            {@const percent = progressPercent(detail.progress?.downloaded_bytes ?? 0, detail.progress?.total_bytes ?? 0)}
            <div class="mt-4" role="status" aria-live="polite">
              <p class="text-sm">{t('modules.installing')}{detail.progress?.step ? ` · ${t(('modules.step_' + detail.progress.step) as TKey)}` : ''}</p>
              <progress class="mt-2 w-full" max="100" value={percent} aria-label={t('modules.installing')}></progress>
              {#if detail.progress}<p class="mt-1 break-all text-xs text-muted-foreground">{detail.progress.detail}</p>{/if}
            </div>
          {/if}
          {#if detail.needs_repair?.length}<p role="alert" class="mt-4 text-sm text-destructive">{t('modules.needs_repair_hint')} ({detail.needs_repair.join(', ')})</p>{/if}
          {#if detail.state.error}<p role="alert" class="mt-4 text-sm text-destructive">{errorText(detail.state.error)}</p>{/if}
          {#if manifest.id === 'agents'}<Agents enabled={actions.open} />{/if}
          <div class="mt-5 flex flex-wrap gap-2">
            {#if actions.install}<Button onclick={() => action('install_module', manifest.id)}>{t(detail.state.status === 'failed' ? 'modules.retry' : 'modules.install')}</Button>{/if}
            {#if actions.open && manifest.id !== 'agents'}<Button onclick={open}>{t('modules.open')}</Button>{/if}
            {#if actions.enable}<Button variant="outline" onclick={() => action('enable', manifest.id)}>{t('modules.enable')}</Button>{/if}
            {#if actions.disable}<Button variant="outline" onclick={() => action('disable', manifest.id)}>{t('modules.disable')}</Button>{/if}
            {#if actions.repair}<Button variant="outline" onclick={() => action('repair', manifest.id)}>{t('modules.repair')}</Button>{/if}
            {#if actions.repair && detail.update_available}<Button variant="outline" onclick={() => action('update', manifest.id)}>{t('modules.update')}</Button>{/if}
            {#if actions.remove}<Button variant="outline" onclick={() => removing = !removing}>{t('modules.remove')}</Button>{/if}
            {#if actions.cancel}<Button variant="outline" onclick={() => action('cancel_module_install', manifest.id)}>{t('modules.cancel')}</Button>{/if}
          </div>
          {#if removing}
            <div class="mt-4 space-y-3 rounded border p-3">
              <p class="text-sm">{t('modules.preserve')}</p>
              {#if detail.data_choices.length}<p class="text-xs text-muted-foreground">{t('modules.delete_scope')}</p>{/if}
              {#each detail.data_choices as choice}
                <label class="flex items-start gap-2 text-sm">
                  {#if choice.id === 'history'}<input type="checkbox" bind:checked={deleteHistory} disabled={busy} />
                  {:else}<input type="checkbox" bind:checked={deleteLogs} disabled={busy} />{/if}
                  <span>{t(choice.id === 'history' ? 'modules.delete_history' : 'modules.delete_logs')} ({bytes(choice.disk_bytes)})
                    <span class="block break-all text-xs text-muted-foreground">{choice.path}</span>
                  </span>
                </label>
              {/each}
              <Button variant="destructive" disabled={busy || !actions.remove} onclick={() => action('remove_module_data', manifest.id)}>{t('modules.confirm_remove')}</Button>
            </div>
          {/if}
        {:else}<p class="mt-5 text-sm text-muted-foreground">{t('modules.planned')}</p>{/if}
      </section>
    {/if}
  </div>
</div>
