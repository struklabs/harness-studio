<script lang="ts">
  import GenerationField from '../playground/GenerationField.svelte';
  import { untrack } from 'svelte';
  import { FolderOpen, Folder, TriangleAlert, CircleX, ShieldAlert } from '@lucide/svelte';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { invoke } from '#lib/ipc.js';
  import { errorText } from '#lib/errors.js';
  import { formatBytes } from '#lib/format.js';
  import type { DatasetReport } from '#lib/training-types.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { fmtInt } from './trainingFormat';
  import { histogramBars, truncatedShare } from './wizardConfig';
  import InfoTip from '#lib/components/common/InfoTip.svelte';

  interface Props {
    path: string;
    validationFraction: number;
    report: DatasetReport | null;
    /** Highlights histogram bars a sequence of this length would cut. */
    sequenceLength?: number;
  }
  let { path = $bindable(), validationFraction = $bindable(), report = $bindable(), sequenceLength = 512 }: Props = $props();

  let checking = $state(false);
  let failure = $state<string | null>(null);
  let seq = 0;

  async function check() {
    const p = path.trim();
    if (!p) {
      report = null;
      return;
    }
    const mine = ++seq;
    checking = true;
    failure = null;
    try {
      const r = await invoke<DatasetReport>('training_check_dataset', { path: p, validationFraction });
      if (mine === seq) report = r;
    } catch (e) {
      if (mine === seq) {
        report = null;
        failure = errorText(e);
      }
    } finally {
      if (mine === seq) checking = false;
    }
  }

  async function pick() {
    try {
      const picked = await invoke<string | null>('pick_training_dataset_file', { label: t('training.dataset_filter') });
      if (picked) {
        path = picked;
        await check();
      }
    } catch (e) {
      failure = errorText(e);
    }
  }

  async function pickFolder() {
    try {
      const picked = await invoke<string | null>('pick_training_dataset_folder');
      if (picked) {
        path = picked;
        await check();
      }
    } catch (e) {
      failure = errorText(e);
    }
  }

  let bars = $derived(report ? histogramBars(report, sequenceLength) : []);
  let cut = $derived(report ? truncatedShare(report, sequenceLength) : 0);
  let evalPercent = $derived(Math.round(validationFraction * 100));

  // The split depends on the validation fraction: re-check when it changes and a path is set.
  $effect(() => {
    void validationFraction;
    untrack(() => {
      if (path.trim()) void check();
    });
  });

  let unknownLicense = $derived(report !== null && !report.license);
  // The license box above already carries the unknown-license warning.
  let issues = $derived(report ? report.issues.filter((i) => i.key !== 'training.dataset_license_unknown') : []);
</script>

<div class="flex flex-col gap-3">
  <GenerationField id="training-dataset" label={t('training.dataset_path')} help="gen.dataset">
    <div class="flex gap-2">
      <Input id="training-dataset" bind:value={path} placeholder={t('training.dataset_placeholder')} onchange={check} onkeydown={(e) => e.key === 'Enter' && check()} />
      <Button type="button" variant="outline" onclick={pick}><FolderOpen class="size-4" aria-hidden="true" />{t('training.choose_file')}</Button>
      <Button type="button" variant="outline" onclick={pickFolder}><Folder class="size-4" aria-hidden="true" />{t('training.choose_folder')}</Button>
    </div>
    <p class="text-xs text-muted-foreground">{t('training.dataset_hint')}</p>
  </GenerationField>

  {#if checking}
    <p class="text-xs text-muted-foreground">{t('training.dataset_checking')}</p>
  {/if}
  {#if failure}
    <p class="text-xs text-destructive" role="alert">{failure}</p>
  {/if}

  {#if report}
    <dl class="grid grid-cols-2 gap-x-4 gap-y-1 rounded-md border border-border p-3 text-sm sm:grid-cols-3">
      <div><dt class="text-xs text-muted-foreground">{t('training.dataset_format')}</dt><dd>{report.format ? t(`training.format_${report.format}` as TKey) : '-'}</dd></div>
      <div><dt class="text-xs text-muted-foreground">{t('training.dataset_rows')}</dt><dd>{fmtInt(report.rows)}</dd></div>
      <div><dt class="text-xs text-muted-foreground">{t('training.dataset_split')}</dt><dd>{fmtInt(report.train_rows)} / {fmtInt(report.val_rows)}</dd></div>
      <div><dt class="text-xs text-muted-foreground">{t('training.dataset_size')}</dt><dd>{formatBytes(report.bytes, 1)}</dd></div>
      <div><dt class="text-xs text-muted-foreground">{t('training.dataset_tokens')}</dt><dd>~{fmtInt(report.estimated_tokens)}</dd></div>
      <div><dt class="text-xs text-muted-foreground">{t('training.dataset_longest')}</dt><dd>~{fmtInt(report.max_row_tokens)}</dd></div>
    </dl>

    {#if report.mapping?.length}
      <div class="flex flex-col gap-1 text-sm">
        <p class="flex items-center gap-1 font-medium">{t('training.dataset_mapping')}<InfoTip text={t('training.help_mapping')} label={t('training.dataset_mapping')} /></p>
        <ul class="flex flex-wrap gap-2 text-xs">
          {#each report.mapping as m (m.role)}
            <li class="rounded-full bg-muted px-2 py-0.5">{m.role === 'messages' ? t('training.map_messages', { field: m.field }) : t('training.map_role', { field: m.field, role: t(`training.role_${m.role}` as TKey) })}</li>
          {/each}
        </ul>
      </div>
    {/if}

    <div class="flex flex-col gap-1 text-sm">
      <div class="flex items-center gap-1">
        <label for="training-eval-split" class="font-medium">{t('training.eval_split')}</label>
        <InfoTip text={t('training.help_validation_fraction')} label={t('training.eval_split')} />
        <span class="ml-auto text-xs text-muted-foreground">{t('training.eval_split_value', { percent: evalPercent, train: fmtInt(report.train_rows), val: fmtInt(report.val_rows) })}</span>
      </div>
      <input id="training-eval-split" type="range" min="1" max="50" step="1" value={evalPercent} class="w-full accent-primary" oninput={(e) => (validationFraction = Number((e.currentTarget as HTMLInputElement).value) / 100)} />
    </div>

    {#if bars.length && report.rows > 0}
      <div class="flex flex-col gap-1 text-sm">
        <p class="flex items-center gap-1 font-medium">{t('training.length_histogram')}<InfoTip text={t('training.help_histogram')} label={t('training.length_histogram')} /></p>
        <div class="flex h-20 items-end gap-1" role="img" aria-label={t('training.length_histogram')}>
          {#each bars as b (b.label)}
            <div class="flex h-full flex-1 flex-col items-center justify-end gap-0.5" title={`${b.label}: ${b.count}`}>
              <div class="w-full rounded-t {b.truncated ? 'bg-amber-500/70' : 'bg-primary/70'}" style="height: {Math.max(b.count > 0 ? 4 : 0, b.height * 100)}%"></div>
            </div>
          {/each}
        </div>
        <div class="flex gap-1 text-[10px] text-muted-foreground" aria-hidden="true">
          {#each bars as b (b.label)}<span class="flex-1 text-center">{b.label}</span>{/each}
        </div>
        {#if cut > 0}
          <p class="text-xs text-amber-700 dark:text-amber-400">{t('training.truncated_share', { percent: Math.round(cut * 100), length: sequenceLength })}</p>
        {/if}
      </div>
    {/if}

    {#if report.samples?.length}
      <div class="flex flex-col gap-2 text-sm">
        <p class="flex items-center gap-1 font-medium">{t('training.samples_title')}<InfoTip text={t('training.help_samples')} label={t('training.samples_title')} /></p>
        {#each report.samples as sample, i (i)}
          <div class="flex flex-col gap-1 rounded-md border border-border p-2 text-xs">
            {#each sample.messages as m, j (j)}
              <p class="whitespace-pre-wrap break-words"><span class="mr-1 font-semibold">{t(`training.role_${m.role}` as TKey)}:</span>{m.content}</p>
            {/each}
            <p class="text-muted-foreground">~{fmtInt(sample.tokens)} {t('training.tokens_unit')}</p>
          </div>
        {/each}
      </div>
    {/if}

    <div class="flex items-start gap-2 rounded-md border p-3 text-sm {unknownLicense ? 'border-amber-500/50 bg-amber-500/10' : 'border-border'}">
      <ShieldAlert class="mt-0.5 size-4 shrink-0 {unknownLicense ? 'text-amber-600 dark:text-amber-400' : 'text-muted-foreground'}" aria-hidden="true" />
      <div class="min-w-0">
        <p class="font-medium">{t('training.dataset_license')}: {report.license ?? t('training.license_unknown')}</p>
        {#if unknownLicense}<p class="text-xs text-muted-foreground">{t('training.license_warning')}</p>{/if}
      </div>
    </div>

    {#if issues.length}
      <ul class="flex flex-col gap-1 text-sm" aria-label={t('training.dataset_issues')}>
        {#each issues as issue, i (i)}
          <li class="flex items-start gap-2">
            {#if issue.level === 'error'}
              <CircleX class="mt-0.5 size-4 shrink-0 text-destructive" aria-hidden="true" />
            {:else}
              <TriangleAlert class="mt-0.5 size-4 shrink-0 text-amber-600 dark:text-amber-400" aria-hidden="true" />
            {/if}
            <span>
              {issue.line !== null ? t('training.issue_line', { line: issue.line }) + ' ' : ''}{t(issue.key as TKey, issue.params)}
            </span>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</div>
