<script lang="ts">
  import type { Snippet } from 'svelte';
  import HelpPopover from '../../common/HelpPopover.svelte';
  import FieldHelp from '../../common/FieldHelp.svelte';
  import { describeFields } from '../../common/fieldHelp';
  import { uiLevel } from '$lib/stores/uiLevel.svelte';
  import { Label } from '$lib/components/ui/label';
  import { cn } from '$lib/utils';

  interface Props {
    label: string;
    /** id of the control this label points to. */
    id?: string;
    help?: string;
    hint?: string;
    note?: string;
    flag?: string;
    advanced?: boolean;
    /** Content on the right side of the label row (value readout, badge). */
    aside?: Snippet;
    class?: string;
    children: Snippet;
  }

  let { label, id, help, hint, note, flag, advanced = false, aside, class: className, children }: Props = $props();
  const uid = $props.id();
  let descriptionId = $derived(id ? id + '-help' : uid);
  let description = $derived([hint, help, note].filter(Boolean).join('\n\n'));
  let popover = $derived([uiLevel.advanced ? hint : undefined, help].filter(Boolean).join('\n\n'));
</script>

{#if !advanced || uiLevel.advanced}
  <div data-help-scope class={cn('flex min-w-0 flex-col gap-1.5', className)} use:describeFields={description ? descriptionId : undefined}>
    <div class="flex min-h-4 flex-wrap items-center justify-between gap-2">
      <span class="flex min-w-0 max-w-full flex-wrap items-center gap-1">
        <Label for={id} class="min-w-0 text-sm font-medium text-foreground">{label}</Label>
        {#if popover}<HelpPopover text={popover} {label} />{/if}
        {#if flag && uiLevel.advanced}<span class="min-w-0 break-all font-mono text-xs text-muted-foreground">{flag}</span>{/if}
      </span>
      {@render aside?.()}
    </div>
    {@render children()}
    {#if hint && !uiLevel.advanced}<FieldHelp text={hint} />{/if}
    {#if note}<p class="text-xs leading-snug text-muted-foreground">{note}</p>{/if}
    {#if description}<span id={descriptionId} class="sr-only" data-field-help-text>{description}</span>{/if}
  </div>
{/if}
