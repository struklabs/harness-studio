<script lang="ts">
  import type { GgufMetadata } from '#lib/types.js';
  import { invoke } from '#lib/ipc.js';
  import { t } from '#lib/i18n.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { capsStore } from '#lib/stores/caps.svelte.js';
  import type { LaunchParamsForm } from './params';
  import { applyMetadataOverride, metadataOverrideApplied } from './metadataOverrides';

  let { modelPath, params = $bindable(), onApply, active = true }: { modelPath: string | null; params: LaunchParamsForm; onApply: () => void; active?: boolean } = $props();
  let fix = $state<string | null>(null);
  $effect(() => {
    if (!active) return;
    const path = modelPath;
    fix = null;
    if (!path) return;
    let stale = false;
    invoke<GgufMetadata>('get_model_metadata', { path }).then((metadata) => {
      if (!stale) fix = metadata.known_metadata_fixes?.override_kv ?? null;
    }).catch(() => {});
    return () => { stale = true; };
  });
  let applied = $derived(fix !== null && metadataOverrideApplied(params.catalog_params.override_kv, fix));
  function apply() {
    if (!fix) return;
    params.catalog_params.override_kv = applyMetadataOverride(params.catalog_params.override_kv, fix);
    onApply();
  }
</script>

{#if fix}
  <div class="rounded-md border border-warning/30 bg-warning/10 px-2.5 py-2 text-xs" role="status">
    <p>{t('dash.metadata_fix_notice')}</p>
    <p class="mt-1 text-muted-foreground">{t('dash.metadata_fix_automatic')}</p>
    <Button variant="outline" size="sm" class="mt-2" disabled={applied || !capsStore.supports('override_kv')} onclick={apply}>
      {applied ? t('dash.metadata_fix_applied') : t('dash.metadata_fix_apply')}
    </Button>
  </div>
{/if}
