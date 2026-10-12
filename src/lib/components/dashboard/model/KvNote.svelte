<script lang="ts">
  // Plain-language note for an automatic conversation-memory (KV cache) downgrade
  // (`harness_core::kv_policy`); the technical flags are in the tooltip.
  import type { KvChoice } from '#lib/types.js';
  import { formatBytes } from '#lib/format.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import AdvancedOnly from '../../common/AdvancedOnly.svelte';
  import { MemoryStick } from '@lucide/svelte';

  interface Props {
    kv: KvChoice;
  }

  let { kv }: Props = $props();

  const NOTE: Partial<Record<KvChoice['level'], TKey>> = {
    keys_only: 'kv.note_keys_only',
    balanced: 'kv.note_balanced',
    saver: 'kv.note_saver',
    compact: 'kv.note_compact',
  };
  let note = $derived(NOTE[kv.level]);
  let speed = $derived(
    kv.prompt_slowdown_pct === null ? '' : kv.prompt_slowdown_pct <= 2 ? t('kv.note_speed_same') : t('kv.note_speed', { pct: kv.prompt_slowdown_pct }),
  );
  let flags = $derived(`-ctk ${kv.cache_type_k} -ctv ${kv.cache_type_v}`);
</script>

{#if note && (kv.reason === 'to_fit' || kv.reason === 'tightest')}
  <Tooltip.Root>
    <Tooltip.Trigger class="flex items-start gap-1.5 rounded text-left text-xs text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none">
      <MemoryStick class="mt-0.5 size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
      <span>
        {t(note)}
        {speed}
        {#if kv.reason === 'tightest'}<span class="text-warning">{t('kv.note_tightest')}</span>{/if}
      </span>
    </Tooltip.Trigger>
    <Tooltip.Content class="max-w-64 flex-col items-start">
      <AdvancedOnly><span class="font-mono text-xs text-muted-foreground">{flags}</span></AdvancedOnly>
      <span>{t('kv.note_saved', { from: formatBytes(kv.kv_bytes_f16), to: formatBytes(kv.kv_bytes) })}</span>
    </Tooltip.Content>
  </Tooltip.Root>
{/if}
