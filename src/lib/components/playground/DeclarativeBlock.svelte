<script lang="ts">
  import type { UiBlock } from './generativeUi';
  import { t } from '#lib/i18n.svelte.js';
  import { Progress } from '#lib/components/ui/progress/index.js';
  let { block }: { block: UiBlock } = $props();
</script>

<section class="min-w-0 space-y-3 rounded-lg border border-border bg-card p-4 text-sm [overflow-wrap:anywhere]" class:border-l-4={block.type === 'callout'}>
  <h3 class="font-semibold text-foreground">{block.title}</h3>
  {#if block.type === 'card' || block.type === 'callout'}<p class="whitespace-pre-wrap leading-relaxed text-muted-foreground">{block.text}</p>
  {:else if block.type === 'list' || block.type === 'steps'}
    {#if !block.items.length}<p class="text-muted-foreground">{t('pg.rich_empty')}</p>{/if}
    {#if block.type === 'steps'}<ol class="list-decimal space-y-2 pl-5 marker:text-muted-foreground">{#each block.items as item}<li class="pl-1 whitespace-pre-wrap">{item}</li>{/each}</ol>
    {:else}<ul class="list-disc space-y-2 pl-5 marker:text-muted-foreground">{#each block.items as item}<li class="pl-1 whitespace-pre-wrap">{item}</li>{/each}</ul>{/if}
  {:else if block.type === 'progress'}
    <div class="flex items-center gap-3"><Progress class="h-2 min-w-0 flex-1" max={100} value={block.value} aria-label={block.title} /><span class="font-num text-xs text-muted-foreground">{block.value}%</span></div>
  {:else if block.type === 'key-value'}
    <dl class="divide-y divide-border">{#each Object.entries(block.entries) as [key, value]}<div class="grid grid-cols-[minmax(0,1fr)_minmax(0,2fr)] gap-3 py-2"><dt class="text-muted-foreground">{key}</dt><dd class="whitespace-pre-wrap">{value}</dd></div>{/each}</dl>
    {#if !Object.keys(block.entries).length}<p class="text-muted-foreground">{t('pg.rich_empty')}</p>{/if}
  {/if}
</section>
