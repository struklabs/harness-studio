<script lang="ts">
  import type { ModelRecord, ModelCaps } from '#lib/types.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { taskSupportStore } from '../hub/taskSupport.svelte';
  import { engineLabel } from '#lib/engines.js';
  let { record, caps, task }: { record?: Partial<ModelRecord> | null; caps?: ModelCaps; task?: string | null } = $props();
  const kinds = ['chat', 'vision', 'audio', 'image_generation', 'asr', 'tts', 'video', 'embedding', 'rerank', 'tools'] as const;
  $effect(() => { void taskSupportStore.load(); });
  // The task table already records partial or missing features of llama.cpp builds.
  let runtimeTask = $derived(record?.runtime === 'llama.cpp' ? taskSupportStore.info(task) : null);
  let blocked = $derived(record?.compatibility?.status === 'unsupported' || record?.compatibility?.status === 'unsupported_platform'
    || runtimeTask?.baseline === 'no' || runtimeTask?.runtime_missing === true);
  let experimental = $derived(record?.runtime === 'audio.cpp' || has('video') || runtimeTask?.baseline === 'partial');
  let supported = $derived(new Set<string>(record?.capabilities ?? []));
  function has(kind: string) {
    if (kind === 'video') return supported.has('video') || !!task?.includes('video');
    if (kind === 'tools') return caps?.verified?.tools ?? caps?.tools ?? false;
    return supported.has(kind);
  }
</script>

<div class="flex flex-wrap gap-1 text-xs" aria-label={t('lib.caps')}>
  {#each kinds.filter(k => uiLevel.advanced || has(k)) as kind}
    <span class="rounded border border-border px-1.5 py-0.5 text-muted-foreground" title={t('w4.source')}>
      {t(`w4.${kind}` as TKey)}{#if uiLevel.advanced}: {t(blocked && has(kind) ? 'w4.unsupported' : has(kind) ? 'w4.supported' : caps ? 'w4.not_supported' : 'w4.unknown')}{/if}
    </span>
  {/each}
  {#if !uiLevel.advanced && !caps}<span class="text-muted-foreground">{t('w4.tools')}: {t('w4.unknown')}</span>{/if}
  {#if uiLevel.advanced && caps?.tools && !caps.verified}<span class="text-warning">{t('lib.caps_template')}</span>{/if}
  {#if blocked}<span class="text-destructive">{t('w4.unsupported')}</span>{/if}
  {#if experimental}<span class="text-warning">{t('w4.experimental')}</span>{/if}
  {#if record?.compatibility?.status === 'needs_runtime'}<span class="text-warning">{t('w4.needs_runtime')}</span>{/if}
  {#if record?.runtime === 'python-hf' && has('chat')}<span class="text-muted-foreground" title={t('w4.source')}>{engineLabel('python-hf')}</span>{/if}
  {#if uiLevel.advanced && record?.runtime}<span class="text-muted-foreground">{record.runtime} · {t('w4.source')}</span>{/if}
</div>
