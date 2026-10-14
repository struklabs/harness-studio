<script lang="ts">
  import { Button } from '#lib/components/ui/button/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import { Plus, X } from '@lucide/svelte';

  interface Props {
    items: string[];
    /** Placeholder per box; `{n}` is replaced by the 1-based number. */
    placeholder: (n: number) => string;
    addLabel: string;
    removeLabel: string;
    disabled?: boolean;
    /** Upper bound of boxes (keeps a single request reasonable). */
    max?: number;
  }

  let { items = $bindable(), placeholder, addLabel, removeLabel, disabled = false, max = 16 }: Props = $props();
</script>

<div class="flex flex-col gap-2">
  {#each items as _, i (i)}
    <div class="flex items-start gap-2">
      <span class="mt-2 w-5 shrink-0 text-right font-num text-xs text-muted-foreground">{i + 1}</span>
      <Textarea
        bind:value={items[i]}
        rows={2}
        placeholder={placeholder(i + 1)}
        aria-label={placeholder(i + 1)}
        {disabled}
        class="min-h-0 min-w-0 flex-1 resize-y text-sm"
      />
      <Button
        variant="ghost"
        size="icon-sm"
        class="mt-1 shrink-0"
        aria-label={`${removeLabel} ${i + 1}`}
        title={removeLabel}
        disabled={disabled || items.length <= 1}
        onclick={() => items.splice(i, 1)}
      >
        <X />
      </Button>
    </div>
  {/each}
  <div class="pl-7">
    <Button variant="outline" size="sm" disabled={disabled || items.length >= max} onclick={() => items.push('')}>
      <Plus />
      {addLabel}
    </Button>
  </div>
</div>
