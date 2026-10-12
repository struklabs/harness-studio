<script lang="ts">
  import { ShieldCheck } from '@lucide/svelte';
  import { t } from '$lib/i18n.svelte';
  import { Switch } from '$lib/components/ui/switch';

  interface Props {
    crashReports: boolean;
    hardwareInfo: boolean;
  }

  let { crashReports = $bindable(), hardwareInfo = $bindable() }: Props = $props();
</script>

<div class="flex flex-col gap-4">
  <div class="divide-y divide-border overflow-hidden rounded-lg border border-border bg-card">
    <div class="flex items-center justify-between gap-6 px-4 py-3">
      <label for="wiz-crash" class="flex min-w-0 flex-col gap-0.5">
        <span class="text-sm font-medium">{t('onboarding.wizard_opt_crash')}</span>
        <span class="text-xs text-muted-foreground">{t('onboarding.wizard_crash_note')}</span>
      </label>
      <Switch id="wiz-crash" bind:checked={crashReports} />
    </div>
    <div class="flex items-center justify-between gap-6 px-4 py-3">
      <label for="wiz-hw" class="flex min-w-0 flex-col gap-0.5">
        <span class="text-sm font-medium">{t('onboarding.wizard_hw_title')}</span>
        <span class="text-xs text-muted-foreground">{t('onboarding.wizard_hw_note')}</span>
      </label>
      <Switch id="wiz-hw" bind:checked={hardwareInfo} />
    </div>
  </div>

  <p class="flex items-start gap-2 text-xs text-muted-foreground">
    <ShieldCheck class="mt-0.5 size-4 shrink-0 text-success" />
    <span><strong class="text-foreground">{t('onboarding.wizard_zero_prompt_title')}</strong> {t('onboarding.wizard_zero_prompt_body')}</span>
  </p>
</div>
