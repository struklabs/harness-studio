<script lang="ts">
  import type { Snippet } from 'svelte';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { t } from '#lib/i18n.svelte.js';
  let { children, collapsible = false, invalid = false }: { children: Snippet; collapsible?: boolean; invalid?: boolean } = $props();
</script>

{#if uiLevel.advanced}
  {@render children()}
{:else if collapsible}
  <details open={invalid} class="min-w-0 rounded-lg border border-border p-3">
    <summary class="cursor-pointer rounded-sm text-sm font-medium outline-none focus-visible:ring-2 focus-visible:ring-ring">{t('uiLevel.more')}</summary>
    <div class="mt-3 flex min-w-0 flex-col gap-3">{@render children()}</div>
  </details>
{/if}
