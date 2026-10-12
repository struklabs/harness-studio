<script lang="ts">
  import type { Snippet } from 'svelte';
  import { uiLevel } from '$lib/stores/uiLevel.svelte';
  let { children }: { children: Snippet } = $props();
</script>

{#if uiLevel.advanced}{@render children()}{/if}
