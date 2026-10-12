<script lang="ts">
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import { Info } from '@lucide/svelte';
  import { t } from '#lib/i18n.svelte.js';
  let { text, label = '' }: { text: string; label?: string } = $props();
  const uid = $props.id();
</script>
<Tooltip.Root>
  <Tooltip.Trigger type="button" data-help-trigger="info" aria-describedby={uid} class="inline-flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" aria-label={`${t('help.info')}${label ? ': ' + label : ''}`}>
    <Info class="size-3.5" aria-hidden="true" />
  </Tooltip.Trigger>
  <Tooltip.Content class="max-w-80 text-xs">{text}</Tooltip.Content>
</Tooltip.Root>
<span id={uid} class="sr-only" data-field-help-text>{text}</span>
