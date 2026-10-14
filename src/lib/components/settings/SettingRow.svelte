<script lang="ts">
  import type { Snippet } from 'svelte';
  import { cn } from '#lib/utils.js';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import FieldHelp from '../common/FieldHelp.svelte';
  import { describeFields } from '../common/fieldHelp';
  import HelpPopover from '../common/HelpPopover.svelte';

  interface Props {
    label: string;
    hint?: string;
    note?: string;
    help?: string;
    flag?: string;
    advanced?: boolean;
    /** Id of the control this row labels (for click-to-focus). */
    for?: string;
    dirty?: boolean;
    class?: string;
    children?: Snippet;
    /** Content under the label/control line (validation results, status). */
    extra?: Snippet;
  }

  let { label, hint, note, help, flag, advanced = false, for: forId, dirty = false, class: className, children, extra }: Props = $props();
  const uid = $props.id();
  let description = $derived([hint, help, note].filter(Boolean).join('\n\n'));
  let popover = $derived([uiLevel.advanced ? hint : undefined, help].filter(Boolean).join('\n\n'));
</script>

{#if !advanced || uiLevel.advanced}
  <div data-setting-field data-help-scope class={cn('flex min-w-0 flex-col gap-2 px-4 py-3', className)} use:describeFields={description ? uid : undefined}>
    <div class="flex min-w-0 flex-wrap items-center justify-between gap-x-6 gap-y-2">
      <div class="flex min-w-0 flex-1 flex-col gap-0.5">
        <label for={forId} class="flex min-w-0 items-center gap-1.5 text-sm font-medium">
          <span class="min-w-0 break-words">{label}</span>
          {#if popover}<HelpPopover text={popover} {label} />{/if}
          {#if flag && uiLevel.advanced}<span class="font-mono text-xs text-muted-foreground">{flag}</span>{/if}
          {#if dirty}<span class="size-1.5 shrink-0 rounded-full bg-primary" aria-hidden="true"></span>{/if}
        </label>
        {#if hint && !uiLevel.advanced}<FieldHelp text={hint} />{/if}
        {#if note}<p class="text-xs leading-snug text-muted-foreground">{note}</p>{/if}
        {#if description}<span id={uid} class="sr-only" data-field-help-text>{description}</span>{/if}
      </div>
      {#if children}
        <div class="flex min-w-0 max-w-full flex-wrap items-center gap-2">{@render children()}</div>
      {/if}
    </div>
    {#if extra}{@render extra()}{/if}
  </div>
{/if}
