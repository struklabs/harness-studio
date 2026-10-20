<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { runtimesStore } from '#lib/stores/runtimes.svelte.js';
  import { getStarted } from '#lib/stores/getStarted.svelte.js';
  import { requestSection } from '#lib/stores/sectionPage.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Check, Circle } from '@lucide/svelte';

  interface Props {
    modelCount: number;
    onGoToHub: () => void;
    onChooseModel: () => void;
    onReopenWizard?: () => void;
  }
  let { modelCount, onGoToHub, onChooseModel, onReopenWizard }: Props = $props();

  let agentInstalled = $state(false);
  let agentConnected = $state(false);
  let agentTaskDone = $state(false);
  let linkCount = $state(0);
  let linkOnline = $state(false);
  let refreshing = $state(false);

  async function refresh() {
    if (refreshing) return;
    refreshing = true;
    try {
      void runtimesStore.load(true);
      const [det, history, links] = await Promise.allSettled([
        invoke<{ installed: boolean; connected: boolean }>('detect_agent'),
        invoke<{ status: string }[]>('agent_task_history'),
        invoke<{ state: string }[]>('link_list'),
      ]);
      if (det.status === 'fulfilled') {
        agentInstalled = det.value.installed;
        agentConnected = det.value.connected;
      }
      if (history.status === 'fulfilled') agentTaskDone = history.value.some((h) => h.status === 'completed');
      if (links.status === 'fulfilled') {
        linkCount = links.value.length;
        linkOnline = links.value.some((l) => l.state === 'online');
      }
    } finally {
      refreshing = false;
    }
  }

  onMount(() => {
    void refresh();
  });

  const engineReady = $derived(runtimesStore.list.some((r) => r.available));
  const modelReady = $derived(modelCount > 0);

  function openModules() {
    app.requestedModule = 'agents';
    app.openTab('modules');
  }
  function openLinks() {
    requestSection('connections', 'harness-link');
    app.openTab('connections');
  }

  interface Step { key: string; done: boolean; go: () => void }
  const scenarios = $derived<{ id: string; steps: Step[] }[]>([
    {
      id: 'model',
      steps: [
        { key: 'engine', done: engineReady, go: () => onReopenWizard?.() },
        { key: 'download', done: modelReady, go: onGoToHub },
        { key: 'start', done: app.isRunning, go: onChooseModel },
      ],
    },
    {
      id: 'agent',
      steps: [
        { key: 'model', done: modelReady, go: onGoToHub },
        { key: 'install', done: agentInstalled, go: openModules },
        { key: 'connect', done: agentConnected, go: openModules },
        { key: 'task', done: agentTaskDone, go: openModules },
      ],
    },
    {
      id: 'remote',
      steps: [
        { key: 'add', done: linkCount > 0, go: openLinks },
        { key: 'online', done: linkOnline, go: openLinks },
      ],
    },
  ]);
  const total = $derived(scenarios.reduce((n, s) => n + s.steps.length, 0));
  const doneCount = $derived(scenarios.reduce((n, s) => n + s.steps.filter((x) => x.done).length, 0));
</script>

{#if getStarted.hidden}
  <Button variant="ghost" size="sm" class="self-start" onclick={() => getStarted.show()}>{t('gs.show')}</Button>
{:else}
  <section class="flex flex-col gap-3 rounded-lg border border-border bg-card/60 p-3" aria-labelledby="gs-title">
    <div class="flex items-start justify-between gap-2">
      <div class="min-w-0">
        <h2 id="gs-title" class="text-sm font-semibold">{t('gs.title')}</h2>
        <p class="text-xs text-muted-foreground">{t('gs.desc')}</p>
      </div>
      <Button variant="ghost" size="sm" onclick={() => getStarted.hide()}>{t('gs.hide')}</Button>
    </div>
    <p class="text-xs text-muted-foreground" role="status">{t('gs.progress', { done: doneCount, total })}</p>
    {#each scenarios as scenario (scenario.id)}
      <div class="flex flex-col gap-1.5 border-t border-border pt-2">
        <h3 class="text-sm font-medium">{t(`gs.${scenario.id}.title` as TKey)}</h3>
        <p class="text-xs text-muted-foreground">{t(`gs.${scenario.id}.desc` as TKey)}</p>
        <ul class="flex flex-col gap-1">
          {#each scenario.steps as step (step.key)}
            <li class="flex items-center justify-between gap-2 text-sm">
              <span class="flex min-w-0 items-center gap-1.5">
                {#if step.done}<Check class="size-3.5 shrink-0 text-success" aria-label={t('gs.done')} />{:else}<Circle class="size-3.5 shrink-0 text-muted-foreground" aria-label={t('gs.todo')} />{/if}
                <span class="break-words">{t(`gs.${scenario.id}.${step.key}` as TKey)}</span>
              </span>
              {#if !step.done}
                <Button variant="outline" size="sm" onclick={step.go}>{t(`gs.${scenario.id}.${step.key}_do` as TKey)}</Button>
              {/if}
            </li>
          {/each}
        </ul>
        <details class="text-xs text-muted-foreground">
          <summary class="cursor-pointer">{t('gs.recovery')}</summary>
          <ul class="mt-1 flex list-disc flex-col gap-1 pl-4">
            <li>{t(`gs.${scenario.id}.r1` as TKey)}</li>
            <li>{t(`gs.${scenario.id}.r2` as TKey)}</li>
            <li>{t(`gs.${scenario.id}.r3` as TKey)}</li>
          </ul>
        </details>
      </div>
    {/each}
    <div class="flex flex-wrap gap-2">
      <Button variant="outline" size="sm" disabled={refreshing} onclick={() => void refresh()}>{t('gs.refresh')}</Button>
      {#if onReopenWizard}<Button variant="ghost" size="sm" onclick={onReopenWizard}>{t('gs.setup')}</Button>{/if}
    </div>
  </section>
{/if}
