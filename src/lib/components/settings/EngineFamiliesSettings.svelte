<script lang="ts">
  // Settings > Installed Runtime > Other Engines: the engine families a model can need besides
  // llama.cpp (speech, images, voices, ONNX, Lemonade, mistral.rs), each with its install state
  // and a one-click install button. The same installer as the "needs an engine" hints.
  import { onMount } from 'svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { ENGINES } from '#lib/engines.js';
  import { engineInstall } from '#lib/stores/engineInstall.svelte.js';
  import { uiLevel } from '#lib/stores/uiLevel.svelte.js';
  import { security } from '#lib/stores/security.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { LoaderCircle, Download } from '@lucide/svelte';
  import SettingGroup from './SettingGroup.svelte';

  onMount(() => void engineInstall.refresh());

  let engines = $derived(ENGINES.filter((e) => engineInstall.offered(e.family)));
</script>

<SettingGroup title={t('eng.group_title')} hint={t('eng.group_desc')}>
  <div class="flex flex-col divide-y divide-border">
    {#each engines as e (e.family)}
      {@const st = engineInstall.state(e.family)}
      {@const installed = engineInstall.installed(e.family)}
      <div class="flex flex-col gap-1 px-4 py-3">
        <div class="flex min-w-0 items-center justify-between gap-3">
          <div class="flex min-w-0 flex-col gap-0.5">
            <span class="flex min-w-0 items-center gap-1.5 text-sm font-medium">
              <span class="truncate font-mono">{e.family}</span>
              <Badge variant={installed ? 'secondary' : 'outline'} class="text-xs">
                {installed ? t('eng.state_installed') : t('eng.state_missing')}
              </Badge>
            </span>
            <span class="text-xs text-muted-foreground">{t(e.descKey)}</span>
          </div>
          <div class="flex shrink-0 items-center gap-1.5">
            {#if !installed && engineInstall.canInstall(e.family)}
              <Button size="sm" disabled={engineInstall.anyBusy} onclick={() => engineInstall.install(e.family)}>
                {#if st.busy}<LoaderCircle class="animate-spin" />{:else}<Download />{/if}
                {st.busy ? t(st.pct === null ? 'eng.installing' : 'eng.installing_pct', { runtime: e.family, pct: st.pct ?? 0 }) : t('eng.install', { runtime: e.family })}
              </Button>
            {:else if !installed && uiLevel.advanced}
              <Button variant="outline" size="sm" onclick={() => security.openSettings('runtime-registry')}>{t('eng.open_registry')}</Button>
            {/if}
          </div>
        </div>
        {#if !installed && !engineInstall.canInstall(e.family)}
          <span class="text-xs text-muted-foreground">{t('eng.manual', { runtime: e.family })}</span>
        {/if}
        {#if st.problem === 'unverified'}
          <span class="text-xs text-warning" role="alert">{t('eng.unverified', { runtime: e.family })}</span>
        {:else if st.problem === 'failed' && st.error}
          <span class="text-xs text-destructive [overflow-wrap:anywhere]" role="alert">{t('eng.failed', { runtime: e.family, error: st.error })}</span>
        {/if}
      </div>
    {/each}
  </div>
</SettingGroup>
