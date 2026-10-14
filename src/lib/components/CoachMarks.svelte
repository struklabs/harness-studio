<script lang="ts">
  import { t } from '#lib/i18n.svelte.js';
  import { cn } from '#lib/utils.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { Badge } from '#lib/components/ui/badge/index.js';

  interface Props {
    onClose: () => void;
  }

  let { onClose }: Props = $props();

  let steps = $derived([
    { title: t('onboarding.coach_step1_title'), desc: t('onboarding.coach_step1_desc'), hint: t('onboarding.coach_step1_hint') },
    { title: t('onboarding.coach_step2_title'), desc: t('onboarding.coach_step2_desc'), hint: t('onboarding.coach_step2_hint') },
    { title: t('onboarding.coach_step3_title'), desc: t('onboarding.coach_step3_desc'), hint: t('onboarding.coach_step3_hint') },
    { title: t('onboarding.coach_step4_title'), desc: t('onboarding.coach_step4_desc'), hint: t('onboarding.coach_step4_hint') },
    { title: t('onboarding.coach_step5_title'), desc: t('onboarding.coach_step5_desc'), hint: t('onboarding.coach_step5_hint') },
  ]);

  let currentStep = $state(0);
  let isLast = $derived(currentStep === steps.length - 1);

  function next() {
    if (!isLast) currentStep++;
    else finish();
  }

  function prev() {
    if (currentStep > 0) currentStep--;
  }

  function finish() {
    try {
      localStorage.setItem('harness_tour_dismissed', 'true');
    } catch {
      // storage unavailable: the tour may show again next launch
    }
    onClose();
  }
</script>

<Dialog.Root open={true} onOpenChange={(open) => !open && finish()}>
  <Dialog.Content class="sm:max-w-md" showCloseButton>
    <Dialog.Header>
      <div>
        <Badge variant="outline" class="font-num">
          {t('onboarding.coach_badge', { current: currentStep + 1, total: steps.length })}
        </Badge>
      </div>
      <Dialog.Title class="text-base">{steps[currentStep].title}</Dialog.Title>
      <Dialog.Description class="leading-relaxed">{steps[currentStep].desc}</Dialog.Description>
    </Dialog.Header>

    <div class="rounded-md border border-border bg-muted/40 px-3 py-2 text-xs leading-relaxed text-muted-foreground">
      {steps[currentStep].hint}
    </div>

    <div class="flex items-center justify-between gap-3">
      <div class="flex items-center gap-1.5" aria-hidden="true">
        {#each steps as _, idx (idx)}
          <span class={cn('h-1.5 rounded-full transition-all duration-150', idx === currentStep ? 'w-4 bg-primary' : 'w-1.5 bg-border')}></span>
        {/each}
      </div>
      <div class="flex items-center gap-1.5">
        {#if currentStep > 0}
          <Button variant="ghost" size="sm" onclick={prev}>{t('onboarding.coach_prev')}</Button>
        {/if}
        {#if !isLast}
          <Button variant="ghost" size="sm" onclick={finish}>{t('onboarding.coach_skip')}</Button>
        {/if}
        <Button size="sm" onclick={next}>
          {isLast ? t('onboarding.coach_finish') : t('onboarding.coach_next')}
        </Button>
      </div>
    </div>
  </Dialog.Content>
</Dialog.Root>
