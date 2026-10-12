<script lang="ts">
  import Field from '../dashboard/parts/Field.svelte';
  // Composer action: compact the conversation now, with an optional instruction for the summary.
  import { t } from '$lib/i18n.svelte';
  import { Button } from '$lib/components/ui/button';
  import { Textarea } from '$lib/components/ui/textarea';
  import * as Popover from '$lib/components/ui/popover';
  import { FoldVertical } from '@lucide/svelte';

  interface Props {
    /** Nothing to compact yet / offline / answering. */
    disabled: boolean;
    /** Newest turns kept verbatim (shown in the description). */
    keepTurns: number;
    /** Context use line ("3,120 / 4,096 tokens · auto at 3,276"), when known. */
    usage?: string;
    onCompact: (instruction: string) => void;
  }

  let { disabled, keepTurns, usage, onCompact }: Props = $props();

  let open = $state(false);
  let instruction = $state('');

  function run() {
    open = false;
    onCompact(instruction.trim());
    instruction = '';
  }
</script>

<Popover.Root bind:open>
  <Popover.Trigger>
    {#snippet child({ props })}
      <Button {...props} type="button" variant="ghost" size="icon-xs" class="mr-1" aria-label={t('cmp.compact_now')} title={t('cmp.compact_now')} {disabled}>
        <FoldVertical />
      </Button>
    {/snippet}
  </Popover.Trigger>
  <Popover.Content align="start" class="flex w-80 flex-col gap-2.5">
    <span class="text-sm font-semibold">{t('cmp.compact_title')}</span>
    <p class="text-xs text-muted-foreground">{t('cmp.compact_desc', { keep: keepTurns })}</p>
    {#if usage}<p class="font-mono text-xs text-muted-foreground tabular">{usage}</p>{/if}
<Field label={t('cmp.instruction_once')} hint={t('cmp.slash_hint')}>
      <Textarea
        rows={2}
        class="min-h-12 resize-y text-xs"
        placeholder={t('cmp.instruction_placeholder')}
        bind:value={instruction}
        onkeydown={(e) => {
          if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
            e.preventDefault();
            run();
          }
        }}
      />
    </Field>
    <Button size="sm" onclick={run}>
      <FoldVertical />
      {t('cmp.compact_now')}
    </Button>
  </Popover.Content>
</Popover.Root>
