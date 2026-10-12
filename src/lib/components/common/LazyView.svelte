<script module lang="ts">
  import type { Component } from 'svelte';

  export type ViewModule = { default: Component<any> };
  export type ViewLoader = () => Promise<ViewModule>;

  /** Components already loaded, by loader: a view opened again renders at once. */
  const loaded = new Map<ViewLoader, Component<any>>();

  /** Loads a view once; shared by the view itself and the idle/hover preloading (`$lib/views`). */
  export function loadView(loader: ViewLoader): Promise<Component<any>> {
    const known = loaded.get(loader);
    if (known) return Promise.resolve(known);
    return loader().then((m) => {
      loaded.set(loader, m.default);
      return m.default;
    });
  }
</script>

<script lang="ts">
  import { untrack } from 'svelte';
  import { TriangleAlert } from '@lucide/svelte';
  import SectionLoading from './SectionLoading.svelte';
  import * as Empty from '$lib/components/ui/empty';
  import { Button } from '$lib/components/ui/button';
  import { t } from '$lib/i18n.svelte';

  /**
   * Renders a component that is loaded on first use (its own JS chunk), so the start screen
   * does not parse and keep code of views that may never be opened. Other props go to it. `fallback` shows a loading skeleton of the screen
   * while the chunk arrives (tab views).
   */
  let { load, fallback = false, ...rest }: { load: ViewLoader; fallback?: boolean; [prop: string]: unknown } = $props();

  // Already loaded: render in the first frame (no empty flash when a tab is opened again).
  let View = $state<Component<any> | null>(loaded.get(untrack(() => load)) ?? null);
  /** The chunk could not be loaded: the skeleton would otherwise stay forever. */
  let failed = $state(false);

  $effect(() => {
    const loader = load;
    const known = loaded.get(loader);
    if (known) {
      View = known;
      return;
    }
    let cancelled = false;
    failed = false;
    loadView(loader)
      .then((c) => {
        if (!cancelled) View = c;
      })
      .catch((e) => {
        console.error('[LazyView] load failed:', e);
        if (!cancelled) failed = true;
      });
    return () => {
      cancelled = true;
    };
  });
</script>

{#if View}
  <View {...rest} />
{:else if fallback && failed}
  <!-- A failed module import is cached by the browser for this page: only a reload retries it. -->
  <Empty.Root class="h-full">
    <Empty.Header>
      <Empty.Media variant="icon"><TriangleAlert /></Empty.Media>
      <Empty.Title>{t('sl.view_failed')}</Empty.Title>
      <Empty.Description>{t('sl.view_failed_desc')}</Empty.Description>
    </Empty.Header>
    <Empty.Content>
      <Button variant="outline" onclick={() => location.reload()}>{t('sl.view_reload')}</Button>
    </Empty.Content>
  </Empty.Root>
{:else if fallback}
  <!-- The chunk is still arriving: the screen's shape at once instead of an empty pane. -->
  <SectionLoading shape="page" class="h-full" label={t('sl.view_label')} phases={[t('sl.view_1'), t('sl.view_2')]} />
{/if}
