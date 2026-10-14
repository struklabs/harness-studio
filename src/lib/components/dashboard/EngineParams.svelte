<script lang="ts">
  // Settings of a model that another engine than llama.cpp runs (ONNX, whisper.cpp, sd.cpp,
  // python-hf, MLX...): every group of that engine's own catalog, rendered from the catalog.
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { engineLabel } from '#lib/engines.js';
  import { capsStore } from '#lib/stores/caps.svelte.js';
  import type { LaunchParamsForm } from './params';
  import AdvSection from './advanced/AdvSection.svelte';
  import CatalogFields from './advanced/CatalogFields.svelte';

  interface Props {
    params: LaunchParamsForm;
  }

  let { params = $bindable() }: Props = $props();

  const KNOWN = ['performance', 'model', 'transcription', 'vad', 'components', 'generation', 'server', 'context', 'memory'];
  let engine = $derived(engineLabel(capsStore.engine));
  let groups = $derived(capsStore.groups().filter((g) => capsStore.groupSpecs(g).length > 0));

  function title(group: string): string {
    return KNOWN.includes(group) ? t(`dash.egroup_${group}` as TKey) : group;
  }
</script>

<div class="flex flex-col gap-3">
  <p class="text-xs text-muted-foreground">{t('dash.engine_settings_help', { engine })}</p>
  {#each groups as group (group)}
    <AdvSection open={true} title={title(group)}>
      <CatalogFields bind:params {group} />
    </AdvSection>
  {:else}
    <p class="text-xs text-muted-foreground">{t('dash.engine_none', { engine })}</p>
  {/each}
</div>
