<script lang="ts">
  import { untrack } from 'svelte';
  import { FolderOpen, TriangleAlert, CircleX, ShieldAlert } from '@lucide/svelte';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { invoke } from '#lib/ipc.js';
  import { errorText } from '#lib/errors.js';
  import { formatBytes } from '#lib/format.js';
  import type { DatasetReport } from '#lib/training-types.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { fmtInt } from './trainingFormat';

  interface Props {
    path: string;
    validationFraction: number;
    report: DatasetReport | null;
  }
  let { path = $bindable(), validationFraction, report = $bindable() }: Props = $props();

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

  // The split depends on the validation fraction: re-check when it changes and a path is set.
  $effect(() => {
    void validationFraction;
    untrack(() => {
      if (path.trim()) void check();
    });
  });

  let unknownLicense = $derived(report !== null && !report.license);
</script>

<div class="flex flex-col gap-3">
  <div class="flex flex-col gap-1 text-sm">
    <label for="training-dataset">{t('training.dataset_path')}</label>
    <div class="flex gap-2">
      <Input id="training-dataset" bind:value={path} placeholder={t('training.dataset_placeholder')} onchange={check} onkeydown={(e) => e.key === 'Enter' && check()} />
      <Button type="button" variant="outline" onclick={pick}><FolderOpen class="size-4" aria-hidden="true" />{t('training.choose')}</Button>
    </div>
    <p class="text-xs text-muted-foreground">{t('training.dataset_hint')}</p>
  </div>

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

    <div class="flex items-start gap-2 rounded-md border p-3 text-sm {unknownLicense ? 'border-amber-500/50 bg-amber-500/10' : 'border-border'}">
      <ShieldAlert class="mt-0.5 size-4 shrink-0 {unknownLicense ? 'text-amber-600 dark:text-amber-400' : 'text-muted-foreground'}" aria-hidden="true" />
      <div class="min-w-0">
        <p class="font-medium">{t('training.dataset_license')}: {report.license ?? t('training.license_unknown')}</p>
        {#if unknownLicense}<p class="text-xs text-muted-foreground">{t('training.license_warning')}</p>{/if}
      </div>
    </div>

    {#if report.issues.length}
      <ul class="flex flex-col gap-1 text-sm" aria-label={t('training.dataset_issues')}>
        {#each report.issues as issue, i (i)}
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
