<script lang="ts">
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import * as Empty from '#lib/components/ui/empty/index.js';
  import { Wrench } from '@lucide/svelte';
  import { Button } from '#lib/components/ui/button/index.js';
  import InstalledToolsSettings from '../settings/InstalledToolsSettings.svelte';
  import { categories, categoryLabel, inventory, type InventoryTool } from './client';
  let { revision = 0 }: { revision?: number } = $props();
  let tools = $state<InventoryTool[]>([]);
  let error = $state('');
  let loading = $state(false);
  async function load() { loading = true; try { tools = await inventory(); error = ''; } catch (e) { error = localizeError(e); } finally { loading = false; } }
  $effect(() => { void revision; void load(); });
  let sources = $derived([...new Set(tools.map(t => t.source === 'mcp' ? 'mcp:' + (t.server ?? '') : t.source))]);
</script>
<div class="flex flex-wrap items-center justify-between gap-3"><p class="text-xs text-muted-foreground">{t('create.mine_desc')}</p><Button variant="outline" disabled={loading} onclick={load}>{t('create.refresh')}</Button></div>
{#if error}<p role="alert" class="text-xs text-destructive">{error}</p>{/if}
{#if loading && !tools.length}<p role="status" class="text-sm text-muted-foreground">{t('create.inventory_loading')}</p>
{:else if !error && !tools.length}
  <Empty.Root class="border border-border bg-card py-10"><Empty.Header><Empty.Media variant="icon"><Wrench /></Empty.Media><Empty.Title>{t('create.inventory_empty')}</Empty.Title><Empty.Description>{t('create.inventory_empty_hint')}</Empty.Description></Empty.Header></Empty.Root>
{/if}
{#each sources as source}
  <section><h2 class="border-b border-border pb-2 text-sm font-semibold">{source.startsWith('mcp:') ? 'MCP · ' + source.slice(4) : t(('create.source_' + source) as TKey)}</h2>
    {#each categories as category}
      {@const items = tools.filter(t => t.category === category && (t.source === 'mcp' ? 'mcp:' + (t.server ?? '') : t.source) === source)}
      {#if items.length}
        <h3 class="mt-3 mb-1 text-xs font-medium text-muted-foreground">{categoryLabel(category)} · {items.length}</h3>
        <ul class="divide-y divide-border">{#each items as item (item.name)}<li class="py-2"><p class="font-mono text-xs font-medium">{item.name}</p><p class="mt-1 text-xs text-muted-foreground">{item.description}</p></li>{/each}</ul>
      {/if}
    {/each}
  </section>
{/each}
<InstalledToolsSettings onChanged={load} />
