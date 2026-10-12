<script lang="ts">
  import { NA } from './perfFormat';

  interface Props {
    label: string;
    /** `null`/`undefined` renders as "not available". */
    value: string | null | undefined;
    hint?: string;
    /** Leave the row out when the value is not reported (hardware without that sensor). */
    hideMissing?: boolean;
  }

  let { label, value, hint, hideMissing = false }: Props = $props();
  let missing = $derived(value == null || value === NA);
</script>

{#if !(hideMissing && missing)}

<div class="flex items-baseline justify-between gap-3 py-1.5 text-sm" title={hint}>
  <span class="text-muted-foreground truncate">{label}</span>
  <span class="font-num shrink-0 {missing ? 'text-muted-foreground' : 'text-foreground'}">
    {missing ? NA : value}
  </span>
</div>
{/if}
