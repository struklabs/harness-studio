<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { panes } from '#lib/stores/panes.svelte.js';
  import AdvancedPageNotice from './common/AdvancedPageNotice.svelte';
  import { pageLevel } from '#lib/uiLevel.js';
  import SectionSidebar from './common/SectionSidebar.svelte';
  import { CREATE_GROUPS, CREATE_PAGES } from './create/navigation';
  import ModelPicker from './create/ModelPicker.svelte';
  import AutoTools from './create/AutoTools.svelte';
  import MyTools from './create/MyTools.svelte';
  import ToolGeneratorDialog from './create/ToolGenerator.svelte';
  import type { GenerateToolRequest } from '#lib/tools-client.js';
  let active = $state('generator');
  let endpoint = $state<GenerateToolRequest['endpoint'] | null>(null);
  let selectedModel = $state('');
  let generatorOpen = $state(true);
  let revision = $state(0);
  function select(id: string) {
    if (!CREATE_PAGES.some(p => p.id === id)) return;
    active = id;
    try { localStorage.setItem('harness_create_page', id); } catch { /* unavailable */ }
    if (panes.createNav.floating) panes.createNav.close();
  }
  function fromUrl() { const match = location.hash.match(/^#create\/(generator|auto|mine)$/); if (match) select(match[1]); }
  onMount(() => {
    try { select(app.requestedCreatePage ?? localStorage.getItem('harness_create_page') ?? 'generator'); } catch { /* unavailable */ }
    fromUrl(); window.addEventListener('hashchange', fromUrl);
    return () => window.removeEventListener('hashchange', fromUrl);
  });
  $effect(() => { if (app.requestedCreatePage) { select(app.requestedCreatePage); app.requestedCreatePage = null; } });
</script>
<div class="flex h-full min-h-0 w-full overflow-hidden bg-background">
  <SectionSidebar pane={panes.createNav} label={t('nav.create')} groups={CREATE_GROUPS} {active} onSelect={select} />
  <main class="min-w-0 flex-1 overflow-y-auto">
    <div class="flex max-w-6xl flex-col gap-5 p-6 max-md:p-4">
      <AdvancedPageNotice level={pageLevel(CREATE_GROUPS, active)} />
      <header><p class="text-xs text-muted-foreground">{t('nav.create')}</p><h1 class="mt-1 text-lg font-semibold">{t(CREATE_PAGES.find(p => p.id === active)!.label)}</h1></header>
      {#if active === 'generator'}
        <ModelPicker bind:endpoint bind:selectedModel />
        <ToolGeneratorDialog inline bind:open={generatorOpen} {endpoint} onUseModel={model => selectedModel = model} modelName={endpoint?.model ?? ''} modelPath={app.activeServer?.model_path ?? null} onChanged={() => revision++} />
      {:else if active === 'auto'}
        <AutoTools onChanged={() => revision++} />
      {:else}
        <MyTools {revision} />
      {/if}
    </div>
  </main>
</div>
