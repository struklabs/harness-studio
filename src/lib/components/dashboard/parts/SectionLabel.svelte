<script lang="ts">
  import type { Snippet } from 'svelte';
  import { cn } from '$lib/utils';

  let { class: className, children }: { class?: string; children: Snippet } = $props();
</script>

<h3 class={cn('text-sm font-semibold text-foreground', className)}>
  {@render children()}
</h3>
