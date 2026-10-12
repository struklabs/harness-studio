<script lang="ts">
  import { untrack } from 'svelte';
  import { invoke } from '#lib/ipc.js';
  import { Check, TriangleAlert, X } from '@lucide/svelte';
  import type { AppSettings, SystemHardwareInfo } from '#lib/types.js';
  import { dialog } from '#lib/actions/dialog.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Progress } from '#lib/components/ui/progress/index.js';
  import { cn } from '#lib/utils.js';
  import { toast } from 'svelte-sonner';
  import BrandLogo from './common/BrandLogo.svelte';
  import StepHardware from './onboarding/StepHardware.svelte';
  import StepStorage from './onboarding/StepStorage.svelte';
  import StepRuntime from './onboarding/StepRuntime.svelte';
  import StepPurpose from './onboarding/StepPurpose.svelte';
  import StepPrivacy from './onboarding/StepPrivacy.svelte';
  import CliPathOption from './onboarding/CliPathOption.svelte';

  interface Props {
    settings: AppSettings;
    hardware: SystemHardwareInfo | null;
    onComplete: (updated: AppSettings) => void;
    /** Present when the wizard was reopened from Settings: leaves without saving. */
    onClose?: () => void;
  }

  let { settings, hardware, onComplete, onClose }: Props = $props();

  const TOTAL = 5;
  const steps: { tab: TKey; title: TKey; desc: TKey }[] = [
    { tab: 'onboarding.wizard_step1_tab', title: 'onboarding.wizard_step1_title', desc: 'onboarding.wizard_step1_desc' },
    { tab: 'onboarding.wizard_step2_tab', title: 'onboarding.wizard_step2_title', desc: 'onboarding.wizard_step2_desc' },
    { tab: 'onboarding.wizard_step3_tab', title: 'onboarding.wizard_step3_title', desc: 'onboarding.wizard_step3_desc' },
    { tab: 'onboarding.wizard_step_purpose_tab', title: 'onboarding.wizard_step_purpose_title', desc: 'onboarding.wizard_step_purpose_desc' },
    { tab: 'onboarding.wizard_step4_tab', title: 'onboarding.wizard_step4_title', desc: 'onboarding.wizard_step4_desc' },
  ];

  let step = $state(1);
  /** Furthest step reached; steps up to it can be revisited from the indicator. */
  let reached = $state(1);

  let selectedLevel = $state<'simple' | 'advanced'>(untrack(() => onClose || settings.privacy.onboarding_completed ? settings.ui_level ?? 'advanced' : 'simple'));
  let currentModelsDir = $state(untrack(() => settings.models_dir));
  let runtimeBusy = $state(false);
  let purposeBusy = $state(false);
  let purposeDone = $state(false);
  /** A download or install runs: leaving the wizard's steps is blocked. */
  const navBlocked = $derived(runtimeBusy || purposeBusy);
  let optCrashReports = $state(untrack(() => settings.privacy.crash_reports ?? true));
  let optHardware = $state(untrack(() => settings.privacy.hardware_info ?? true));

  let optCliPath = $state(false);

  let finishing = $state(false);
  let finishError = $state<string | null>(null);

  const current = $derived(steps[step - 1]);

  function goTo(n: number) {
    if (finishing || navBlocked) return;
    step = n;
    reached = Math.max(reached, n);
  }

  async function handleFinish() {
    if (finishing) return;
    finishing = true;
    finishError = null;
    try {
      // The runtime and folder steps changed settings in the backend; start from those, not from the
      // (possibly older) prop, so finishing does not write stale values back.
      const fresh: AppSettings = await invoke('get_app_settings');
      const updated: AppSettings = {
        ...fresh,
        ui_level: selectedLevel,
        models_dir: currentModelsDir,
        privacy: {
          ...fresh.privacy,
          onboarding_completed: true,
          crash_reports: optCrashReports,
          app_errors: fresh.privacy.app_errors ?? true,
          llama_errors: fresh.privacy.llama_errors ?? true,
          hardware_info: optHardware,
          performance_metrics: false,
          anonymous_analytics: false,
        },
      };
      await invoke('save_app_settings', { settings: updated });
      // The CLI on PATH is a convenience: a failure is reported but never blocks finishing.
      if (optCliPath) {
        try {
          await invoke('cli_path_install', { userPath: null });
        } catch (e) {
          toast.error(t('onboarding.wizard_cli_path_failed', { error: String(e) }));
        }
      }
      onComplete(updated);
    } catch (e) {
      finishError = t('onboarding.wizard_finish_failed', { error: String(e) });
      finishing = false;
    }
  }
</script>

<!-- Below the title bar (h-11), which stays usable: the window is frameless. -->
<div
  class="fixed inset-x-0 top-11 bottom-0 z-50 flex flex-col bg-background text-foreground"
  role="dialog"
  aria-modal="true"
  aria-labelledby="wizard-title"
  use:dialog
>
  <Progress value={(step / TOTAL) * 100} class="h-0.5 rounded-none" aria-label={t('onboarding.wizard_step_of', { current: step, total: TOTAL })} />

  <header class="flex shrink-0 items-center justify-between gap-6 px-8 pt-5 pb-2">
    <BrandLogo version="" />

    <nav class="flex items-center gap-1" aria-label={t('onboarding.wizard_title')}>
      {#each steps as s, i (i)}
        {@const n = i + 1}
        <button
          type="button"
          disabled={n > reached || finishing || navBlocked}
          aria-current={step === n ? 'step' : undefined}
          class={cn(
            'flex h-7 items-center gap-1.5 rounded-md px-2 text-xs transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring/50 disabled:cursor-default',
            step === n ? 'bg-muted font-medium text-foreground' : n <= reached ? 'text-muted-foreground hover:text-foreground' : 'text-muted-foreground/50',
          )}
          onclick={() => goTo(n)}
        >
          <span
            class={cn(
              'flex size-4 items-center justify-center rounded-full border font-num text-xs',
              step === n ? 'border-primary bg-primary text-primary-foreground' : n < step ? 'border-success/50 text-success' : 'border-border',
            )}
          >
            {#if n < step}<Check class="size-2.5" />{:else}{n}{/if}
          </span>
          <span class="max-[1100px]:sr-only">{t(s.tab)}</span>
        </button>
      {/each}
      {#if onClose}
        <Button variant="ghost" size="icon-sm" class="ml-2" onclick={onClose} aria-label={t('onboarding.wizard_close')}>
          <X />
        </Button>
      {/if}
    </nav>
  </header>

  <main class="min-h-0 flex-1 overflow-y-auto">
    <div class="mx-auto flex w-full max-w-xl flex-col gap-6 px-6 py-8">
      <div class="flex flex-col gap-1.5">
        <span class="text-xs text-muted-foreground">{t('onboarding.wizard_step_of', { current: step, total: TOTAL })}</span>
        <h1 id="wizard-title" class="text-lg font-semibold">{t(current.title)}</h1>
        <p class="text-sm text-muted-foreground">{t(current.desc)}</p>
      </div>

      <!-- Steps stay mounted so an in-progress download or detection survives going back and forth. -->
      <div class:hidden={step !== 1}><StepHardware {hardware} /></div>
      <div class:hidden={step !== 2}>
        <StepStorage modelsDir={currentModelsDir} onChanged={(u) => (currentModelsDir = u.models_dir)} />
      </div>
      <div class:hidden={step !== 3}>
        <StepRuntime bind:busy={runtimeBusy} {hardware} />
      </div>
      <div class:hidden={step !== 4}>
        <StepPurpose bind:level={selectedLevel} bind:busy={purposeBusy} bind:done={purposeDone} {hardware} ctx={settings.server.default_ctx || 32768} />
      </div>
      <div class:hidden={step !== 5}>
        <StepPrivacy bind:crashReports={optCrashReports} bind:hardwareInfo={optHardware} />
        <div class="mt-4"><CliPathOption bind:enabled={optCliPath} /></div>
      </div>
    </div>
  </main>

  <footer class="shrink-0 border-t border-border bg-card px-8 py-3">
    <div class="mx-auto flex w-full max-w-xl flex-col gap-2">
      {#if finishError}
        <p class="flex items-start gap-1.5 text-xs break-words text-destructive" role="alert">
          <TriangleAlert class="mt-0.5 size-3.5 shrink-0" /> <span>{finishError}</span>
        </p>
      {/if}
      <div class="flex items-center justify-between gap-4">
        {#if step > 1}
          <Button variant="ghost" onclick={() => goTo(step - 1)} disabled={finishing || navBlocked}>{t('onboarding.wizard_btn_prev')}</Button>
        {:else}
          <span></span>
        {/if}

        {#if step < TOTAL}
          <Button data-autofocus onclick={() => goTo(step + 1)} disabled={navBlocked}>
            {step === 4 && !purposeDone ? t('onboarding.wizard_btn_skip') : t('onboarding.wizard_btn_next')}
          </Button>
        {:else}
          <Button data-autofocus onclick={handleFinish} disabled={finishing}>
            {#if finishing}{t('onboarding.wizard_saving')}{:else if finishError}{t('onboarding.wizard_retry')}{:else}{t('onboarding.wizard_btn_finish')}{/if}
          </Button>
        {/if}
      </div>
    </div>
  </footer>
</div>
