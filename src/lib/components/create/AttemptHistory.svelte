<script lang="ts">
  import { localizeError } from '#lib/network.js';
  import { t, locale } from '#lib/i18n.svelte.js';
  import { showRepetitionWarning } from './generatorProgress';
  import type { GeneratorAttempt } from '#lib/tools-client.js';
  let { history }: { history: GeneratorAttempt[] } = $props();
  const fmt = (n: number) => Math.round(n).toLocaleString(locale());
</script>
{#if history.length}
  <div data-attempt-history class="space-y-2 text-xs">
    <p class="font-medium">{t('create.attempt_history')}</p>
    {#each history as attempt, i (i)}
      <div class="rounded-md border border-border p-2">
        <p class="font-num">{t('create.attempt_number', { number: attempt.number })} · {t('create.failed_cases', { count: attempt.failed_cases })} · {attempt.usage.estimated ? '≈ ' : ''}↑{fmt(attempt.usage.prompt_tokens)} ↓{fmt(attempt.usage.completion_tokens)}</p>
        <p class="mt-1 break-words">{attempt.change || t('create.change_missing')}</p>
        {#if attempt.feedback}<details class="mt-1"><summary class="cursor-pointer text-muted-foreground">{t('create.attempt_feedback')}</summary><pre class="mt-1 max-h-32 overflow-auto whitespace-pre-wrap break-words">{localizeError(attempt.feedback)}</pre></details>{/if}
        {#if showRepetitionWarning(attempt)}<p class="text-warning">{t('create.repeated')}</p>{/if}
        {#if attempt.strategy !== 'repair'}<p>{t('create.strategy')}: {attempt.strategy === 'test_first' ? t('create.test_first') : t('create.rewrite')}</p>{/if}
        {#if attempt.compacted}<p>{t('create.context_compacted')}</p>{/if}
      </div>
    {/each}
  </div>
{/if}
