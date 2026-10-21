<script lang="ts">
  import { untrack } from 'svelte';
  import { CircleCheck, TriangleAlert, CircleX } from '@lucide/svelte';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { invoke } from '#lib/ipc.js';
  import { errorText } from '#lib/errors.js';
  import { formatBytes } from '#lib/format.js';
  import { library } from '#lib/stores/library.svelte.js';
  import { training } from '#lib/stores/training.svelte.js';
  import type { DatasetReport, TrainingCreateRequest, TrainingDevice, TrainingMethod, TrainingPreview } from '#lib/training-types.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import NativeSelect from '#lib/components/dashboard/parts/NativeSelect.svelte';
  import DatasetStep from './DatasetStep.svelte';
  import { baseName, fmtDuration } from './trainingFormat';

  interface Props {
    open: boolean;
    onCreated?: (id: number) => void;
  }
  let { open = $bindable(), onCreated }: Props = $props();

  const STEPS = ['model', 'dataset', 'method', 'review'] as const;
  let step = $state(0);

  let model = $state('');
  let datasetPath = $state('');
  let report = $state<DatasetReport | null>(null);
  let name = $state('');
  let method = $state<TrainingMethod>('lora');
  let device = $state<TrainingDevice>('cpu');
  let steps = $state(100);
  let learningRate = $state(0.0002);
  let rank = $state(16);
  let sequenceLength = $state(512);
  let validationFraction = $state(0.1);
  let checkpointSteps = $state<number | null>(null);
  let evalSteps = $state<number | null>(null);

  let preview = $state<TrainingPreview | null>(null);
  let previewing = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  /** Folders with a config.json that are not GGUF: the Hugging Face (safetensors) models. */
  let models = $derived(
    library.models
      .filter((m) => !m.path.toLowerCase().endsWith('.gguf') && !m.filename.toLowerCase().endsWith('.gguf') && (!m.kind || m.kind === 'llm') && !m.task)
      .sort((a, b) => a.name.localeCompare(b.name)),
  );

  let compat = $derived(training.compat);
  let devices = $derived<TrainingDevice[]>(compat?.devices?.length ? compat.devices : ['cpu']);
  let qlora = $derived(compat?.methods.find((m) => m.method === 'qlora') ?? null);
  let qloraBlocked = $derived(qlora !== null && qlora.support === 'unsupported');

  // QLoRA needs CUDA: fall back to LoRA when the choice becomes impossible.
  $effect(() => {
    if (method === 'qlora' && (qloraBlocked || device !== 'cuda')) method = 'lora';
  });
  $effect(() => {
    if (!devices.includes(device)) device = devices.includes('cuda') ? 'cuda' : devices.includes('mps') ? 'mps' : 'cpu';
  });

  // Opening the wizard rescans the library and reads this machine's compatibility.
  $effect(() => {
    if (!open) return;
    untrack(() => {
      void library.refresh();
      void training.loadCompat();
    });
  });

  function reset() {
    step = 0;
    preview = null;
    error = null;
    report = null;
    datasetPath = '';
    name = '';
  }

  let stepValid = $derived.by(() => {
    if (step === 0) return model.trim().length > 0;
    if (step === 1) return report?.ok === true;
    if (step === 2) return steps >= 1 && rank >= 1 && sequenceLength >= 32 && learningRate > 0 && validationFraction >= 0.01 && validationFraction <= 0.5;
    return true;
  });

  function request(): TrainingCreateRequest {
    return {
      name: name.trim() || undefined,
      model: model.trim(),
      dataset: datasetPath.trim(),
      method,
      device,
      steps,
      learning_rate: learningRate,
      rank,
      sequence_length: sequenceLength,
      validation_fraction: validationFraction,
      checkpoint_steps: checkpointSteps || undefined,
      eval_steps: evalSteps || undefined,
    };
  }

  async function loadPreview() {
    previewing = true;
    error = null;
    preview = null;
    try {
      preview = await invoke<TrainingPreview>('training_preview', { request: request() });
    } catch (e) {
      error = errorText(e);
    } finally {
      previewing = false;
    }
  }

  async function next() {
    if (step < STEPS.length - 1) {
      step += 1;
      if (STEPS[step] === 'review') await loadPreview();
    }
  }

  async function create(start: boolean) {
    busy = true;
    error = null;
    try {
      const job = await training.create(request());
      if (start) {
        const started = await training.startJob(job.id);
        if (!started && training.actionError) {
          error = training.actionError;
          return;
        }
      }
      training.select(job.id);
      onCreated?.(job.id);
      open = false;
      reset();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }

  type Fit = 'fits' | 'tight' | 'no';
  let fit = $derived.by<Fit | null>(() => {
    const est = preview?.[2];
    if (!est) return null;
    if (!est.fits) return 'no';
    return est.memory_bytes > est.available_bytes * 0.85 ? 'tight' : 'fits';
  });
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
    <Dialog.Header>
      <Dialog.Title>{t('training.new_job')}</Dialog.Title>
      <Dialog.Description>{t('training.wizard_step', { n: step + 1, total: STEPS.length, name: t(`training.step_${STEPS[step]}` as TKey) })}</Dialog.Description>
    </Dialog.Header>

    {#if step === 0}
      <div class="flex flex-col gap-3 text-sm">
        <div class="flex flex-col gap-1">
          <label for="training-model">{t('training.base_model')}</label>
          <NativeSelect
            id="training-model"
            value={models.some((m) => m.path === model) ? model : ''}
            onchange={(e) => {
              const v = (e.currentTarget as HTMLSelectElement).value;
              if (v) model = v;
            }}
          >
            <option value="">{models.length ? t('training.model_pick') : t('training.model_none')}</option>
            {#each models as m (m.id)}<option value={m.path}>{m.name}</option>{/each}
          </NativeSelect>
        </div>
        <div class="flex flex-col gap-1">
          <label for="training-model-path">{t('training.model_path')}</label>
          <Input id="training-model-path" bind:value={model} placeholder={t('training.model_path_placeholder')} />
          <p class="text-xs text-muted-foreground">{t('training.model_hint')}</p>
        </div>
        <div class="flex flex-col gap-1">
          <label for="training-name">{t('training.job_name')}</label>
          <Input id="training-name" bind:value={name} maxlength={80} placeholder={model ? `${baseName(model)} LoRA` : ''} />
        </div>
      </div>
    {:else if step === 1}
      <DatasetStep bind:path={datasetPath} {validationFraction} bind:report />
    {:else if step === 2}
      <div class="flex flex-col gap-4 text-sm">
        <div class="grid gap-3 sm:grid-cols-2">
          <div class="flex flex-col gap-1">
            <label for="training-method">{t('training.method')}</label>
            <NativeSelect id="training-method" bind:value={method}>
              <option value="lora">{t('training.method_lora')}</option>
              <option value="qlora" disabled={qloraBlocked || !devices.includes('cuda')}>{t('training.method_qlora')}</option>
            </NativeSelect>
            {#if qlora && (qloraBlocked || qlora.support === 'limited')}
              <p class="text-xs text-muted-foreground">{t(qlora.reason as TKey, qlora.params)}</p>
            {/if}
          </div>
          <div class="flex flex-col gap-1">
            <label for="training-device">{t('training.device')}</label>
            <NativeSelect id="training-device" bind:value={device}>
              {#each devices as d (d)}<option value={d}>{t(`training.device_${d}` as TKey)}</option>{/each}
            </NativeSelect>
          </div>
          <label class="flex flex-col gap-1">{t('training.steps')}<Input type="number" min="1" max="100000" bind:value={steps} /></label>
          <label class="flex flex-col gap-1">{t('training.learning_rate')}<Input type="number" step="0.00005" min="0" max="1" bind:value={learningRate} /></label>
          <label class="flex flex-col gap-1">{t('training.rank')}<Input type="number" min="1" max="256" bind:value={rank} /></label>
          <label class="flex flex-col gap-1">{t('training.sequence_length')}<Input type="number" min="32" max="8192" step="32" bind:value={sequenceLength} /></label>
          <label class="flex flex-col gap-1">{t('training.validation_fraction')}<Input type="number" step="0.01" min="0.01" max="0.5" bind:value={validationFraction} /></label>
        </div>
        <details class="rounded-md border border-border p-3">
          <summary class="cursor-pointer text-sm font-medium">{t('training.advanced')}</summary>
          <div class="mt-3 grid gap-3 sm:grid-cols-2">
            <label class="flex flex-col gap-1">{t('training.checkpoint_steps')}<Input type="number" min="1" placeholder={t('training.auto')} bind:value={checkpointSteps} /></label>
            <label class="flex flex-col gap-1">{t('training.eval_steps')}<Input type="number" min="1" placeholder={t('training.auto')} bind:value={evalSteps} /></label>
          </div>
        </details>
      </div>
    {:else}
      <div class="flex flex-col gap-3 text-sm">
        {#if previewing}
          <p class="text-muted-foreground">{t('training.estimating')}</p>
        {:else if preview}
          {@const cfg = preview[0]}
          {@const rep = preview[1]}
          {@const est = preview[2]}
          <dl class="grid grid-cols-2 gap-x-4 gap-y-1 rounded-md border border-border p-3 sm:grid-cols-3">
            <div class="col-span-2 sm:col-span-3"><dt class="text-xs text-muted-foreground">{t('training.job_name')}</dt><dd>{cfg.name}</dd></div>
            <div class="col-span-2 sm:col-span-3"><dt class="text-xs text-muted-foreground">{t('training.base_model')}</dt><dd class="break-all">{cfg.model}</dd></div>
            <div><dt class="text-xs text-muted-foreground">{t('training.method')}</dt><dd>{t(`training.method_${cfg.method}` as TKey)}</dd></div>
            <div><dt class="text-xs text-muted-foreground">{t('training.device')}</dt><dd>{t(`training.device_${cfg.device}` as TKey)}</dd></div>
            <div><dt class="text-xs text-muted-foreground">{t('training.steps')}</dt><dd>{cfg.steps}</dd></div>
            <div><dt class="text-xs text-muted-foreground">{t('training.dataset_rows')}</dt><dd>{rep.rows} ({rep.train_rows} / {rep.val_rows})</dd></div>
            <div><dt class="text-xs text-muted-foreground">{t('training.dataset_license')}</dt><dd>{rep.license ?? t('training.license_unknown')}</dd></div>
            <div><dt class="text-xs text-muted-foreground">{t('training.model_params')}</dt><dd>{(est.params / 1e9).toFixed(2)} B</dd></div>
          </dl>
          <div class="grid gap-2 sm:grid-cols-2">
            <div class="rounded-md border border-border p-3">
              <p class="text-xs text-muted-foreground">{t('training.est_memory')}</p>
              <p class="text-base font-medium">{formatBytes(est.memory_bytes, 1)}</p>
              <p class="text-xs text-muted-foreground">{t('training.est_available', { size: formatBytes(est.available_bytes, 1) })}</p>
            </div>
            <div class="rounded-md border border-border p-3">
              <p class="text-xs text-muted-foreground">{t('training.est_time')}</p>
              <p class="text-base font-medium">{fmtDuration(est.seconds)}</p>
              <p class="text-xs text-muted-foreground">{t('training.est_per_step', { s: est.seconds_per_step.toFixed(2) })}</p>
            </div>
          </div>
          {#if fit}
            <div
              class="flex items-start gap-2 rounded-md border p-3 {fit === 'fits' ? 'border-emerald-500/40 bg-emerald-500/10' : fit === 'tight' ? 'border-amber-500/50 bg-amber-500/10' : 'border-destructive/40 bg-destructive/10'}"
              role="status"
            >
              {#if fit === 'fits'}
                <CircleCheck class="mt-0.5 size-4 text-emerald-600 dark:text-emerald-400" aria-hidden="true" />
              {:else if fit === 'tight'}
                <TriangleAlert class="mt-0.5 size-4 text-amber-600 dark:text-amber-400" aria-hidden="true" />
              {:else}
                <CircleX class="mt-0.5 size-4 text-destructive" aria-hidden="true" />
              {/if}
              <div>
                <p class="font-medium">{t(`training.fit_${fit}` as TKey)}</p>
                <p class="text-xs text-muted-foreground">{t(`training.fit_${fit}_hint` as TKey)}</p>
              </div>
            </div>
          {/if}
        {/if}
        {#if !training.envReady}
          <p class="text-xs text-amber-700 dark:text-amber-400" role="note">{t('training.env_required')}</p>
        {/if}
      </div>
    {/if}

    {#if error}<p class="text-xs text-destructive" role="alert">{error}</p>{/if}

    <Dialog.Footer>
      <Button variant="outline" disabled={busy} onclick={() => (open = false)}>{t('training.cancel')}</Button>
      {#if step > 0}<Button variant="outline" disabled={busy} onclick={() => (step -= 1)}>{t('training.back')}</Button>{/if}
      {#if step < STEPS.length - 1}
        <Button disabled={!stepValid || busy} onclick={next}>{t('training.next')}</Button>
      {:else}
        <Button variant="secondary" disabled={!preview || busy} onclick={() => create(false)}>{t('training.save_draft')}</Button>
        <Button disabled={!preview || busy || !training.envReady} onclick={() => create(true)}>{t('training.create_start')}</Button>
      {/if}
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
