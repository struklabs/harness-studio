<script lang="ts">
  import BadgeCheck from '@lucide/svelte/icons/badge-check';
  import Users from '@lucide/svelte/icons/users';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { builtInPage, moduleActions, moduleStatusKey } from '#lib/modules.js';
  import { app } from '#lib/stores/app.svelte.js';
  import type { CatalogEntry } from '#lib/moduleCatalog.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { categoryIcon, categoryLabel } from './labels.js';

  let { entry, layout, tabbable, position, total, busy, onopen, onaction }: {
    entry: CatalogEntry;
    layout: 'grid' | 'list';
    /** Roving tabindex: only one card is in the tab order. */
    tabbable: boolean;
    position: number;
    total: number;
    busy: boolean;
    onopen: () => void;
    onaction: (command: string) => void;
  } = $props();

  let manifest = $derived(entry.view.manifest);
  let actions = $derived(moduleActions(entry.view, busy));
  let page = $derived(builtInPage(manifest.id));
  let statusKey = $derived(page ? 'modules.built_in' : manifest.planned ? 'modules.planned' : `modules.${moduleStatusKey(entry.view)}`);
  let Icon = $derived(categoryIcon(entry.category));
  let version = $derived(entry.view.state.installed_version ?? manifest.version);
  /** The single most useful one-click action for this card. */
  let primary = $derived(
    entry.updateAvailable && actions.repair ? { command: 'update', label: 'modules.update', variant: 'default' as const }
    : actions.install ? { command: 'install_module', label: entry.view.state.status === 'failed' ? 'modules.retry' : 'modules.install', variant: 'default' as const }
    : actions.enable ? { command: 'enable', label: 'modules.enable', variant: 'outline' as const }
    : null);
  let installing = $derived(entry.view.state.status === 'installing' || entry.view.progress !== null);
</script>

<div
  role="listitem"
  aria-setsize={total}
  aria-posinset={position}
  data-module-id={entry.id}
  class="flex h-full min-w-0 gap-3 rounded-lg border bg-card p-3 text-card-foreground {layout === 'grid' ? 'flex-col' : 'flex-row items-center'}"
>
  <button
    type="button"
    class="flex min-w-0 flex-1 gap-3 rounded-md text-left outline-none focus-visible:ring-3 focus-visible:ring-ring/50 {layout === 'grid' ? 'flex-col' : 'items-center'}"
    tabindex={tabbable ? 0 : -1}
    data-card-button
    aria-label={t('modules.open_details', { name: entry.name })}
    onclick={onopen}
  >
    <span class="flex min-w-0 items-start gap-3">
      <span class="flex size-9 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground" aria-hidden="true"><Icon class="size-5" /></span>
      <span class="min-w-0">
        <span class="block truncate font-medium">{entry.name}</span>
        <span class="flex flex-wrap items-center gap-x-1.5 text-xs text-muted-foreground">
          <span>v{version}</span>
          <span aria-hidden="true">·</span>
          <span class="truncate">{entry.author ?? t('modules.official_author')}</span>
        </span>
      </span>
    </span>
    <span class="text-sm text-muted-foreground {layout === 'grid' ? 'line-clamp-2 min-h-10' : 'line-clamp-1 flex-1'}">{entry.description}</span>
    <span class="flex flex-wrap items-center gap-1.5">
      {#if entry.origin === 'community'}
        <Badge variant="outline" class="border-amber-500/60 text-amber-700 dark:text-amber-300"><Users />{t('modules.badge_community')}</Badge>
      {:else}
        <Badge variant="secondary"><BadgeCheck />{t('modules.badge_official')}</Badge>
      {/if}
      <Badge variant={entry.view.state.status === 'failed' || entry.view.needs_repair?.length ? 'destructive' : entry.installed ? 'default' : 'outline'}>{t(statusKey as TKey)}</Badge>
      {#if entry.updateAvailable}<Badge variant="outline">{t('modules.badge_update')}</Badge>{/if}
      <span class="text-xs text-muted-foreground">{categoryLabel(entry.category)}</span>
    </span>
  </button>
  <div class="flex shrink-0 {layout === 'grid' ? 'justify-end' : ''}">
    {#if page}
      <Button size="sm" variant="outline" onclick={() => (app.requestedTab = page)}>{t(`modules.open_${manifest.id}` as TKey)}</Button>
    {:else if primary}
      <Button size="sm" variant={primary.variant} disabled={busy} onclick={() => onaction(primary.command)}>{t(primary.label as TKey)}</Button>
    {:else if installing}
      <span class="px-2 text-xs text-muted-foreground" role="status">{t('modules.installing')}…</span>
    {/if}
  </div>
</div>
