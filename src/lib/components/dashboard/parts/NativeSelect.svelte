<script lang="ts">
  import type { HTMLSelectAttributes } from 'svelte/elements';
  import { cn } from '#lib/utils.js';

  let { value = $bindable(), class: className, children, ...rest }: HTMLSelectAttributes = $props();
</script>

<select
  bind:value
  data-slot="native-select"
  class={cn(
    'h-8 w-full min-w-0 rounded-lg border border-input bg-transparent px-2 text-sm outline-none transition-colors focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-input/30 [&>option]:bg-popover [&>option]:text-popover-foreground',
    className
  )}
  {...rest}
>
  {@render children?.()}
</select>
