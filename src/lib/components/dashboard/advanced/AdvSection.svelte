<script lang="ts">
  import type { Snippet } from 'svelte';
  import * as Collapsible from '#lib/components/ui/collapsible/index.js';
  import { ChevronRight } from '@lucide/svelte';
  import { cn } from '#lib/utils.js';

  interface Props {
    title: string;
    /** Small status text on the right (e.g. a warning marker). */
    badge?: string;
    badgeTone?: 'warning' | 'destructive';
    open?: boolean;
    children: Snippet;
  }

  let { title, badge, badgeTone = 'warning', open = $bindable(false), children }: Props = $props();
</script>

<Collapsible.Root bind:open class="border-b border-border last:border-b-0">
  <Collapsible.Trigger
    class="group flex w-full items-center gap-1.5 rounded py-2 text-left text-xs font-medium text-foreground/90 outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
  >
    <ChevronRight class={cn('size-3.5 shrink-0 text-muted-foreground transition-transform duration-150', open && 'rotate-90')} />
    <span class="min-w-0 flex-1 truncate">{title}</span>
    {#if badge}
      <span class={cn('shrink-0 text-xs font-medium', badgeTone === 'destructive' ? 'text-destructive' : 'text-warning')}>{badge}</span>
    {/if}
  </Collapsible.Trigger>
  <Collapsible.Content class="flex flex-col gap-3 pb-3 pl-5">
    {@render children()}
  </Collapsible.Content>
</Collapsible.Root>
