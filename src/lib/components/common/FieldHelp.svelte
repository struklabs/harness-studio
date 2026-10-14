<script lang="ts">
  import type { Snippet } from 'svelte';
  import HelpPopover from './HelpPopover.svelte';
  import { describeFields } from './fieldHelp';
  import { cn } from '#lib/utils.js';
  let { text, id, label, children, class: className }: { text: string; id?: string; label?: string; children?: Snippet; class?: string } = $props();
  const uid = $props.id();
  let descriptionId = $derived(id ?? uid);
</script>
{#if children}
  <span data-help-scope class={cn('field-help inline-flex min-w-0 items-center gap-1', className)} use:describeFields={descriptionId}>
    <span class="min-w-0 flex-1">{@render children()}</span>
    <HelpPopover {text} {label} />
    <span id={descriptionId} class="sr-only">{text}</span>
  </span>
{:else}
  <p id={descriptionId} data-field-help-text class="text-xs leading-snug text-muted-foreground">{text}</p>
{/if}
