<script lang="ts">
  import type { SlotInfo } from '$lib/types';
  import { t } from '$lib/i18n.svelte';
  import { fmtCount } from './perfFormat';

  interface Props {
    slots: SlotInfo[];
  }

  let { slots }: Props = $props();
</script>

{#if slots.length > 0}
  <div class="flex flex-wrap gap-2" aria-label={t('perf.section_slots')}>
    {#each slots as slot (slot.id)}
      <div
        class="flex items-center gap-2 rounded-md border px-2.5 py-1 text-xs {slot.busy
          ? 'border-primary/40 bg-primary/5 text-foreground'
          : 'border-border text-muted-foreground'}"
      >
        <span class="size-1.5 rounded-full {slot.busy ? 'bg-primary' : 'bg-muted-foreground/40'}"></span>
        <span class="font-medium">{t('perf.slot_n', { n: slot.id })}</span>
        <span class="font-num">
          {slot.busy
            ? t('perf.slot_busy', { prompt: fmtCount(slot.prompt_tokens), gen: fmtCount(slot.decoded_tokens) })
            : t('perf.slot_idle')}
        </span>
      </div>
    {/each}
  </div>
{/if}
