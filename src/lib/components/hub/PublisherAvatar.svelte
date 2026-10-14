<script lang="ts">
  import { avatars } from '#lib/stores/avatars.svelte.js';
  import { cn } from '#lib/utils.js';

  interface Props {
    /** Hugging Face user / organization name. */
    name: string;
    class?: string;
  }

  let { name, class: className }: Props = $props();

  let url = $derived(avatars.get(name));
  let broken = $state(false);
  $effect(() => {
    void url;
    broken = false;
  });
  $effect(() => {
    if (name) avatars.request([name]);
  });

  /** Two initials: first letters of the first two words ("meta-llama" -> "ML"), else the first two characters. */
  let initials = $derived.by(() => {
    const parts = name.split(/[^a-zA-Z0-9]+/).filter(Boolean);
    const s = parts.length >= 2 ? parts[0][0] + parts[1][0] : (parts[0] ?? name).slice(0, 2);
    return s.toUpperCase();
  });
  /** Stable 1..5 chart-token index from the name, so a publisher always gets the same color. */
  let tone = $derived.by(() => {
    let h = 0;
    for (const ch of name.toLowerCase()) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
    return (h % 5) + 1;
  });
</script>

{#if url && !broken}
  <img
    src={url}
    alt=""
    loading="lazy"
    decoding="async"
    referrerpolicy="no-referrer"
    class={cn('size-8 shrink-0 rounded-md border border-border bg-muted object-cover', className)}
    onerror={() => (broken = true)}
  />
{:else}
  <span
    aria-hidden="true"
    class={cn('flex size-8 shrink-0 items-center justify-center rounded-md border border-border text-xs font-semibold', className)}
    style="background: color-mix(in oklab, var(--chart-{tone}) 16%, transparent); color: var(--chart-{tone});"
    >{initials}</span
  >
{/if}
