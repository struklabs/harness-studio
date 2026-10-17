<script lang="ts">
  import { Button } from '#lib/components/ui/button/index.js';
  import { t } from '#lib/i18n.svelte.js';
  import { toast } from 'svelte-sonner';
  import { ArrowDown, ArrowUp, Copy, Check } from '@lucide/svelte';
  import { scrollFocusable } from '#lib/actions/scrollFocusable.js';
  import { toCsv, sortRows } from './generativeUi';
  import { renderChatMarkdown } from './markdown';
  let { headers, rows, sortable = false, markdown = false }: { headers: string[]; rows: string[][]; sortable?: boolean; markdown?: boolean } = $props();
  let column = $state(-1);
  let direction = $state(1);
  let copied = $state(false);
  let timer: ReturnType<typeof setTimeout>;
  $effect(() => () => clearTimeout(timer));
  let sorted = $derived(column < 0 ? rows : sortRows(rows, column, direction));
  function sort(i: number) { direction = column === i ? -direction : 1; column = i; }
  async function copy() {
    try {
      // Markdown is converted to plain text for CSV, never exported as HTML.
      const plain = (v: string) => {
        if (!markdown) return v;
        const el = document.createElement('div'); el.innerHTML = renderChatMarkdown(v); return el.textContent ?? '';
      };
      await navigator.clipboard.writeText(toCsv([headers, ...sorted].map(row => row.map(plain))));
      copied = true; clearTimeout(timer); timer = setTimeout(() => copied = false, 1500);
    } catch { toast.error(t('pg.rich_copy_failed')); }
  }
</script>

<div class="min-w-0 overflow-hidden rounded-lg border border-border bg-card text-sm">
  <div class="flex flex-wrap items-center justify-between gap-2 border-b border-border bg-muted/30 px-3 py-1.5">
    <span class="text-xs text-muted-foreground">{t('pg.rich_rows', { n: rows.length })}</span>
    <Button variant="ghost" size="xs" onclick={copy}>{#if copied}<Check />{:else}<Copy />{/if}{copied ? t('pg.rich_copied') : t('pg.rich_copy_csv')}</Button>
  </div>
  <div use:scrollFocusable={t('pg.rich_table')} class="max-h-96 overflow-auto focus-visible:outline-offset-[-2px]">
    <table class="w-full border-collapse text-left text-xs">
      <caption class="sr-only">{t('pg.rich_table')}</caption>
      <thead class="sticky top-0 bg-muted">
        <tr>{#each headers as header, i}<th scope="col" aria-sort={sortable ? (column === i ? direction === 1 ? 'ascending' : 'descending' : 'none') : undefined} class="min-w-28 border-b border-border px-3 py-2 align-top font-medium [overflow-wrap:anywhere]">
          {#if sortable}<button class="flex w-full items-start gap-2 rounded-sm text-left focus-visible:outline-2 focus-visible:outline-ring" onclick={() => sort(i)} aria-label={t('pg.rich_sort', { name: header })}>{header}{#if column === i}{#if direction === 1}<ArrowUp class="size-3 shrink-0" />{:else}<ArrowDown class="size-3 shrink-0" />{/if}{/if}</button>
          {:else if markdown}<div class="[&_p]:m-0">{@html renderChatMarkdown(header)}</div>{:else}{header}{/if}
        </th>{/each}</tr>
      </thead>
      <tbody>{#each sorted as row}<tr class="border-b border-border/60 last:border-0 hover:bg-muted/30">{#each headers as _, i}<td class="max-w-80 px-3 py-2.5 align-top whitespace-pre-wrap [overflow-wrap:anywhere]">{#if markdown}<div class="[&_p]:m-0 [&_a]:text-primary [&_a]:underline">{@html renderChatMarkdown(row[i] ?? '')}</div>{:else}{row[i] ?? '—'}{/if}</td>{/each}</tr>{/each}</tbody>
    </table>
    {#if !rows.length}<p class="p-4 text-sm text-muted-foreground">{t('pg.rich_empty')}</p>{/if}
  </div>
</div>
