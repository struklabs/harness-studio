<script lang="ts">
  import { t } from '#lib/i18n.svelte.js';
  import type { GeneratorProgress } from './generatorProgress';
  let { progress }: { progress: GeneratorProgress } = $props();
</script>

<span data-generation-status class="flex flex-wrap gap-x-1">
  {t('create.attempt_number', { number: progress.number })} ·
  {t(progress.stage === 'testing' ? 'create.progress_testing' : progress.stage === 'retrying' ? 'create.progress_retrying' : progress.stage === 'complete' ? 'create.stage_test' : 'create.progress_writing')}
  {#if progress.result}
    · {t('create.progress_cases', progress.result)}
  {/if}
  {#if progress.number > 1 && progress.stage === 'writing'}
    · {t('create.progress_retrying')}
  {/if}
</span>
