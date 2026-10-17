<script lang="ts">
  import FieldHelp from '#lib/components/common/FieldHelp.svelte';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Switch } from '#lib/components/ui/switch/index.js';
  import { cn } from '#lib/utils.js';
  import { RotateCcw } from '@lucide/svelte';
  import FilterSelect from './FilterSelect.svelte';

  interface Props {
    /** '' = any (picked in the always-visible task chips; cleared here by "reset"). */
    pipelineTag: string;
    /** '' = any; free-text license id when not in the list. */
    license: string;
    author: string;
    /** Also list repos that are not GGUF (default on). */
    includeNonGguf: boolean;
    tagFilter: string;
    quantFilter: string;
    sizeFilter: string;
    availableTags: string[];
    /** Server-side filters changed: re-run the search. */
    onServerFiltersChange: () => void;
  }

  let {
    pipelineTag = $bindable(),
    license = $bindable(),
    author = $bindable(),
    includeNonGguf = $bindable(),
    tagFilter = $bindable(),
    quantFilter = $bindable(),
    sizeFilter = $bindable(),
    availableTags,
    onServerFiltersChange,
  }: Props = $props();

  const LICENSES = [
    'apache-2.0',
    'mit',
    'llama3',
    'llama3.1',
    'llama3.2',
    'llama3.3',
    'llama4',
    'gemma',
    'cc-by-4.0',
    'cc-by-nc-4.0',
    'openrail',
    'other',
  ];
  const CUSTOM = '__custom__';

  const QUANTS: { value: string; key: TKey | null; text?: string }[] = [
    { value: 'ALL', key: 'hub.quant_all' },
    { value: 'Q4_K_M', key: 'hub.quant_q4km' },
    { value: 'Q5_K_M', key: 'hub.quant_q5km' },
    { value: 'Q8_0', key: 'hub.quant_q80' },
    { value: 'Q4_0', key: 'hub.quant_q40' },
    { value: 'IQ3', key: 'hub.quant_iq3' },
    { value: 'BF16/FP16', key: null, text: 'FP16 / BF16' },
  ];

  const SIZES: { value: string; key: TKey | null; text?: string }[] = [
    { value: 'ALL', key: 'hub.size_all' },
    { value: '<4B', key: null, text: '< 4B (< 3.5 GB)' },
    { value: '4B-9B', key: null, text: '4B - 9B (3.5 - 7.5 GB)' },
    { value: '10B-20B', key: null, text: '10B - 20B (7.5 - 16 GB)' },
    { value: '30B+', key: null, text: '30B+ (> 16 GB)' },
  ];

  let changeQueued = false;
  function changed() {
    if (changeQueued) return;
    changeQueued = true;
    queueMicrotask(() => {
      changeQueued = false;
      onServerFiltersChange();
    });
  }

  let customLicense = $state(false);
  let licenseSelect = $derived(customLicense || (license && !LICENSES.includes(license)) ? CUSTOM : license);

  function onLicenseSelect(value: string) {
    if (value === CUSTOM) {
      customLicense = true;
      return;
    }
    customLicense = false;
    license = value;
    changed();
  }

  let hasFilters = $derived(
    !!(pipelineTag || license || author.trim() || !includeNonGguf || tagFilter !== 'ALL' || quantFilter !== 'ALL' || sizeFilter !== 'ALL')
  );

  function resetAllFilters() {
    pipelineTag = '';
    license = '';
    customLicense = false;
    author = '';
    includeNonGguf = true;
    tagFilter = 'ALL';
    quantFilter = 'ALL';
    sizeFilter = 'ALL';
    changed();
  }
</script>

<div class="flex shrink-0 flex-col gap-3 rounded-lg border border-border bg-card p-3" role="group" aria-label={t('hub.search_filters_label')}>
  <div class="grid grid-cols-[repeat(auto-fill,minmax(min(100%,160px),1fr))] gap-3">
    <div class="flex min-w-0 flex-col gap-1">
      <FilterSelect help={t('help.license')}
        label={t('hub.license_label')}
        value={licenseSelect}
        options={[
          { value: '', label: t('hub.license_any') },
          ...LICENSES.map((l) => ({ value: l, label: l })),
          { value: CUSTOM, label: t('hub.license_custom_option') },
        ]}
        onchange={onLicenseSelect}
      />
      {#if licenseSelect === CUSTOM}
        <FieldHelp text={t('help.license')} class="w-full"><Input
          class="h-8 font-mono text-xs"
          aria-label={t('hub.license_custom_label')}
          placeholder={t('hub.license_custom_placeholder')}
          bind:value={license}
          onchange={changed}
        /></FieldHelp>
      {/if}
    </div>

    <div class="flex min-w-0 flex-col gap-1">
      <label for="hub-author" class="text-xs text-muted-foreground">{t('hub.author_label')}</label>
      <FieldHelp text={t('help.author')} class="w-full"><Input
        id="hub-author"
        class="h-8 text-xs"
        placeholder={t('hub.author_placeholder')}
        bind:value={author}
        onchange={changed}
      /></FieldHelp>
    </div>

    <FilterSelect help={t('help.quant')}
      label={t('hub.quant_label')}
      bind:value={quantFilter}
      options={QUANTS.map((x) => ({ value: x.value, label: x.key ? t(x.key) : (x.text ?? x.value) }))}
    />

    <FilterSelect help={t('help.size')}
      label={t('hub.size_label')}
      bind:value={sizeFilter}
      options={SIZES.map((x) => ({ value: x.value, label: x.key ? t(x.key) : (x.text ?? x.value) }))}
    />
  </div>

  <label class="flex items-start gap-2 text-xs" for="hub-non-gguf">
    <FieldHelp text={t('help.non_gguf')}><Switch
      id="hub-non-gguf"
      aria-label={t('hub.include_non_gguf')}
      size="sm"
      class="mt-px"
      checked={includeNonGguf}
      onCheckedChange={(c) => {
        includeNonGguf = c;
        changed();
      }}
    /></FieldHelp>
    <span class="flex min-w-0 flex-col gap-0.5">
      <span class="text-foreground">{t('hub.include_non_gguf')}</span>
      <span class="text-muted-foreground">{t('hub.include_non_gguf_hint')}</span>
    </span>
  </label>

  <div class="flex flex-wrap items-start justify-between gap-3">
    <div class="flex min-w-0 flex-wrap items-center gap-1.5">
      <span class="mr-1 text-xs text-muted-foreground">{t('hub.tag_filter_label')}</span>
      {#each ['ALL', ...availableTags] as tag (tag)}
        {@const active = tagFilter === tag}
        <FieldHelp text={t('help.tag_filter')}><button
          type="button"
          class={cn(
            'min-h-8 max-w-full break-words rounded-md border px-2 py-1 text-xs transition-colors focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none',
            active
              ? 'border-primary/50 bg-primary/10 text-primary'
              : 'border-border text-muted-foreground hover:bg-muted hover:text-foreground'
          )}
          aria-pressed={active}
          onclick={() => (tagFilter = tag)}
        >
          {tag === 'ALL' ? t('hub.filter_all') : `#${tag}`}
        </button></FieldHelp>
      {/each}
    </div>

    {#if hasFilters}
      <Button variant="ghost" size="xs" class="shrink-0" onclick={resetAllFilters}>
        <RotateCcw />
        {t('hub.reset_filters')}
      </Button>
    {/if}
  </div>
</div>
