<script lang="ts">
  import * as Tooltip from '$lib/components/ui/tooltip';
  import { t, type TKey } from '$lib/i18n.svelte';
  import { capsStore } from '$lib/stores/caps.svelte';
  import { uiLevel } from '$lib/stores/uiLevel.svelte';
  import { cn } from '$lib/utils';

  interface Props {
    /** Feature id (capabilities::features); nothing is shown while its status is unknown. */
    feature: string | null | undefined;
  }

  let { feature }: Props = $props();

  let status = $derived(feature ? capsStore.features[feature] : undefined);
  let reason = $derived(status ? t(status.reason.key as TKey, status.reason.params) : '');
  let tone = $derived.by(() => {
    if (!status) return '';
    if (status.level === 'endpoint_verified') return 'border-border text-muted-foreground';
    if (status.available === false) {
      return 'border-border text-muted-foreground';
    }
    return 'border-border text-muted-foreground';
  });
</script>

{#if status && uiLevel.advanced}
  <Tooltip.Root>
    <Tooltip.Trigger
      class={cn(
        'inline-flex h-4 shrink-0 items-center rounded-full border px-1.5 text-xs leading-none font-medium whitespace-nowrap outline-none focus-visible:ring-2 focus-visible:ring-ring/50',
        tone
      )}
      aria-label={`${t(`caps.level.${status.level}` as TKey)}: ${reason}`}
    >
      {t(`caps.level.${status.level}` as TKey)}
    </Tooltip.Trigger>
    <Tooltip.Content class="max-w-64">{reason}</Tooltip.Content>
  </Tooltip.Root>
{/if}
