<script lang="ts">
  import * as Popover from '$lib/components/ui/popover';
  import { CircleHelp } from '@lucide/svelte';
  import { t } from '$lib/i18n.svelte';
  let { text, label = '', href }: { text: string; label?: string; href?: string } = $props();
  const uid = $props.id();
</script>
<Popover.Root>
  <Popover.Trigger type="button" data-help-trigger="help" aria-label={`${t('help.how')}${label ? ': ' + label : ''}`} aria-describedby={uid} class="inline-flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
    <CircleHelp class="size-3.5" aria-hidden="true" />
  </Popover.Trigger>
  <span id={uid} class="sr-only" data-field-help-text>{text}</span>
  <Popover.Content align="end" class="max-w-[calc(100vw-2rem)] text-xs leading-relaxed">
    {#if label}<p class="font-medium">{label}</p>{/if}
    <p class="whitespace-pre-line">{text}</p>
    {#if href}<a {href} class="text-primary underline underline-offset-4" target="_blank" rel="noreferrer">{t('help.more')}</a>{/if}
  </Popover.Content>
</Popover.Root>
