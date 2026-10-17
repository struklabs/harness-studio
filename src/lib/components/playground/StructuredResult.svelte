<script lang="ts">
  import { Button } from '#lib/components/ui/button/index.js';
  import { t } from '#lib/i18n.svelte.js';
  import DataTable from './DataTable.svelte';
  import { structuredResult, displayValue } from './generativeUi';
  import { scrollFocusable } from '#lib/actions/scrollFocusable.js';
  let { source }: { source: string } = $props();
  let raw = $state(false);
  let data = $derived(structuredResult(source));
  let headers = $derived(Array.isArray(data) ? [...new Set(data.flatMap(Object.keys))] : []);
</script>

<div class="mt-2 min-w-0 space-y-2">
  {#if data !== null}
    <div class="flex justify-end"><Button variant="ghost" size="xs" aria-pressed={raw} onclick={() => raw = !raw}>{raw ? t('pg.rich_preview') : t('pg.rich_raw')}</Button></div>
  {/if}
  {#if raw || data === null}
    <pre use:scrollFocusable={t('pg.rich_raw')} class="max-h-80 overflow-auto rounded-md border border-border bg-muted p-3 font-mono text-xs whitespace-pre-wrap focus-visible:outline-offset-[-2px] [overflow-wrap:anywhere]">{source}</pre>
  {:else if Array.isArray(data)}
    <DataTable {headers} rows={data.map(row => headers.map(key => displayValue(row[key])))} sortable />
  {:else}
    <dl class="divide-y divide-border rounded-lg border border-border bg-card px-3 text-sm">
      {#each Object.entries(data) as [key, value]}<div class="grid grid-cols-[minmax(0,1fr)_minmax(0,2fr)] gap-3 py-2.5 [overflow-wrap:anywhere]"><dt class="text-muted-foreground">{key}</dt><dd class="whitespace-pre-wrap">{displayValue(value)}</dd></div>{/each}
      {#if !Object.keys(data).length}<p class="py-3 text-muted-foreground">{t('pg.rich_empty')}</p>{/if}
    </dl>
  {/if}
</div>
