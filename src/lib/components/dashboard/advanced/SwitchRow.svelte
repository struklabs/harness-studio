<script lang="ts">
  import { Switch } from '$lib/components/ui/switch';
  import { Label } from '$lib/components/ui/label';
  import { uiLevel } from '$lib/stores/uiLevel.svelte';
  import HelpPopover from '../../common/HelpPopover.svelte';
  import FeatureBadge from './FeatureBadge.svelte';

  interface Props {
    label: string;
    hint?: string;
    note?: string;
    help?: string;
    flag?: string;
    advanced?: boolean;
    checked: boolean;
    id: string;
    /** Capability badge next to the label (feature id). */
    feature?: string;
  }

  let { label, hint, note, help, flag, advanced = false, checked = $bindable(), id, feature }: Props = $props();
  let description = $derived([hint, help, note].filter(Boolean).join('\n\n'));
  let popover = $derived([uiLevel.advanced ? hint : undefined, help].filter(Boolean).join('\n\n'));
</script>

{#if !advanced || uiLevel.advanced}
  <div class="flex min-w-0 items-start justify-between gap-3">
    <div class="flex min-w-0 flex-col gap-0.5">
      <span class="flex flex-wrap items-center gap-1.5">
        <Label for={id} class="text-sm font-medium text-foreground">{label}</Label>
        {#if popover}<HelpPopover text={popover} {label} />{/if}
        {#if flag && uiLevel.advanced}<span class="font-mono text-xs text-muted-foreground">{flag}</span>{/if}
        <FeatureBadge {feature} />
      </span>
      {#if hint && !uiLevel.advanced}<p class="text-xs leading-snug text-muted-foreground">{hint}</p>{/if}
      {#if note}<p class="text-xs leading-snug text-muted-foreground">{note}</p>{/if}
      {#if description}<span id="{id}-hint" class="sr-only" data-field-help-text>{description}</span>{/if}
    </div>
    <Switch {id} size="sm" bind:checked aria-describedby={description ? `${id}-hint` : undefined} class="mt-0.5 shrink-0" />
  </div>
{/if}
