<script lang="ts">
  import FieldHelp from './common/FieldHelp.svelte';
  import HelpPopover from './common/HelpPopover.svelte';
  import AdvancedOnly from './common/AdvancedOnly.svelte';
  import { localizeError } from '#lib/network.js';
  import { invoke } from '#lib/ipc.js';
  import { onMount } from 'svelte';
  import { toast } from 'svelte-sonner';
  import type { AppSettings } from '#lib/types.js';
  import {
    sourceLabel,
    type ImportCandidate,
    type ImportMode,
    type ImportResult,
    type ImportScan,
    type ImportSource,
  } from '#lib/import-types.js';
  import { formatBytes } from '#lib/format.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { t } from '#lib/i18n.svelte.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import * as Collapsible from '#lib/components/ui/collapsible/index.js';
  import * as RadioGroup from '#lib/components/ui/radio-group/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Checkbox } from '#lib/components/ui/checkbox/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import { Skeleton } from '#lib/components/ui/skeleton/index.js';
  import { cn } from '#lib/utils.js';
  import { Check, ChevronDown, CircleAlert, Eye, LoaderCircle, RefreshCw, X } from '@lucide/svelte';

  interface Props {
    inline?: boolean;
    onClose: () => void;
    /** Called after at least one model was imported (or a library dir added): rescan models. */
    onImported: () => void;
    /** Called when the LM Studio folder was added as a library directory. */
    onSettingsChanged?: (updated: AppSettings) => void;
  }

  let { inline = false, onClose, onImported, onSettingsChanged }: Props = $props();

  const uid = $props.id();

  let scan = $state<ImportScan | null>(null);
  let scanning = $state(false);
  let scanError = $state<string | null>(null);
  /** Selected candidate paths. */
  let selected = $state<Record<string, boolean>>({});
  let mode = $state<ImportMode>('link');
  let importing = $state(false);
  let importError = $state<string | null>(null);
  let results = $state<ImportResult[] | null>(null);
  let lmsBusy = $state(false);
  let lmsMsg = $state<{ ok: boolean; text: string } | null>(null);
  let lmsOpen = $state(false);

  const SOURCES: ImportSource[] = ['ollama', 'lmstudio'];

  let groups = $derived(
    SOURCES.map((source) => ({
      source,
      dir: scan ? (source === 'ollama' ? scan.ollama_dir : scan.lmstudio_dir) : null,
      items: scan ? scan.candidates.filter((c) => c.source === source) : [],
    }))
  );

  let selectedItems = $derived((scan?.candidates ?? []).filter((c) => !c.imported && selected[c.path]));
  let selectedBytes = $derived(selectedItems.reduce((sum, c) => sum + c.size_bytes, 0));
  let okCount = $derived(results?.filter((r) => r.ok).length ?? 0);
  let busy = $derived(importing || lmsBusy);

  onMount(() => {
    void runScan();
  });

  async function runScan() {
    scanning = true;
    scanError = null;
    try {
      scan = await invoke<ImportScan>('scan_import_sources');
      // Keep only selections that still point at importable candidates.
      const importable = new Set(scan.candidates.filter((c) => !c.imported).map((c) => c.path));
      selected = Object.fromEntries(Object.entries(selected).filter(([p, on]) => on && importable.has(p)));
    } catch (e) {
      scanError = t('imp.scan_failed', { error: String(e) });
    } finally {
      scanning = false;
    }
  }

  function close() {
    if (!busy) onClose();
  }

  /** Select all importable items of a group, or clear them when they are all selected. */
  function toggleGroup(items: ImportCandidate[]) {
    const importable = items.filter((c) => !c.imported);
    const allOn = importable.length > 0 && importable.every((c) => selected[c.path]);
    const next = { ...selected };
    for (const c of importable) next[c.path] = !allOn;
    selected = next;
  }

  async function handleImport() {
    if (selectedItems.length === 0 || importing) return;
    importing = true;
    importError = null;
    results = null;
    try {
      results = await invoke<ImportResult[]>('import_models', {
        paths: selectedItems.map((c) => c.path),
        mode,
      });
      const ok = results.filter((r) => r.ok).length;
      if (ok > 0) {
        onImported();
        toast.success(t('imp.results_summary', { ok, failed: results.length - ok }));
      }
      selected = {};
    } catch (e) {
      importError = t('imp.import_failed', { error: String(e) });
    } finally {
      importing = false;
    }
    await runScan();
  }

  async function addLmStudioDir(dirPath: string) {
    lmsBusy = true;
    lmsMsg = null;
    try {
      const updated = await invoke<AppSettings>('add_storage_directory', { dirPath });
      app.setSettings(updated);
      onSettingsChanged?.(updated);
      lmsMsg = { ok: true, text: t('imp.lms_alt_added') };
      onImported();
    } catch (e) {
      lmsMsg = { ok: false, text: t('imp.lms_alt_failed', { error: String(e) }) };
    } finally {
      lmsBusy = false;
    }
  }

  function detailText(r: ImportResult): string {
    if (!r.ok) return localizeError(r.detail);
    switch (r.detail) {
      case 'linked':
        return t('imp.detail_linked');
      case 'hardlinked':
        return t('imp.detail_hardlinked');
      case 'copied':
        return t('imp.detail_copied');
      default:
        return r.detail;
    }
  }
</script>

{#snippet content()}
  <header class="border-b border-border p-4"><h2 class="flex items-center gap-1 text-base font-semibold">{t('imp.title')}<HelpPopover text={t('imp.desc')} /></h2></header>
    <div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4">
      <!-- Detected folders -->
      <section class="flex flex-col gap-1.5" aria-labelledby="{uid}-folders">
        <div class="flex items-center justify-between gap-2">
          <h3 id="{uid}-folders" class="flex items-center gap-1.5 text-sm font-semibold text-foreground">
            {t('imp.folders_title')}
            <AdvancedOnly><HelpPopover text={t('imp.folders_hint')} /></AdvancedOnly>
          </h3>
          <Button variant="ghost" size="xs" onclick={runScan} disabled={scanning || busy}>
            <RefreshCw class={cn(scanning && 'animate-spin')} />{t('imp.rescan_btn')}
          </Button>
        </div>
        {#if scan}
          <ul class="flex flex-col gap-1 rounded-lg border border-border bg-muted/30 p-2.5 text-xs">
            {#each groups as g (g.source)}
              <li class="flex min-w-0 items-baseline gap-2">
                <span class="w-16 shrink-0 text-muted-foreground">{sourceLabel(g.source)}</span>
                {#if g.dir}
                  <code class="truncate font-mono" title={g.dir}>{g.dir}</code>
                {:else}
                  <span class="text-muted-foreground">{t('imp.not_found')}</span>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </section>

      {#if scanning && !scan}
        <div class="flex flex-col gap-2" role="status" aria-label={t('imp.scanning')}>
          <Skeleton class="h-9 w-full" />
          <Skeleton class="h-9 w-full" />
        </div>
      {/if}
      {#if scanError}
        <div class="flex gap-2 rounded-lg border border-destructive/30 bg-destructive/10 p-2.5 text-xs text-destructive" role="alert">
          <CircleAlert class="mt-px size-3.5 shrink-0" /><span class="min-w-0 break-words">{scanError}</span>
        </div>
      {/if}

      {#if scan}
        {#if scan.candidates.length === 0}
          <p class="py-4 text-center text-xs text-muted-foreground">{t('imp.no_candidates')}</p>
        {/if}

        {#each groups as g (g.source)}
          {#if g.items.length > 0}
            {@const label = sourceLabel(g.source)}
            {@const importable = g.items.filter((c) => !c.imported)}
            {@const allOn = importable.length > 0 && importable.every((c) => selected[c.path])}
            <section class="flex flex-col gap-1.5" aria-labelledby="{uid}-group-{g.source}">
              <div class="flex items-center justify-between gap-2">
                <h3 id="{uid}-group-{g.source}" class="text-sm font-semibold text-foreground">
                  {label}
                  <span class="font-num normal-case">· {t('imp.group_count', { count: g.items.length })}</span>
                </h3>
                <Button
                  variant="ghost"
                  size="xs"
                  disabled={importing || importable.length === 0}
                  aria-label={allOn ? t('imp.select_none_aria', { source: label }) : t('imp.select_all_aria', { source: label })}
                  onclick={() => toggleGroup(g.items)}
                >
                  {allOn ? t('imp.select_none') : t('imp.select_all')}
                </Button>
              </div>
              <ul class="divide-y divide-border overflow-hidden rounded-lg border border-border">
                {#each g.items as c (`${c.path}:${c.name}`)}
                  <li class={cn('flex items-center gap-3 px-3 py-2', c.imported && 'opacity-60')}>
                    <Checkbox
                      id="{uid}-c-{c.path}-{c.name}"
                      checked={c.imported || !!selected[c.path]}
                      disabled={c.imported || importing}
                      onCheckedChange={(v) => (selected = { ...selected, [c.path]: !!v })}
                    />
                    <Label for="{uid}-c-{c.path}-{c.name}" class="min-w-0 flex-1 truncate font-mono text-xs font-normal" title={c.path}>{c.name}</Label>
                    <span class="flex min-w-0 flex-wrap items-center justify-end gap-1.5">
                      <AdvancedOnly><span class="font-mono text-xs text-muted-foreground">{#if c.architecture}{c.architecture} · {/if}{c.quantization ?? t('imp.unknown_quant')}</span></AdvancedOnly>
                      {#if c.mmproj_path}
                        <Badge variant="secondary" class="text-xs" title={t('imp.chip_vision_title')}><Eye />{t('imp.chip_vision')}</Badge>
                      {/if}
                      {#if c.imported}
                        <Badge variant="outline" class="text-xs text-success"><Check />{t('imp.chip_imported')}</Badge>
                      {/if}
                      <span class="w-16 text-right font-num text-xs text-muted-foreground">{formatBytes(c.size_bytes)}</span>
                    </span>
                  </li>
                {/each}
              </ul>
            </section>
          {/if}
        {/each}

        {#if scan.candidates.length > 0}
          <section class="flex flex-col gap-2" aria-labelledby="{uid}-mode">
            <h3 id="{uid}-mode" class="text-sm font-semibold text-foreground">{t('imp.mode_legend')}</h3>
            <RadioGroup.Root bind:value={mode} disabled={importing} class="grid grid-cols-1 gap-2 sm:grid-cols-2">
              {#each [{ v: 'link', label: 'imp.mode_link', desc: 'imp.mode_link_desc' }, { v: 'copy', label: 'imp.mode_copy', desc: 'imp.mode_copy_desc' }] as const as m (m.v)}
                <Label
                  for="{uid}-mode-{m.v}"
                  class={cn(
                    'flex cursor-pointer items-start gap-2.5 rounded-lg border p-3 font-normal',
                    mode === m.v ? 'border-foreground/40 bg-muted/50' : 'border-border hover:bg-muted/40'
                  )}
                >
                  <RadioGroup.Item id="{uid}-mode-{m.v}" aria-describedby="{uid}-mode-{m.v}-note" value={m.v} class="mt-0.5" />
                  <span class="flex min-w-0 flex-col gap-0.5">
                    <span class="text-sm font-medium">{t(m.label)}</span>
                    <span id="{uid}-mode-{m.v}-note" class="text-xs text-muted-foreground">{t(m.desc)}</span>
                    <AdvancedOnly><span class="text-xs text-muted-foreground">{t(m.v === 'copy' ? 'imp.mode_copy_technical' : 'imp.mode_link_technical')}</span></AdvancedOnly>
                  </span>
                </Label>
              {/each}
            </RadioGroup.Root>
          </section>
        {/if}

        {#if scan.lmstudio_dir}
          {@const lmsDir = scan.lmstudio_dir}
          <Collapsible.Root bind:open={lmsOpen}>
            <Collapsible.Trigger
              class="flex w-full items-center justify-between gap-2 rounded-md text-left text-xs font-medium text-muted-foreground hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none"
            >
              {t('imp.lms_alt_title')}
              <ChevronDown class={cn('size-4 transition-transform', lmsOpen && 'rotate-180')} />
            </Collapsible.Trigger>
            <Collapsible.Content class="flex flex-col items-start gap-2 pt-2">
              <FieldHelp text={t('imp.lms_alt_desc')} />
              <Button variant="outline" size="sm" onclick={() => addLmStudioDir(lmsDir)} disabled={busy}>
                {#if lmsBusy}<LoaderCircle class="animate-spin" />{/if}
                {t('imp.lms_alt_btn')}
              </Button>
              {#if lmsMsg}
                <p class={cn('text-xs', lmsMsg.ok ? 'text-success' : 'text-destructive')} role={lmsMsg.ok ? 'status' : 'alert'}>{lmsMsg.text}</p>
              {/if}
            </Collapsible.Content>
          </Collapsible.Root>
        {/if}
      {/if}

      <div aria-live="polite" class="flex flex-col gap-3">
        {#if importing && mode === 'copy'}
          <p class="text-xs text-muted-foreground">{t('imp.importing_copy_note')}</p>
        {/if}
        {#if results}
          <section class="flex flex-col gap-1.5" aria-labelledby="{uid}-results">
            <h3 id="{uid}-results" class="text-sm font-semibold text-foreground">
              {t('imp.results_title')}
              <span class="font-num normal-case">· {t('imp.results_summary', { ok: okCount, failed: results.length - okCount })}</span>
            </h3>
            <ul class="flex flex-col gap-1 text-xs">
              {#each results as r, i (i)}
                <li class={cn('flex items-start gap-1.5', r.ok ? 'text-success' : 'text-destructive')}>
                  {#if r.ok}<Check class="mt-px size-3.5 shrink-0" />{:else}<X class="mt-px size-3.5 shrink-0" />{/if}
                  <span class="min-w-0 break-words"><span class="font-mono font-medium">{r.name}</span> <span class="text-muted-foreground">— {detailText(r)}</span></span>
                </li>
              {/each}
            </ul>
          </section>
        {/if}
      </div>

      {#if importError}
        <div class="flex gap-2 rounded-lg border border-destructive/30 bg-destructive/10 p-2.5 text-xs text-destructive" role="alert">
          <CircleAlert class="mt-px size-3.5 shrink-0" /><span class="min-w-0 break-words">{importError}</span>
        </div>
      {/if}
    </div>

    <footer class="items-center border-t border-border p-4 sm:justify-between">
      <span class="font-num text-xs text-muted-foreground">
        {#if selectedItems.length > 0}
          {t('imp.selected_count', { count: selectedItems.length, size: formatBytes(selectedBytes) })}
        {/if}
      </span>
      <div class="flex gap-2">
        <Button variant="ghost" onclick={close} disabled={busy}>{t('imp.close_btn')}</Button>
        <Button onclick={handleImport} disabled={busy || scanning || selectedItems.length === 0}>
          {#if importing}<LoaderCircle class="animate-spin" />{/if}
          {importing ? t('imp.importing_btn') : t('imp.import_btn')}
        </Button>
      </div>
    </footer>
{/snippet}
{#if inline}
  <section class="flex min-h-0 flex-col" aria-busy={scanning || importing}>{@render content()}</section>
{:else}
  <Dialog.Root open={true} onOpenChange={(o) => !o && close()}>
    <Dialog.Content class="flex max-h-[85vh] flex-col gap-0 p-0 sm:max-w-2xl" showCloseButton={!busy} aria-busy={scanning || importing}>
      <Dialog.Title class="sr-only">{t('imp.title')}</Dialog.Title><Dialog.Description class="sr-only">{t('imp.desc')}</Dialog.Description>
      {@render content()}
    </Dialog.Content>
  </Dialog.Root>
{/if}
