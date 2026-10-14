<script lang="ts">
  import { invoke } from '#lib/ipc.js';
  import { Check, X, TriangleAlert } from '@lucide/svelte';
  import type { AppSettings } from '#lib/types.js';
  import { t } from '#lib/i18n.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';

  interface Props {
    settings: AppSettings;
    onComplete: (updated: AppSettings) => void;
  }

  let { settings, onComplete }: Props = $props();

  let busy = $state<'accept' | 'decline' | null>(null);
  let error = $state<string | null>(null);

  /** Saves the consent choice. On failure the dialog stays open with the error so the user can retry. */
  async function choose(allow: boolean) {
    if (busy) return;
    busy = allow ? 'accept' : 'decline';
    error = null;
    try {
      const updated: AppSettings = {
        ...settings,
        privacy: {
          ...settings.privacy,
          onboarding_completed: true,
          crash_reports: allow,
          app_errors: allow,
          llama_errors: allow,
          hardware_info: allow,
          performance_metrics: false,
          anonymous_analytics: false,
        },
      };
      await invoke('save_app_settings', { settings: updated });
      onComplete(updated);
    } catch (e) {
      error = t('onboarding.consent_save_failed', { error: String(e) });
      busy = null;
    }
  }

  const allowed = [
    'onboarding.consent_item_crash',
    'onboarding.consent_item_llama_errors',
    'onboarding.consent_item_hardware',
  ] as const;
  const forbidden = [
    'onboarding.consent_item_prompts',
    'onboarding.consent_item_api_body',
    'onboarding.consent_item_paths',
    'onboarding.consent_item_keys',
  ] as const;
</script>

<!-- A required choice: no close button, Escape or outside click. -->
<Dialog.Root open>
  <Dialog.Content
    class="sm:max-w-lg"
    showCloseButton={false}
    interactOutsideBehavior="ignore"
    escapeKeydownBehavior="ignore"
  >
    <Dialog.Header>
      <Dialog.Title class="text-base font-semibold">{t('onboarding.consent_title')}</Dialog.Title>
      <Dialog.Description class="text-sm text-muted-foreground">{t('onboarding.consent_subtitle')}</Dialog.Description>
    </Dialog.Header>

    <div class="grid grid-cols-2 gap-3 max-[720px]:grid-cols-1">
      <div class="flex min-w-0 flex-col gap-2 rounded-lg border border-border bg-card p-3">
        <span class="text-xs font-medium text-success">{t('onboarding.consent_allowed_title')}</span>
        <ul class="flex flex-col gap-1.5 text-xs text-muted-foreground">
          {#each allowed as key (key)}
            <li class="flex items-start gap-1.5"><Check class="mt-0.5 size-3.5 shrink-0 text-success" /> <span>{t(key)}</span></li>
          {/each}
        </ul>
      </div>
      <div class="flex min-w-0 flex-col gap-2 rounded-lg border border-border bg-card p-3">
        <span class="text-xs font-medium text-destructive">{t('onboarding.consent_forbidden_title')}</span>
        <ul class="flex flex-col gap-1.5 text-xs text-muted-foreground">
          {#each forbidden as key (key)}
            <li class="flex items-start gap-1.5"><X class="mt-0.5 size-3.5 shrink-0 text-destructive" /> <span>{t(key)}</span></li>
          {/each}
        </ul>
      </div>
    </div>

    <p class="text-xs text-muted-foreground">{t('onboarding.consent_footer')}</p>

    {#if error}
      <p class="flex items-start gap-1.5 text-xs break-words text-destructive" role="alert">
        <TriangleAlert class="mt-0.5 size-3.5 shrink-0" /> <span>{error}</span>
      </p>
    {/if}

    <Dialog.Footer>
      <Button variant="outline" onclick={() => choose(false)} disabled={busy !== null}>
        {busy === 'decline' ? t('onboarding.wizard_saving') : t('onboarding.consent_decline')}
      </Button>
      <Button data-autofocus onclick={() => choose(true)} disabled={busy !== null}>
        {busy === 'accept' ? t('onboarding.wizard_saving') : t('onboarding.consent_accept')}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
