<script lang="ts">
  import { fixed } from '#lib/format.js';
  import { Play, Square, RotateCcw, PackageOpen, Trash2, Pencil, Copy } from '@lucide/svelte';
  import { t, locale } from '#lib/i18n.svelte.js';
  import { errorText } from '#lib/errors.js';
  import { invoke } from '#lib/ipc.js';
  import { training } from '#lib/stores/training.svelte.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { library } from '#lib/stores/library.svelte.js';
  import { samePath } from '#lib/stores/domain.svelte.js';
  import { loadAndChat } from '#lib/stores/downloads.svelte.js';
  import { canCompare } from './runActions';
  import type { TrainingJob } from '#lib/training-types.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { recipeFromConfig } from './wizardConfig';
  import { Progress } from '#lib/components/ui/progress/index.js';
  import { STATUS_CLASS, STATUS_KEY, canExport, elapsedSeconds, fmtDuration, fmtLoss, liveStats } from './trainingFormat';
  import type { TKey } from '#lib/i18n.svelte.js';

  interface Props {
    jobs: TrainingJob[];
    now: number;
    onExport: (job: TrainingJob) => void;
  }
  let { jobs, now, onExport }: Props = $props();

  let removing = $state<number | null>(null);
  let renaming = $state<number | null>(null);
  let renameText = $state('');
  let renameError = $state<string | null>(null);
  let copiedId = $state<number | null>(null);
  let loading = $state<string | null>(null);

  async function chat(path: string) {
    loading = path;
    try {
      library.markChanged();
      await library.refresh();
      const model = library.models.find(m => samePath(m.path, path));
      if (!model) throw new Error(t('training.export_model_missing'));
      await loadAndChat(model);
    } catch (e) { training.actionError = errorText(e); }
    finally { loading = null; }
  }

  function compare(job: TrainingJob) {
    training.compareJobId = job.id;
    app.openPlaygroundTab('chat');
  }

  async function commitRename(job: TrainingJob) {
    renameError = null;
    const name = renameText.trim();
    if (!name || name === job.config.name) {
      renaming = null;
      return;
    }
    try {
      await invoke('training_rename', { jobId: job.id, name });
      renaming = null;
      await training.refresh();
    } catch (e) {
      renameError = errorText(e);
    }
  }

  async function copyRecipe(job: TrainingJob) {
    try {
      await navigator.clipboard.writeText(recipeFromConfig(job.config));
      copiedId = job.id;
      setTimeout(() => (copiedId = null), 2000);
    } catch {
      // clipboard not available
    }
  }

  const canCancel = (j: TrainingJob) => j.status === 'draft' || j.status === 'queued' || j.status === 'running';
  const canResume = (j: TrainingJob) => (j.status === 'failed' || j.status === 'cancelled') && j.checkpoint_step !== null;
  const canRestart = (j: TrainingJob) => (j.status === 'failed' || j.status === 'cancelled') && j.checkpoint_step === null;
  const percent = (j: TrainingJob) => (j.total_steps > 0 ? Math.min(100, (j.step / j.total_steps) * 100) : 0);
</script>

<ul class="flex flex-col gap-2" aria-label={t('training.jobs_title')}>
  {#each jobs as job (job.id)}
    {@const elapsed = elapsedSeconds(job, now)}
    {@const live = liveStats(job, now)}
    <li>
      <div
        role="button"
        tabindex="0"
        class="flex w-full cursor-pointer flex-col gap-2 rounded-lg border p-3 text-left transition-colors hover:bg-muted/40 {training.selectedId === job.id ? 'border-primary' : 'border-border'}"
        aria-pressed={training.selectedId === job.id}
        onclick={() => training.select(job.id)}
        onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && e.target === e.currentTarget && (e.preventDefault(), training.select(job.id))}
      >
        <div class="flex flex-wrap items-center gap-2">
          {#if renaming === job.id}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <span class="flex min-w-0 flex-1 items-center gap-1" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
              <Input class="h-7" bind:value={renameText} maxlength={80} aria-label={t('training.job_name')} onkeydown={(e) => { if (e.key === 'Enter') void commitRename(job); if (e.key === 'Escape') renaming = null; }} />
              <Button size="sm" onclick={() => commitRename(job)}>{t('training.rename_save')}</Button>
            </span>
          {:else}
            <span class="min-w-0 flex-1 truncate text-sm font-medium">{job.config.name}</span>
          {/if}
          <span class="rounded-full px-2 py-0.5 text-xs font-medium {STATUS_CLASS[job.status]}">{job.paused && job.status === 'cancelled' ? t('training.paused') : job.paused && job.status === 'running' ? t('training.pausing') : t(STATUS_KEY[job.status])}</span>
        </div>
        <p class="text-xs text-muted-foreground">
          {t(`training.method_${job.config.method}` as TKey)} · {t(`training.device_${job.config.device}` as TKey)}
          {#if elapsed !== null}· {t('training.elapsed', { time: fmtDuration(elapsed) })}{/if}
          {#if job.resumed}· {t('training.resumed')}{/if}
        </p>

        {#if renaming === job.id && renameError}<p class="text-xs text-destructive" role="alert">{renameError}</p>{/if}

        {#if job.status !== 'draft'}
          <div class="flex flex-col gap-1">
            <Progress value={percent(job)} />
            <div class="flex flex-wrap gap-x-4 text-xs text-muted-foreground">
              <span>{t('training.step_of', { step: job.step, total: job.total_steps })}</span>
              <span>{t('training.loss')}: {fmtLoss(job.loss, locale())}</span>
              <span>{t('training.eval_loss')}: {fmtLoss(job.eval_loss, locale())}</span>
              {#if job.status === 'running' && !job.training}<span>{t('training.loading_model')}</span>{/if}
              {#if live}
                <span>{t('training.live_speed', { sps: fixed(live.stepsPerSecond, 2), tps: Math.round(live.tokensPerSecond).toLocaleString() })}</span>
                <span>{t('training.live_eta', { time: fmtDuration(live.etaSeconds) })}</span>
              {/if}
            </div>
          </div>
        {/if}

        {#if job.error}
          <p class="text-xs text-destructive" role="alert">{errorText(job.error)}</p>
        {/if}

        {#if job.export}
          <div class="rounded-md bg-muted/50 p-2 text-xs">
            <p class="font-medium">
              {t('training.export_status')}: {t(`training.export_${job.export.status}` as TKey)}
            </p>
            {#if job.export.dir}<p class="break-all text-muted-foreground">{t('training.export_folder')}: {job.export.dir}</p>{/if}
            {#each job.export.ggufs?.length ? job.export.ggufs : job.export.gguf ? [job.export.gguf] : [] as path}
              <div class="flex flex-wrap items-center gap-2">
                <p class="break-all text-muted-foreground">{t('training.export_gguf_file')}: {path}</p>
                {#if !job.export.adapter_only}
                  <Button size="sm" disabled={loading !== null} onclick={(e) => { e.stopPropagation(); void chat(path); }}>{t('training.load_chat')}</Button>
                {/if}
              </div>
            {/each}
            {#if job.export.warning}<p class="text-amber-700 dark:text-amber-400">{errorText(job.export.warning)}</p>{/if}
            {#if job.export.error}<p class="text-destructive">{errorText(job.export.error)}</p>{/if}
          </div>
        {/if}

        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
        <details class="text-xs" onclick={(e) => e.stopPropagation()}>
          <summary class="cursor-pointer text-muted-foreground">{t('training.run_settings')}</summary>
          <dl class="mt-1 grid grid-cols-2 gap-x-4 gap-y-0.5 sm:grid-cols-3">
            <div><dt class="text-muted-foreground">{t('training.steps')}</dt><dd>{job.config.steps}</dd></div>
            <div><dt class="text-muted-foreground">{t('training.learning_rate')}</dt><dd>{job.config.learning_rate}</dd></div>
            <div><dt class="text-muted-foreground">{t('training.rank')}</dt><dd>{job.config.rank} / {job.config.lora_alpha || job.config.rank * 2}</dd></div>
            <div><dt class="text-muted-foreground">{t('training.batch_size')}</dt><dd>{job.config.batch_size} × {job.config.grad_accum}</dd></div>
            <div><dt class="text-muted-foreground">{t('training.sequence_length')}</dt><dd>{job.config.sequence_length}</dd></div>
            <div><dt class="text-muted-foreground">{t('training.lr_scheduler')}</dt><dd>{job.config.lr_scheduler}</dd></div>
          </dl>
        </details>

        <div class="flex flex-wrap gap-1" role="group" aria-label={t('training.actions')}>
          <Button size="sm" variant="ghost" onclick={(e) => { e.stopPropagation(); renameText = job.config.name; renameError = null; renaming = job.id; }}>
            <Pencil class="size-3.5" aria-hidden="true" />{t('training.rename')}
          </Button>
          <Button size="sm" variant="ghost" onclick={(e) => { e.stopPropagation(); void copyRecipe(job); }}>
            <Copy class="size-3.5" aria-hidden="true" />{copiedId === job.id ? t('training.recipe_copied') : t('training.recipe_copy')}
          </Button>
          {#if job.status === 'draft'}
            <Button size="sm" disabled={training.installing} onclick={(e) => { e.stopPropagation(); void training.startJob(job.id); }}>
              <Play class="size-3.5" aria-hidden="true" />{t('training.start')}
            </Button>
          {/if}
          {#if canCancel(job)}
            {#if job.status === 'running' && job.training}
              <Button size="sm" variant="outline" disabled={job.paused} onclick={(e) => { e.stopPropagation(); void training.pause(job.id); }}>{t('training.pause')}</Button>
            {/if}
            <Button size="sm" variant="outline" onclick={(e) => { e.stopPropagation(); void training.cancel(job.id); }}>
              <Square class="size-3.5" aria-hidden="true" />{t('training.cancel_job')}
            </Button>
          {/if}
          {#if canResume(job)}
            <Button size="sm" disabled={training.installing} onclick={(e) => { e.stopPropagation(); void training.resume(job.id); }}>
              <RotateCcw class="size-3.5" aria-hidden="true" />{t('training.resume')}
            </Button>
          {:else if canRestart(job)}
            <Button size="sm" disabled={training.installing} onclick={(e) => { e.stopPropagation(); void training.restart(job.id); }}>
              <RotateCcw class="size-3.5" aria-hidden="true" />{t('training.restart')}
            </Button>
          {/if}
          {#if canExport(job)}
            <Button size="sm" variant="secondary" onclick={(e) => { e.stopPropagation(); onExport(job); }}>
              <PackageOpen class="size-3.5" aria-hidden="true" />{t('training.export_to_library')}
            </Button>
          {/if}
          {#if canCompare(job)}
            <Button size="sm" variant="outline" onclick={(e) => { e.stopPropagation(); compare(job); }}>{t('training.compare_title')}</Button>
          {/if}
          {#if removing === job.id}
            <Button size="sm" variant="destructive" onclick={(e) => { e.stopPropagation(); removing = null; void training.remove(job.id); }}>{t('training.delete_confirm')}</Button>
            <Button size="sm" variant="ghost" onclick={(e) => { e.stopPropagation(); removing = null; }}>{t('training.cancel')}</Button>
          {:else if job.status !== 'running' && job.status !== 'queued'}
            <Button size="sm" variant="ghost" onclick={(e) => { e.stopPropagation(); removing = job.id; }}>
              <Trash2 class="size-3.5" aria-hidden="true" />{t('training.delete')}
            </Button>
          {/if}
        </div>
      </div>
    </li>
  {/each}
</ul>
