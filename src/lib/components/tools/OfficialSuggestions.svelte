<script lang="ts">
  // "Look before you draft": before the user has a model write a Skill, MCP server, Tool or
  // Connector, show the official packages (and Harness's own connectors) that already do it.
  import { onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { getLanguage, t, type TKey } from '#lib/i18n.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { security } from '#lib/stores/security.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import type { RegistryEntry } from './registryCatalog.ts';
  import { suggestConnectors, suggestOfficial, type SuggestKind } from './officialSuggest.ts';

  let { kind, query }: { kind: SuggestKind; query: string } = $props();
  let entries = $state<RegistryEntry[]>([]);
  onMount(() => { void invoke<{ index: { entries: RegistryEntry[] } }>('official_registry_list').then(s => entries = s.index.entries).catch(() => {}); });
  let official = $derived(kind === 'connector' ? [] : suggestOfficial(entries, kind, query));
  // A missing connector can also be an official MCP server, so both are offered.
  let mcp = $derived(kind === 'connector' ? suggestOfficial(entries, 'mcp', query) : []);
  let connectors = $derived(kind === 'connector' || kind === 'tool' || kind === 'mcp' ? suggestConnectors(query) : []);
  let shown = $derived([...official, ...mcp]);
</script>

{#if shown.length || connectors.length}
  <aside class="flex flex-col gap-2 rounded-lg border border-info/40 bg-info/5 p-3" aria-label={t('suggest.title')}>
    <h4 class="text-sm font-semibold">{t('suggest.title')}</h4>
    <p class="text-xs text-muted-foreground">{t('suggest.hint')}</p>
    {#each connectors as id (id)}
      <div class="flex flex-wrap items-center justify-between gap-2 text-sm">
        <span>{t('suggest.connector', { name: t(`connector.name_${id}` as TKey) })}</span>
        <Button size="sm" variant="outline" onclick={() => security.openSettings('tools-connectors')}>{t('suggest.open_connectors')}</Button>
      </div>
    {/each}
    {#each shown as entry (entry.id)}
      <div class="flex flex-wrap items-center justify-between gap-2 text-sm">
        <span class="min-w-0 break-words"><strong>{entry.name}</strong> · {t(`suggest.kind_${entry.type}` as TKey)} — {entry.description[getLanguage() === 'tr' ? 'tr' : 'en']}</span>
        <Button size="sm" variant="outline" onclick={() => { app.requestedRegistryKind = entry.type; security.openSettings('tools-installed'); }}>{t('suggest.open_catalog')}</Button>
      </div>
    {/each}
  </aside>
{/if}
