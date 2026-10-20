<script lang="ts">
  import '../app.css';
  import '../styles/global.css';
  import { onMount } from 'svelte';
  import BrowserManager from '#lib/components/BrowserManager.svelte';
  import { isDesktop } from '#lib/transport.js';

  let { children } = $props();
  let ready = $state(false);
  onMount(() => { ready = true; });
</script>

{#if ready}
  {#if isDesktop()}
    {@render children?.()}
  {:else}
    <BrowserManager />
  {/if}
{/if}
