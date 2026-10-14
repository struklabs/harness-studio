<script lang="ts">
  import { describeFields } from '../common/fieldHelp';
  import FieldHelp from '../common/FieldHelp.svelte';
  import HelpPopover from '../common/HelpPopover.svelte';
  import type { Snippet } from 'svelte';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { cn } from '#lib/utils.js';

  interface Props {
    title: string;
    advanced?: boolean;
    showTitle?: boolean;
    description?: string;
    hint?: string;
    note?: string;
    danger?: boolean;
    actions?: Snippet;
    children: Snippet;
  }

  const uid = $props.id();
  let { advanced = false, title, showTitle = true, description, hint, note, danger = false, actions, children }: Props = $props();
</script>

{#if !advanced || uiLevel.advanced}
  <section data-help-scope class="flex min-w-0 flex-col gap-2" aria-describedby={hint ? uid : undefined} use:describeFields={hint ? uid : undefined}>
    <div class="flex items-end justify-between gap-3">
      <div class="flex min-w-0 flex-col gap-0.5">
        {#if showTitle}<h3
          class={cn(
            'flex items-center gap-1.5 text-sm font-semibold',
            danger ? 'text-destructive' : 'text-foreground',
          )}
        >
          {title}
          {#if hint && uiLevel.advanced}<HelpPopover text={hint} label={title} />{/if}
        </h3>{/if}
        <!-- Without a visible title the hint is written out: a lone "?" above the first row has no label to belong to. -->
        {#if hint && !showTitle}<p class="text-xs text-muted-foreground">{hint}</p>
        {:else if hint && !uiLevel.advanced}<div class="flex items-center gap-1.5"><FieldHelp text={hint} /></div>{/if}
        {#if hint}<span id={uid} class="sr-only" data-field-help-text>{hint}</span>{/if}
        {#if description}<p class="text-xs text-muted-foreground">{description}</p>{/if}
        {#if note}<p class="text-xs text-muted-foreground">{note}</p>{/if}
      </div>
      {#if actions}<div class="flex shrink-0 items-center gap-2">{@render actions()}</div>{/if}
    </div>
    <div
      class={cn(
        'divide-y divide-border overflow-hidden rounded-lg border bg-card',
        danger ? 'border-destructive/30' : 'border-border',
      )}
    >
      {@render children()}
    </div>
  </section>
{/if}
