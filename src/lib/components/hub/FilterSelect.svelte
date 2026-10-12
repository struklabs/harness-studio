<script lang="ts">
  import * as Select from '$lib/components/ui/select';
  import { cn } from '$lib/utils';
  import FieldHelp from '../common/FieldHelp.svelte';

  interface Props {
    label: string;
    value: string;
    options: { value: string; label: string }[];
    onchange?: (value: string) => void;
    class?: string;
    /** Show the caption above the trigger (otherwise it is only the aria-label). */
    showLabel?: boolean;
    help: string;
  }

  let { label, help, value = $bindable(), options, onchange, class: className, showLabel = true }: Props = $props();

  let current = $derived(options.find((o) => o.value === value)?.label ?? '');
</script>

<div class={cn('flex min-w-0 flex-col gap-1', className)}>
  {#if showLabel}
    <span class="text-xs text-muted-foreground">{label}</span>
  {/if}
  <FieldHelp text={help} {label} class="w-full"><Select.Root
    type="single"
    {value}
    onValueChange={(v) => {
      value = v;
      onchange?.(v);
    }}
  >
    <Select.Trigger size="sm" class="w-full min-w-0" aria-label={label}>
      <span class="truncate">{current}</span>
    </Select.Trigger>
    <Select.Content>
      {#each options as o (o.value)}
        <Select.Item value={o.value} label={o.label} />
      {/each}
    </Select.Content>
  </Select.Root></FieldHelp>
</div>
