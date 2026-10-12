<script lang="ts">
  // "All" plus one chip per server: which servers a panel shows. A chip carries the server's
  // colour (the same one its card, log tag and memory segments use).
  import { app } from '$lib/stores/app.svelte';
  import type { ServerFilter } from '$lib/stores/serverFilter.svelte';
  import { slotColor, slotHatched } from '$lib/serverColor';
  import { t } from '$lib/i18n.svelte';
  import { cn } from '$lib/utils';

  interface Props {
    filter: ServerFilter;
    /** Servers the chips are for, in display order. */
    ids: string[];
    /** Shown after the name (a stopped server's state), by id. */
    hint?: (id: string) => string | null;
    label: string;
    class?: string;
  }

  let { filter, ids, hint, label, class: className }: Props = $props();

  const chip =
    'inline-flex h-6 max-w-44 items-center gap-1.5 rounded-full border px-2 text-xs transition-colors focus-visible:outline-2 focus-visible:outline-ring';
  const on = 'border-primary/50 bg-primary/10 text-foreground';
  const off = 'border-border bg-transparent text-muted-foreground hover:bg-accent hover:text-foreground';
</script>

<div class={cn('flex flex-wrap items-center gap-1.5', className)} role="group" aria-label={label}>
  <button type="button" class={cn(chip, filter.isAll ? on : off)} aria-pressed={filter.isAll} onclick={() => filter.all()}>
    {t('dash.srv_all')}
  </button>
  {#each ids as id (id)}
    {@const slot = app.serverSlots[id] ?? 0}
    {@const pressed = filter.isChosen(id, ids)}
    <button type="button" class={cn(chip, pressed ? on : off)} aria-pressed={pressed} onclick={() => filter.toggle(id, ids)} title={app.serverLabel(id)}>
      <span
        class={cn('size-2 shrink-0 rounded-full', slotHatched(slot) && 'hatch')}
        style="background-color: {slotColor(slot)}"
        aria-hidden="true"
      ></span>
      <span class="truncate">{app.serverLabel(id)}</span>
      {#if hint?.(id)}<span class="shrink-0 text-muted-foreground">{hint(id)}</span>{/if}
    </button>
  {/each}
</div>
