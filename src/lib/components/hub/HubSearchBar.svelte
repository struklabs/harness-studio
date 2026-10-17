<script lang="ts">
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  import { t } from '#lib/i18n.svelte.js';
  import type { HfSortKey } from '#lib/types.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import * as ToggleGroup from '#lib/components/ui/toggle-group/index.js';
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import { cn } from '#lib/utils.js';
  import { Search, SlidersHorizontal, LayoutGrid, List, Link, LoaderCircle } from '@lucide/svelte';
  import FilterSelect from './FilterSelect.svelte';

  interface Props {
    query: string;
    sort: HfSortKey;
    viewMode: 'grid' | 'list';
    showFilters: boolean;
    activeFilters: number;
    searching: boolean;
    onSearch: () => void;
    onSortChange: () => void;
    onCustomUrl: () => void;
  }

  let {
    query = $bindable(),
    sort = $bindable(),
    viewMode = $bindable(),
    showFilters = $bindable(),
    activeFilters,
    searching,
    onSearch,
    onSortChange,
    onCustomUrl,
  }: Props = $props();

  const SORTS: { value: HfSortKey; key: Parameters<typeof t>[0] }[] = [
    { value: 'downloads', key: 'hub.sort_downloads' },
    { value: 'likes', key: 'hub.sort_likes' },
    { value: 'trendingScore', key: 'hub.sort_trending' },
    { value: 'lastModified', key: 'hub.sort_updated' },
    { value: 'createdAt', key: 'hub.sort_newest' },
  ];
</script>

<div class="flex shrink-0 flex-wrap items-center gap-2">
  <!-- A form so Enter in the field searches however the key arrives (the Search button submits it). -->
  <form
    class="flex min-w-0 max-w-3xl flex-[1_1_24rem] items-center gap-2"
    onsubmit={(e) => {
      e.preventDefault();
      onSearch(); // a new search replaces one still running
    }}
  >
  <div class="relative min-w-0 flex-1">
    <Search class="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
    <FieldHelp text={t('help.hub_search')} class="w-full"><Input
      type="text"
      class="h-9 pl-8 text-sm"
      placeholder={t('hub.search_placeholder')}
      aria-label={t('hub.search_input_label')}
      bind:value={query}
    /></FieldHelp>
  </div>

  <Button type="submit" class="h-9 shrink-0" aria-busy={searching}>
    {#if searching}
      <LoaderCircle class="animate-spin" />
      {t('hub.searching')}
    {:else}
      <Search />
      {t('hub.search_button')}
    {/if}
  </Button>
  </form>

  <AdvancedOnly>
    <Button
      variant="outline"
      class={cn('h-9', showFilters && 'border-foreground/40 bg-muted text-foreground')}
      aria-expanded={showFilters}
      onclick={() => (showFilters = !showFilters)}
    >
      <SlidersHorizontal />
      {t('hub.filters_button')}
      {#if activeFilters > 0}
        <span class="rounded-full bg-muted px-1.5 font-num text-xs leading-4 text-muted-foreground">{activeFilters}</span>
      {/if}
    </Button>
  </AdvancedOnly>

  <FilterSelect help={t('help.hub_sort')}
    class="w-44 max-w-full"
    label={t('hub.sort_label')}
    showLabel={false}
    bind:value={sort}
    options={SORTS.map((s) => ({ value: s.value, label: t(s.key) }))}
    onchange={onSortChange}
  />

  <AdvancedOnly>
    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="outline" size="icon" class="h-9" onclick={onCustomUrl} aria-label={t('hub.custom_url_button')}>
            <Link />
          </Button>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content>{t('hub.custom_url_button')}</Tooltip.Content>
    </Tooltip.Root>

    <FieldHelp text={t('help.layout')}><ToggleGroup.Root
      type="single"
      variant="outline"
      size="sm"
      value={viewMode}
      onValueChange={(v) => v && (viewMode = v as 'grid' | 'list')}
      aria-label={t('hub.view_label')}
    >
      <ToggleGroup.Item value="grid" aria-label={t('hub.grid_view_title')}><LayoutGrid /></ToggleGroup.Item>
      <ToggleGroup.Item value="list" aria-label={t('hub.list_view_title')}><List /></ToggleGroup.Item>
    </ToggleGroup.Root></FieldHelp>
  </AdvancedOnly>
</div>
