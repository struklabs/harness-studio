<script lang="ts">
  import AdvancedPageNotice from './common/AdvancedPageNotice.svelte';
  import { pageLevel } from '#lib/uiLevel.js';
  import SectionSidebar from './common/SectionSidebar.svelte';
  import { panes } from '#lib/stores/panes.svelte.js';
  import { SectionSelection } from '#lib/stores/sectionPage.svelte.js';
  import { PERFORMANCE_GROUPS } from './common/screenSections';
  import type { AppSettings, ServerStatus } from '#lib/types.js';
  import { t } from '#lib/i18n.svelte.js';
  import PerformanceView from './PerformanceView.svelte';
  import Benchmark from './Benchmark.svelte';

  interface Props {
    visible?: boolean;
    settings: AppSettings;
    serverStatus: ServerStatus;
    onGoToSettings: () => void;
  }

  let { visible = true, settings, serverStatus, onGoToSettings }: Props = $props();


  const section = new SectionSelection('performance', PERFORMANCE_GROUPS.flatMap(g => g.pages.map(p => p.id)), localStorage.getItem('harness_perf_view') === 'benchmark' ? 'new' : 'overview', undefined, PERFORMANCE_GROUPS);
  let benchmarkVisited = $state(false);
  $effect(() => { if (['new', 'results', 'compare'].includes(section.active)) benchmarkVisited = true; });
</script>
<div class="flex h-full min-h-0 overflow-hidden">
  <SectionSidebar pane={panes.performanceNav} label={t('nav.performance')} groups={PERFORMANCE_GROUPS} active={section.active} onSelect={section.select} />
  <main class="flex min-w-0 flex-1 flex-col overflow-hidden">
    <AdvancedPageNotice level={pageLevel(PERFORMANCE_GROUPS, section.active)} />
    <h1 class="shrink-0 border-b border-border px-6 py-3 text-lg font-semibold">{t(PERFORMANCE_GROUPS.flatMap(g => g.pages).find(p => p.id === section.active)!.label)}</h1>
    <div hidden={['new', 'results', 'compare'].includes(section.active)} class="min-h-0 flex-1">
      <PerformanceView visible={visible && !['new', 'results', 'compare'].includes(section.active)} {settings} {serverStatus} page={section.active} />
    </div>
    <div hidden={!['new', 'results', 'compare'].includes(section.active)} class="min-h-0 flex-1">
      {#if benchmarkVisited}<Benchmark {onGoToSettings} page={section.active} onSelectPage={section.select} />{/if}
    </div>
  </main>
</div>
