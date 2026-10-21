<script lang="ts">
  import { untrack } from 'svelte';
  import { CircleCheck, TriangleAlert, CircleX, Check, Copy, RefreshCw, Sparkles } from '@lucide/svelte';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { invoke } from '#lib/ipc.js';
  import { errorText } from '#lib/errors.js';
  import { formatBytes } from '#lib/format.js';
  import { library } from '#lib/stores/library.svelte.js';
  import { training } from '#lib/stores/training.svelte.js';
  import type {
    DatasetReport,
    TrainingDevice,
    TrainingNameCheck,
    TrainingPreset,
    TrainingPresetSet,
    TrainingPreview,
  } from '#lib/training-types.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Switch } from '#lib/components/ui/switch/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import InfoTip from '#lib/components/common/InfoTip.svelte';
  import DatasetStep from './DatasetStep.svelte';
  import BaseModelFetch from './BaseModelFetch.svelte';
  import ParamField from './ParamField.svelte';
  import { baseName, fmtDuration } from './trainingFormat';
  import {
    applyPreset,
    buildRequest,
    changedFields,
    defaultState,
    effectiveBatch,
    parseRecipe,
    presetFields,
    recipeFromConfig,
    settingsError,
    type WizardState,
  } from './wizardConfig';

  interface Props {
    open: boolean;
    onCreated?: (id: number) => void;
  }
  let { open = $bindable(), onCreated }: Props = $props();

  const MODE_KEY = 'training.wizard.advanced';
  function loadMode(): boolean {
    try {
      return localStorage.getItem(MODE_KEY) === '1';
    } catch {
      return false;
    }
  }
  let advanced = $state(loadMode());
  function setAdvanced(v: boolean) {
    advanced = v;
    try {
      localStorage.setItem(MODE_KEY, v ? '1' : '0');
    } catch {
      // the choice just is not remembered
    }
  }

  let steps = $derived((advanced ? ['model', 'dataset', 'method', 'preset', 'review'] : ['model', 'dataset', 'preset', 'review']) as readonly string[]);
  let step = $state(0);
  let current = $derived(steps[Math.min(step, steps.length - 1)]);

  let s = $state<WizardState>(defaultState());
  let report = $state<DatasetReport | null>(null);
  let methodChoice = $state<'auto' | 'lora' | 'qlora'>('auto');
  let modelFilter = $state('');

  let presetSet = $state<TrainingPresetSet | null>(null);
  let presetTier = $state<string>('balanced');
  let presetBusy = $state(false);
  let presetError = $state<string | null>(null);

  let preview = $state<TrainingPreview | null>(null);
  let previewing = $state(false);
  let busy = $state(false);
  let installing = $state(false);
  let error = $state<string | null>(null);

  let nameCheck = $state<TrainingNameCheck | null>(null);
  let recipeText = $state('');
  let recipeError = $state<string | null>(null);
  let copied = $state(false);

  /** Folders with a config.json that are not GGUF: the Hugging Face (safetensors) models. */
  let models = $derived(
    library.models
      .filter((m) => !m.path.toLowerCase().endsWith('.gguf') && !m.filename.toLowerCase().endsWith('.gguf') && (!m.kind || m.kind === 'llm') && !m.task)
      .sort((a, b) => a.name.localeCompare(b.name)),
  );
  let shownModels = $derived(models.filter((m) => m.name.toLowerCase().includes(modelFilter.trim().toLowerCase())));

  let compat = $derived(training.compat);
  let devices = $derived<TrainingDevice[]>(compat?.devices?.length ? compat.devices : ['cpu']);
  let qlora = $derived(compat?.methods.find((m) => m.method === 'qlora') ?? null);
  let qloraBlocked = $derived(qlora !== null && qlora.support === 'unsupported');
  let unavailable = $derived((compat?.methods ?? []).filter((m) => m.method === 'full' || m.method === 'dpo'));

  $effect(() => {
    if (!devices.includes(s.device)) s.device = devices.includes('cuda') ? 'cuda' : devices.includes('mps') ? 'mps' : 'cpu';
  });
  $effect(() => {
    if (methodChoice === 'qlora' && (qloraBlocked || !devices.includes('cuda'))) methodChoice = 'auto';
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
    presetSet = null;
    nameCheck = null;
    recipeError = null;
    recipeText = '';
    methodChoice = 'auto';
    s = defaultState();
  }

  // New data or a different model needs fresh presets.
  $effect(() => {
    void report?.path;
    void report?.rows;
    untrack(() => (presetSet = null));
  });

  let selectedPreset = $derived<TrainingPreset | null>(presetSet?.presets.find((p) => p.tier === presetTier) ?? null);
  let changed = $derived(selectedPreset ? changedFields(s, selectedPreset) : []);
  let rec = $derived(selectedPreset ? presetFields(selectedPreset) : null);
  let invalidField = $derived(settingsError(s));

  let stepValid = $derived.by(() => {
    if (current === 'model') return s.model.trim().length > 0;
    if (current === 'dataset') return report?.ok === true;
    if (current === 'method') return true;
    if (current === 'preset') return selectedPreset !== null && invalidField === null;
    return true;
  });

  function pickModel(path: string) {
    if (s.model !== path) {
      s.model = path;
      presetSet = null;
    }
  }

  function importRecipe() {
    recipeError = null;
    const r = parseRecipe(recipeText);
    if (!r.ok) {
      recipeError = t(`training.recipe_error_${r.error}` as TKey, { field: r.field ?? '' });
      return;
    }
    s = r.state;
    methodChoice = s.method;
    presetSet = null;
    recipeText = '';
  }

  async function loadPresets() {
    presetBusy = true;
    presetError = null;
    try {
      const set = await invoke<TrainingPresetSet>('training_presets', {
        request: {
          model: s.model.trim(),
          dataset: s.dataset.trim() || undefined,
          device: s.device,
          method: methodChoice === 'auto' ? undefined : methodChoice,
          validation_fraction: s.validationFraction,
        },
      });
      presetSet = set;
      const pick = set.presets.find((p) => p.recommended) ?? set.presets[0];
      if (pick) {
        presetTier = pick.tier;
        s = applyPreset(s, pick);
      }
    } catch (e) {
      presetSet = null;
      presetError = errorText(e);
    } finally {
      presetBusy = false;
    }
  }

  function choosePreset(p: TrainingPreset) {
    presetTier = p.tier;
    s = applyPreset(s, p);
  }

  async function loadPreview() {
    previewing = true;
    error = null;
    preview = null;
    try {
      preview = await invoke<TrainingPreview>('training_preview', { request: buildRequest(s) });
    } catch (e) {
      error = errorText(e);
    } finally {
      previewing = false;
    }
  }

  async function checkName(name: string | null) {
    try {
      const r = await invoke<TrainingNameCheck>('training_check_name', {
        name: name || undefined,
        model: s.model.trim(),
        dataset: s.dataset.trim(),
      });
      if (!name && !s.name.trim()) s.name = r.suggested;
      nameCheck = r;
    } catch {
      nameCheck = null;
    }
  }

  let nameTimer: ReturnType<typeof setTimeout> | undefined;
  function onNameInput() {
    clearTimeout(nameTimer);
    nameTimer = setTimeout(() => void checkName(s.name.trim() || null), 300);
  }

  async function next() {
    if (step >= steps.length - 1) return;
    step += 1;
    const target = steps[step];
    if (target === 'preset' && (!presetSet || presetSet.presets.length === 0)) await loadPresets();
    if (target === 'review') {
      await checkName(s.name.trim() || null);
      await loadPreview();
    }
  }

  function back() {
    step -= 1;
  }

  // A different method or device needs new presets.
  function methodChanged() {
    presetSet = null;
  }

  async function create(start: boolean) {
    busy = true;
    error = null;
    try {
      if (start && !training.envReady) {
        // The training engine is installed or updated on the way, without asking.
        installing = true;
        const ready = await training.ensureEnv();
        installing = false;
        if (!ready) {
          error = training.installError ?? t('training.env_required');
          return;
        }
      }
      const job = await training.create(buildRequest(s));
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
      installing = false;
      busy = false;
    }
  }

  async function copyRecipe() {
    const cfg = preview?.[0];
    if (!cfg) return;
    try {
      await navigator.clipboard.writeText(recipeFromConfig(cfg));
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch (e) {
      error = errorText(e);
    }
  }

  type Fit = 'fits' | 'tight' | 'no';
  let fit = $derived.by<Fit | null>(() => {
    const est = preview?.[2];
    if (!est) return null;
    if (!est.fits) return 'no';
    return est.memory_bytes > est.available_bytes * 0.85 ? 'tight' : 'fits';
  });

  const FIT_CLASS = { fits: 'border-emerald-500/40 bg-emerald-500/10', tight: 'border-amber-500/50 bg-amber-500/10', no: 'border-destructive/40 bg-destructive/10' } as const;
  const schedulerOptions = $derived([
    { value: 'linear', label: t('training.scheduler_linear') },
    { value: 'cosine', label: t('training.scheduler_cosine') },
    { value: 'constant', label: t('training.scheduler_constant') },
  ]);
  const deviceOptions = $derived(devices.map((d) => ({ value: d, label: t(`training.device_${d}` as TKey) })));
  const gb = (n: number) => formatBytes(n, 1);
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
    <Dialog.Header>
      <Dialog.Title>{t('training.new_job')}</Dialog.Title>
      <Dialog.Description>{t('training.wizard_step', { n: step + 1, total: steps.length, name: t(`training.step_${current}` as TKey) })}</Dialog.Description>
    </Dialog.Header>

    <div class="flex items-center gap-2 text-sm">
      <Switch id="training-advanced" checked={advanced} onCheckedChange={(v) => setAdvanced(v === true)} />
      <label for="training-advanced" class="font-medium">{t('training.advanced_mode')}</label>
      <InfoTip text={t('training.help_advanced_mode')} label={t('training.advanced_mode')} />
    </div>

    {#if current === 'model'}
      <div class="flex flex-col gap-3 text-sm">
        <div class="flex flex-col gap-1">
          <div class="flex items-center gap-2">
            <label for="training-model-filter" class="font-medium">{t('training.base_model')}</label>
            <Button type="button" size="sm" variant="ghost" class="ml-auto" onclick={() => library.refresh()}><RefreshCw class="size-3.5" aria-hidden="true" />{t('training.refresh_library')}</Button>
          </div>
          <Input id="training-model-filter" bind:value={modelFilter} placeholder={t('training.model_search')} />
          <ul class="max-h-56 overflow-y-auto rounded-md border border-border" aria-label={t('training.base_model')}>
            {#each shownModels as m (m.id)}
              <li>
                <button
                  type="button"
                  class="flex w-full items-center gap-2 px-3 py-2 text-left hover:bg-muted/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring {s.model === m.path ? 'bg-primary/10' : ''}"
                  aria-pressed={s.model === m.path}
                  onclick={() => pickModel(m.path)}
                >
                  <span class="min-w-0 flex-1 truncate font-medium">{m.name}</span>
                  <span class="shrink-0 text-xs text-muted-foreground">{m.architecture ?? ''} {m.size_bytes ? `· ${gb(m.size_bytes)}` : ''}</span>
                  {#if s.model === m.path}<Check class="size-4 shrink-0 text-primary" aria-hidden="true" />{/if}
                </button>
              </li>
            {:else}
              <li class="px-3 py-3 text-xs text-muted-foreground">{models.length ? t('training.model_none_match') : t('training.model_none')}</li>
            {/each}
          </ul>
        </div>
        <BaseModelFetch
          onReady={(repoId) => {
            const hit = models.find((m) => m.path.replaceAll(String.fromCharCode(92), '/').toLowerCase().includes(`/${repoId.toLowerCase()}`));
            if (hit) pickModel(hit.path);
          }}
        />
        <div class="flex flex-col gap-1">
          <label for="training-model-path">{t('training.model_path')}</label>
          <Input id="training-model-path" bind:value={s.model} placeholder={t('training.model_path_placeholder')} oninput={() => (presetSet = null)} />
          <p class="text-xs text-muted-foreground">{t('training.model_hint')}</p>
        </div>
        <details class="rounded-md border border-border p-3">
          <summary class="cursor-pointer text-sm font-medium">{t('training.recipe_import')}</summary>
          <div class="mt-2 flex flex-col gap-2">
            <textarea
              class="min-h-24 w-full rounded-md border border-input bg-transparent p-2 font-mono text-xs"
              aria-label={t('training.recipe_import')}
              placeholder={t('training.recipe_placeholder')}
              bind:value={recipeText}
            ></textarea>
            <div class="flex items-center gap-2">
              <Button type="button" size="sm" variant="outline" disabled={!recipeText.trim()} onclick={importRecipe}>{t('training.recipe_apply')}</Button>
              {#if recipeError}<span class="text-xs text-destructive" role="alert">{recipeError}</span>{/if}
            </div>
          </div>
        </details>
      </div>
    {:else if current === 'dataset'}
      <DatasetStep bind:path={s.dataset} bind:validationFraction={s.validationFraction} bind:report sequenceLength={s.sequenceLength} />
    {:else if current === 'method'}
      <div class="flex flex-col gap-3 text-sm">
        <div class="flex items-center gap-1">
          <p class="font-medium">{t('training.method')}</p>
          <InfoTip text={t('training.help_method')} label={t('training.method')} />
        </div>
        <div class="grid gap-2 sm:grid-cols-3" role="radiogroup" aria-label={t('training.method')}>
          {#each [{ id: 'auto', blocked: false }, { id: 'lora', blocked: false }, { id: 'qlora', blocked: qloraBlocked || !devices.includes('cuda') }] as m (m.id)}
            <button
              type="button"
              role="radio"
              aria-checked={methodChoice === m.id}
              disabled={m.blocked}
              class="flex flex-col gap-1 rounded-md border p-3 text-left disabled:cursor-not-allowed disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring {methodChoice === m.id ? 'border-primary bg-primary/10' : 'border-border hover:bg-muted/40'}"
              onclick={() => {
                methodChoice = m.id as 'auto' | 'lora' | 'qlora';
                methodChanged();
              }}
            >
              <span class="font-medium">{t(`training.method_pick_${m.id}` as TKey)}</span>
              <span class="text-xs text-muted-foreground">{m.id === 'qlora' && m.blocked && qlora ? t(qlora.reason as TKey, qlora.params) : t(`training.method_pick_${m.id}_hint` as TKey)}</span>
            </button>
          {/each}
        </div>
        {#if qlora && methodChoice === 'qlora' && qlora.support === 'limited'}
          <p class="text-xs text-muted-foreground">{t(qlora.reason as TKey, qlora.params)}</p>
        {/if}
        {#if unavailable.length}
          <div class="flex flex-col gap-1 rounded-md border border-dashed border-border p-3">
            <p class="text-xs font-medium">{t('training.method_unavailable')}</p>
            {#each unavailable as m (m.method)}
              <p class="text-xs text-muted-foreground"><span class="font-medium text-foreground">{t(`training.method_name_${m.method}` as TKey)}:</span> {t(m.reason as TKey, m.params)}</p>
            {/each}
          </div>
        {/if}
      </div>
    {:else if current === 'preset'}
      <div class="flex flex-col gap-4 text-sm">
        {#if presetBusy}
          <p class="text-muted-foreground">{t('training.presets_loading')}</p>
        {:else if presetError}
          <p class="text-xs text-destructive" role="alert">{presetError}</p>
          <div><Button size="sm" variant="outline" onclick={loadPresets}>{t('training.presets_retry')}</Button></div>
        {:else if presetSet}
          <div class="flex flex-col gap-1">
            <p class="text-xs text-muted-foreground">{t('training.presets_hint', { device: t(`training.device_${presetSet.device}` as TKey), memory: gb(presetSet.available_bytes) })}</p>
            <div class="grid gap-2 sm:grid-cols-3" role="radiogroup" aria-label={t('training.presets_title')}>
              {#each presetSet.presets as p (p.tier)}
                <button
                  type="button"
                  role="radio"
                  aria-checked={presetTier === p.tier}
                  class="flex flex-col gap-1 rounded-md border p-3 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring {presetTier === p.tier ? 'border-primary bg-primary/10' : 'border-border hover:bg-muted/40'}"
                  onclick={() => choosePreset(p)}
                >
                  <span class="flex items-center gap-2">
                    <span class="font-medium">{t(`training.tier_${p.tier}` as TKey)}</span>
                    {#if p.recommended}<span class="inline-flex items-center gap-1 rounded-full bg-emerald-500/15 px-2 py-0.5 text-xs text-emerald-700 dark:text-emerald-400"><Sparkles class="size-3" aria-hidden="true" />{t('training.recommended')}</span>{/if}
                  </span>
                  <span class="text-xs text-muted-foreground">{t(`training.tier_${p.tier}_hint` as TKey)}</span>
                  <span class="text-xs">{t(`training.method_${p.method}` as TKey)} · {t('training.preset_facts', { seq: p.sequence_length, rank: p.rank, batch: p.batch_size * p.grad_accum })}</span>
                  <span class="text-xs">{gb(p.estimate.memory_bytes)} · ~{fmtDuration(p.estimate.seconds)}</span>
                  <span class="text-xs font-medium {p.fit === 'fits' ? 'text-emerald-700 dark:text-emerald-400' : p.fit === 'tight' ? 'text-amber-700 dark:text-amber-400' : 'text-destructive'}">{t(`training.fit_${p.fit}` as TKey)}</span>
                  {#each p.notes as n (n)}<span class="text-xs text-muted-foreground">{t(n as TKey)}</span>{/each}
                </button>
              {/each}
            </div>
          </div>

          {#if advanced}
            <div class="flex flex-col gap-3">
              <div class="flex items-center gap-2">
                <p class="font-medium">{t('training.settings_title')}</p>
                {#if selectedPreset && changed.length}
                  <span class="text-xs text-muted-foreground">{t('training.settings_changed', { n: changed.length })}</span>
                  <Button size="sm" variant="ghost" class="ml-auto" onclick={() => selectedPreset && choosePreset(selectedPreset)}>{t('training.settings_reset_all')}</Button>
                {/if}
              </div>
              <div class="grid gap-3 sm:grid-cols-2">
                <ParamField id="tp-device" label={t('training.device')} help={t('training.help_device')} bind:value={s.device} options={deviceOptions} recommended={rec?.device ?? null} />
                <ParamField id="tp-steps" label={t('training.steps')} help={t('training.help_steps')} bind:value={s.steps} min={1} max={100000} recommended={rec?.steps ?? null} />
                <ParamField id="tp-lr" label={t('training.learning_rate')} help={t('training.help_learning_rate')} bind:value={s.learningRate} min={0} max={1} step={0.00005} recommended={rec?.learningRate ?? null} />
                <ParamField id="tp-seq" label={t('training.sequence_length')} help={t('training.help_sequence_length')} bind:value={s.sequenceLength} min={32} max={8192} step={32} recommended={rec?.sequenceLength ?? null} />
                <ParamField id="tp-rank" label={t('training.rank')} help={t('training.help_rank')} bind:value={s.rank} min={1} max={256} recommended={rec?.rank ?? null} />
                <ParamField id="tp-alpha" label={t('training.lora_alpha')} help={t('training.help_lora_alpha')} bind:value={s.loraAlpha} min={1} max={1024} recommended={rec?.loraAlpha ?? null} />
                <ParamField id="tp-dropout" label={t('training.lora_dropout')} help={t('training.help_lora_dropout')} bind:value={s.loraDropout} min={0} max={0.5} step={0.01} recommended={rec?.loraDropout ?? null} />
                <ParamField id="tp-batch" label={t('training.batch_size')} help={t('training.help_batch_size')} bind:value={s.batchSize} min={1} max={64} recommended={rec?.batchSize ?? null} />
                <ParamField id="tp-accum" label={t('training.grad_accum')} help={t('training.help_grad_accum')} bind:value={s.gradAccum} min={1} max={256} recommended={rec?.gradAccum ?? null} />
                <ParamField id="tp-warmup" label={t('training.warmup_ratio')} help={t('training.help_warmup_ratio')} bind:value={s.warmupRatio} min={0} max={0.5} step={0.01} recommended={rec?.warmupRatio ?? null} />
                <ParamField id="tp-sched" label={t('training.lr_scheduler')} help={t('training.help_lr_scheduler')} bind:value={s.lrScheduler} options={schedulerOptions} recommended={rec?.lrScheduler ?? null} />
                <ParamField id="tp-wd" label={t('training.weight_decay')} help={t('training.help_weight_decay')} bind:value={s.weightDecay} min={0} max={1} step={0.01} recommended={rec?.weightDecay ?? null} />
                <label class="flex flex-col gap-1">
                  <span class="flex items-center gap-1 font-medium">{t('training.checkpoint_steps')}<InfoTip text={t('training.help_checkpoint_steps')} label={t('training.checkpoint_steps')} /></span>
                  <Input type="number" min="1" placeholder={t('training.auto')} bind:value={s.checkpointSteps} />
                </label>
                <label class="flex flex-col gap-1">
                  <span class="flex items-center gap-1 font-medium">{t('training.eval_steps')}<InfoTip text={t('training.help_eval_steps')} label={t('training.eval_steps')} /></span>
                  <Input type="number" min="1" placeholder={t('training.auto')} bind:value={s.evalSteps} />
                </label>
              </div>
              <p class="text-xs text-muted-foreground">{t('training.effective_batch', { n: effectiveBatch(s) })}</p>
              {#if invalidField}<p class="text-xs text-destructive" role="alert">{t('training.invalid_field', { field: t(`training.field_${invalidField}` as TKey) })}</p>{/if}
            </div>
          {:else}
            <p class="text-xs text-muted-foreground">{t('training.simple_hint')}</p>
          {/if}
        {/if}
      </div>
    {:else}
      <div class="flex flex-col gap-3 text-sm">
        <div class="flex flex-col gap-1">
          <div class="flex items-center gap-1">
            <label for="training-name" class="font-medium">{t('training.job_name')}</label>
            <InfoTip text={t('training.help_name')} label={t('training.job_name')} />
          </div>
          <div class="flex gap-2">
            <Input id="training-name" bind:value={s.name} maxlength={80} oninput={onNameInput} aria-invalid={nameCheck !== null && !nameCheck.available} />
            {#if nameCheck && nameCheck.suggested !== s.name}
              <Button type="button" variant="outline" onclick={() => { s.name = nameCheck?.suggested ?? s.name; void checkName(s.name); }}>{t('training.name_suggest')}</Button>
            {/if}
          </div>
          {#if nameCheck && !nameCheck.available}
            <p class="text-xs text-destructive" role="alert">{t(`training.name_taken_${nameCheck.taken_by ?? 'job'}` as TKey)}</p>
          {:else if nameCheck}
            <p class="text-xs text-muted-foreground">{t('training.name_ok')}</p>
          {/if}
          <p class="text-xs text-muted-foreground">{t('training.name_library_hint')}</p>
        </div>
        {#if previewing}
          <p class="text-muted-foreground">{t('training.estimating')}</p>
        {:else if preview}
          {@const cfg = preview[0]}
          {@const rep = preview[1]}
          {@const est = preview[2]}
          <dl class="grid grid-cols-2 gap-x-4 gap-y-1 rounded-md border border-border p-3 sm:grid-cols-3">
            <div class="col-span-2 sm:col-span-3"><dt class="text-xs text-muted-foreground">{t('training.base_model')}</dt><dd class="break-all">{baseName(cfg.model)}</dd></div>
            <div><dt class="text-xs text-muted-foreground">{t('training.method')}</dt><dd>{t(`training.method_${cfg.method}` as TKey)}</dd></div>
            <div><dt class="text-xs text-muted-foreground">{t('training.device')}</dt><dd>{t(`training.device_${cfg.device}` as TKey)}</dd></div>
            <div><dt class="text-xs text-muted-foreground">{t('training.steps')}</dt><dd>{cfg.steps}</dd></div>
            <div><dt class="text-xs text-muted-foreground">{t('training.sequence_length')}</dt><dd>{cfg.sequence_length}</dd></div>
            <div><dt class="text-xs text-muted-foreground">{t('training.rank')}</dt><dd>{cfg.rank}</dd></div>
            <div><dt class="text-xs text-muted-foreground">{t('training.batch_size')}</dt><dd>{cfg.batch_size} × {cfg.grad_accum}</dd></div>
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
            <div class="flex items-start gap-2 rounded-md border p-3 {FIT_CLASS[fit]}" role="status">
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
          <div>
            <Button type="button" size="sm" variant="outline" onclick={copyRecipe}><Copy class="size-3.5" aria-hidden="true" />{copied ? t('training.recipe_copied') : t('training.recipe_copy')}</Button>
          </div>
        {/if}
        {#if !training.envReady}
          <p class="text-xs text-muted-foreground" role="note">{t(training.env?.installed ? 'training.env_auto_update_note' : 'training.env_auto_note')}</p>
        {/if}
        {#if installing}
          <p class="text-xs text-muted-foreground" role="status">{t(training.env?.installed ? 'training.env_updating' : 'training.env_installing')}</p>
        {/if}
      </div>
    {/if}

    {#if error}<p class="text-xs text-destructive" role="alert">{error}</p>{/if}

    <Dialog.Footer>
      <Button variant="outline" disabled={busy} onclick={() => (open = false)}>{t('training.cancel')}</Button>
      {#if step > 0}<Button variant="outline" disabled={busy} onclick={back}>{t('training.back')}</Button>{/if}
      {#if step < steps.length - 1}
        <Button disabled={!stepValid || busy || presetBusy} onclick={next}>{t('training.next')}</Button>
      {:else}
        <Button variant="secondary" disabled={!preview || busy || (nameCheck !== null && !nameCheck.available)} onclick={() => create(false)}>{t('training.save_draft')}</Button>
        <Button disabled={!preview || busy || (nameCheck !== null && !nameCheck.available)} onclick={() => create(true)}>{t('training.create_start')}</Button>
      {/if}
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
