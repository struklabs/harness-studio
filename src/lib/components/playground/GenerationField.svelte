<script lang="ts">
  import type { Snippet } from 'svelte';
  import HelpPopover from '../common/HelpPopover.svelte';
  import { describeFields } from '../common/fieldHelp';
  import { Label } from '#lib/components/ui/label/index.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  let { label, id, help, note, children, class: className = '' }: { label: string; id?: string; help: TKey; note?: string; children: Snippet; class?: string } = $props();
  const uid = $props.id();
</script>
<div data-help-scope class={`flex min-w-0 flex-col gap-1.5 ${className}`} use:describeFields={uid}>
  <div class="flex min-h-7 items-center gap-1"><Label for={id} class="text-sm font-medium">{label}</Label><HelpPopover text={t(help)} {label} info /></div>
  {@render children()}
  <p id={uid} class="text-xs leading-snug text-muted-foreground">{t(`${help}_hint` as TKey)}</p>
  {#if note && note !== t(help)}<p class="text-xs leading-snug text-muted-foreground">{note}</p>{/if}
</div>
