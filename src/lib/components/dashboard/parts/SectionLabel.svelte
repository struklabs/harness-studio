<script lang="ts">
  import type { Snippet } from 'svelte';
  import { cn } from '#lib/utils.js';

  let { class: className, children }: { class?: string; children: Snippet } = $props();
</script>

<h2 class={cn('text-sm font-semibold text-foreground', className)}>
  {@render children()}
</h2>
