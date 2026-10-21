<script lang="ts">
  import { t } from '#lib/i18n.svelte.js';
  import { connections } from '../connections.svelte.ts';

  let { selected = $bindable() }: { selected: string } = $props();
</script>

{#if connections.harness.length > 1}
  <select aria-label={t('mobile.conn.harness')} class="h-9 w-full rounded-md border border-input bg-background px-2 text-sm" bind:value={selected}>
    {#each connections.harness as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
  </select>
{/if}
