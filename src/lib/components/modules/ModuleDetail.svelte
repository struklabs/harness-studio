<script lang="ts">
  import { openUrl } from '@tauri-apps/plugin-opener';
  import TrustPermissions from '../tools/TrustPermissions.svelte';
  import Agents from '../Agents.svelte';
  import { t, getLanguage, type TKey } from '#lib/i18n.svelte.js';
  import { errorText } from '#lib/errors.js';
  import { moduleActions, moduleStatusKey, progressPercent, type LocaleKeys } from '#lib/modules.js';
  import { formatBytes, type CatalogEntry } from '#lib/moduleCatalog.js';
  import * as Sheet from '#lib/components/ui/sheet/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Switch } from '#lib/components/ui/switch/index.js';
  import { categoryLabel, capabilityLabel, platformLabel } from './labels.js';

  let { entry, open = $bindable(false), busy, advanced, simulated, names, onaction, onremove, onopenmodule }: {
    entry: CatalogEntry | null;
    open: boolean;
    busy: boolean;
    advanced: boolean;
    /** Dev fixture: no backend behind the actions, so the live Agents panel is not mounted. */
    simulated: boolean;
    /** Display name of any module id, for the "required modules" list. */
    names: (id: string) => string;
    onaction: (command: string, id: string) => void;
    onremove: (id: string, options: { delete_history: boolean; delete_logs: boolean }) => void;
    onopenmodule: (id: string) => void;
  } = $props();

  const text = (keys: LocaleKeys) => t(keys[getLanguage()] as TKey);
  /** Catalog dates are `YYYY-MM-DD`; shown in the UI language (UTC, so the day never shifts). */
  const day = (iso: string) => { const d = new Date(`${iso}T00:00:00Z`); return Number.isNaN(d.getTime()) ? iso : d.toLocaleDateString(getLanguage(), { dateStyle: 'medium', timeZone: 'UTC' }); };
  let removing = $state(false);
  let deleteHistory = $state(false);
  let deleteLogs = $state(false);
  let linkError = $state('');
  $effect(() => { void entry?.id; removing = false; deleteHistory = false; deleteLogs = false; linkError = ''; });

  let view = $derived(entry?.view);
  let actions = $derived(view ? moduleActions(view, busy) : null);
  let manifest = $derived(view?.manifest);
  let downloadBytes = $derived(manifest?.components.reduce((sum, c) => sum + c.download_bytes, 0) ?? 0);
  let estimateBytes = $derived(manifest?.components.reduce((sum, c) => sum + c.disk_bytes, 0) ?? 0);
  const link = (url: string) => (event: MouseEvent) => { event.preventDefault(); void openUrl(url).catch(e => linkError = errorText(e)); };
</script>

<Sheet.Root bind:open>
  <Sheet.Content class="w-full gap-0 overflow-y-auto data-[side=right]:sm:max-w-xl">
    {#if entry && view && manifest && actions}
      {@const statusKey = manifest.planned ? 'modules.planned' : `modules.${moduleStatusKey(view)}`}
      <Sheet.Header class="border-b">
        <div class="flex flex-wrap items-center gap-1.5 pr-8">
          {#if entry.origin === 'community'}
            <Badge variant="outline" class="border-amber-500/60 text-amber-700 dark:text-amber-300">{t('modules.badge_community')}</Badge>
          {:else}<Badge variant="secondary">{t('modules.badge_official')}</Badge>{/if}
          <Badge variant={entry.installed ? 'default' : 'outline'}>{t(statusKey as TKey)}</Badge>
          {#if entry.updateAvailable}<Badge variant="outline">{t('modules.badge_update')}</Badge>{/if}
        </div>
        <Sheet.Title class="text-lg">{entry.name}</Sheet.Title>
        <Sheet.Description>v{view.state.installed_version ?? manifest.version} · {entry.author ?? t('modules.official_author')} · {categoryLabel(entry.category)}</Sheet.Description>
      </Sheet.Header>

      <div class="space-y-5 p-4 text-sm">
        {#if entry.origin === 'community'}
          <p role="note" class="rounded-md border border-amber-500/60 p-3 text-xs">{t('modules.community_notice')}</p>
        {/if}

        <div class="flex flex-wrap gap-2">
          {#if actions.install}<Button disabled={busy} onclick={() => onaction('install_module', manifest.id)}>{t(view.state.status === 'failed' ? 'modules.retry' : 'modules.install')}</Button>{/if}
          {#if entry.updateAvailable && actions.repair}<Button disabled={busy} onclick={() => onaction('update', manifest.id)}>{t('modules.update')}</Button>{/if}
          {#if actions.open && manifest.id !== 'agents'}<Button onclick={() => onopenmodule(manifest.id)}>{t('modules.open')}</Button>{/if}
          {#if actions.enable}<Button variant="outline" onclick={() => onaction('enable', manifest.id)}>{t('modules.enable')}</Button>{/if}
          {#if actions.disable}<Button variant="outline" onclick={() => onaction('disable', manifest.id)}>{t('modules.disable')}</Button>{/if}
          {#if advanced && actions.repair}<Button variant="outline" onclick={() => onaction('repair', manifest.id)}>{t('modules.repair')}</Button>{/if}
          {#if actions.cancel}<Button variant="outline" onclick={() => onaction('cancel_module_install', manifest.id)}>{t('modules.cancel')}</Button>{/if}
          {#if actions.remove}<Button variant="destructive" onclick={() => removing = !removing}>{t('modules.uninstall')}</Button>{/if}
        </div>

        {#if view.state.status === 'installing' || view.progress}
          {@const percent = progressPercent(view.progress?.downloaded_bytes ?? 0, view.progress?.total_bytes ?? 0)}
          <div role="status" aria-live="polite">
            <p>{t('modules.installing')}{view.progress?.step ? ` · ${t(('modules.step_' + view.progress.step) as TKey)}` : ''}</p>
            <progress class="mt-2 w-full" max="100" value={percent} aria-label={t('modules.installing')}></progress>
            {#if view.progress}<p class="mt-1 break-all text-xs text-muted-foreground">{view.progress.detail}</p>{/if}
          </div>
        {/if}
        {#if view.needs_repair?.length}<p role="alert" class="text-destructive">{t('modules.needs_repair_hint')} ({view.needs_repair.join(', ')})</p>{/if}
        {#if view.state.error}<p role="alert" class="text-destructive">{errorText(view.state.error)}</p>{/if}
        {#if linkError}<p role="alert" class="text-destructive">{linkError}</p>{/if}

        {#if removing && actions.remove}
          <div class="space-y-3 rounded-md border p-3">
            <p>{t('modules.preserve')}</p>
            {#if view.data_choices.length}<p class="text-xs text-muted-foreground">{t('modules.delete_scope')}</p>{/if}
            {#each view.data_choices as choice}
              <label class="flex items-start gap-2">
                {#if choice.id === 'history'}<input type="checkbox" bind:checked={deleteHistory} disabled={busy} />
                {:else}<input type="checkbox" bind:checked={deleteLogs} disabled={busy} />{/if}
                <span>{t(choice.id === 'history' ? 'modules.delete_history' : 'modules.delete_logs')} ({formatBytes(choice.disk_bytes)})
                  <span class="block break-all text-xs text-muted-foreground">{choice.path}</span></span>
              </label>
            {/each}
            <Button variant="destructive" disabled={busy || !actions.remove} onclick={() => { onremove(manifest.id, { delete_history: deleteHistory, delete_logs: deleteLogs }); removing = false; }}>{t('modules.confirm_remove')}</Button>
          </div>
        {/if}

        <section aria-labelledby="md-desc">
          <h3 id="md-desc" class="font-medium">{t('modules.description')}</h3>
          <p class="mt-1 text-muted-foreground">{entry.description}</p>
          {#if !manifest.planned && manifest.functions.length}
            <ul class="mt-2 list-inside list-disc">{#each manifest.functions as fn}<li>{text(fn)}</li>{/each}</ul>
          {/if}
          {#if entry.tags.length}
            <ul class="mt-2 flex flex-wrap gap-1.5">{#each entry.tags as tag}<li><Badge variant="outline">{tag}</Badge></li>{/each}</ul>
          {/if}
        </section>

        <section aria-labelledby="md-source">
          <h3 id="md-source" class="font-medium">{t('modules.source')}</h3>
          <dl class="mt-1 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1">
            <dt class="text-muted-foreground">{t('modules.author')}</dt><dd>{entry.author ?? t('modules.official_author')}</dd>
            <dt class="text-muted-foreground">{t('modules.license')}</dt><dd>{view.catalog?.license ?? t('modules.unknown_value')}</dd>
            <dt class="text-muted-foreground">{t('modules.repository')}</dt>
            <dd class="break-all">{#if view.catalog?.repository}<a class="text-primary underline" href={view.catalog?.repository} onclick={link(view.catalog?.repository)}>{view.catalog?.repository}</a>{:else}{t('modules.unknown_value')}{/if}</dd>
            <dt class="text-muted-foreground">{t('modules.updated')}</dt><dd>{view.catalog?.updated_at ? day(view.catalog.updated_at) : t('modules.unknown_value')}</dd>
            <dt class="text-muted-foreground">{t('modules.version')}</dt><dd>{view.state.installed_version ?? manifest.version}</dd>
          </dl>
        </section>

        {#if !manifest.planned}
          <section aria-labelledby="md-compat">
            <h3 id="md-compat" class="font-medium">{t('modules.compatibility')}</h3>
            <p class="mt-1">{view.incompatibility ? errorText(view.incompatibility) : t('modules.compatible')}</p>
            <p class="mt-1 text-muted-foreground">{t('modules.platforms')}: {manifest.platforms.map(platformLabel).join(', ')}</p>
            <p class="mt-1 text-muted-foreground">{t('modules.minimum_app')}: {manifest.min_app_version}</p>
            <p class="mt-1 text-muted-foreground">{t('modules.hardware')}: {text(manifest.hardware)}</p>
            {#if entry.capabilities.length}<p class="mt-1 text-muted-foreground">{t('modules.filter_capability')}: {entry.capabilities.map(capabilityLabel).join(', ')}</p>{/if}
          </section>

          <section aria-labelledby="md-size">
            <h3 id="md-size" class="font-medium">{t('modules.size')}</h3>
            <dl class="mt-1 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1">
              <dt class="text-muted-foreground">{t('modules.download_size')}</dt><dd>{formatBytes(downloadBytes)}</dd>
              <dt class="text-muted-foreground">{t('modules.estimate')}</dt><dd>{formatBytes(estimateBytes)}</dd>
              {#if entry.installed}<dt class="text-muted-foreground">{t('modules.disk_used')}</dt><dd>{formatBytes(view.disk_bytes)} · {t('modules.shared_disk')}: {formatBytes(view.shared_disk_bytes)}</dd>{/if}
            </dl>
          </section>

          <section aria-labelledby="md-perm">
            <h3 id="md-perm" class="font-medium">{t('modules.permissions')}</h3>
            <div class="mt-2"><TrustPermissions kind={manifest.id === 'training' ? 'trainer' : 'native'} /></div>
            <ul class="mt-2 list-inside list-disc space-y-1">{#each manifest.permissions as permission}<li>{text(permission)}</li>{/each}</ul>
            <p class="mt-2 text-xs text-muted-foreground">{t('modules.approve')}</p>
          </section>

          <section aria-labelledby="md-req">
            <h3 id="md-req" class="font-medium">{t('modules.requires')}</h3>
            {#if view.catalog?.requires?.length}
              <ul class="mt-1 list-inside list-disc">{#each view.catalog?.requires as id}<li>{names(id)}</li>{/each}</ul>
            {:else}<p class="mt-1 text-muted-foreground">{t('modules.none')}</p>{/if}
          </section>

          {#if entry.installed}
            <section aria-labelledby="md-settings">
              <h3 id="md-settings" class="font-medium">{t('modules.settings')}</h3>
              <label class="mt-2 flex items-center justify-between gap-3">
                <span>{t('modules.settings_enabled')}<span class="block text-xs text-muted-foreground">{t('modules.settings_hint')}</span></span>
                <Switch checked={entry.enabled} disabled={busy || !(entry.enabled ? actions.disable : actions.enable)} onCheckedChange={(on) => onaction(on ? 'enable' : 'disable', manifest.id)} aria-label={t('modules.settings_enabled')} />
              </label>
              {#if manifest.id === 'agents' && !simulated}<div class="mt-3"><Agents enabled={actions.open} /></div>{/if}
            </section>
          {/if}

          {#if advanced}
            <section aria-labelledby="md-tech">
              <h3 id="md-tech" class="font-medium">{t('modules.technical')}</h3>
              <ul class="mt-2 space-y-2">
                {#each manifest.components as component}
                  <li class="rounded-md bg-muted/50 p-3">
                    <div class="break-all font-medium">{component.id} <span class="text-xs font-normal text-muted-foreground">{manifest.shared_component_ids.includes(component.id) ? t('modules.shared') : ''}</span></div>
                    <div class="mt-1 break-all text-xs">{t('modules.version')}: {view.components[component.id]?.version || component.version}</div>
                    <div class="mt-1 text-xs">{t('modules.disk_used')}: {formatBytes(view.components[component.id]?.disk_bytes ?? 0)}</div>
                    <div class="mt-1 text-xs">{t('modules.download')}: {formatBytes(component.download_bytes)} · {t('modules.estimate')}: {formatBytes(component.disk_bytes)}</div>
                    {#each component.sources as source}<a class="mt-1 block break-all text-xs text-primary underline" href={source} onclick={link(source)}>{source}</a>{/each}
                    {#if view.components[component.id]?.owners.length}<div class="mt-1 text-xs">{t('modules.owners')}: {view.components[component.id].owners.join(', ')}</div>{/if}
                  </li>
                {/each}
              </ul>
              <p class="mt-2 text-xs text-muted-foreground">{t('modules.preserve')}</p>
            </section>
          {/if}
        {/if}

        <section aria-labelledby="md-log">
          <h3 id="md-log" class="font-medium">{t('modules.changelog')}</h3>
          {#if view.catalog?.changelog?.length}
            <ol class="mt-1 space-y-2">{#each view.catalog?.changelog as item}<li><span class="font-medium">v{item.version}</span> <span class="text-muted-foreground">{text(item.notes)}</span></li>{/each}</ol>
          {:else}<p class="mt-1 text-muted-foreground">{t('modules.no_changelog')}</p>{/if}
        </section>
      </div>
    {/if}
  </Sheet.Content>
</Sheet.Root>
