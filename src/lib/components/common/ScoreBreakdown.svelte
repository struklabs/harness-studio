<script lang="ts">
  // The four model scores (hardware fit, task fit, quality, speed) as bars, with the reasons
  // behind each one under "Show Reasons". Used in Model Hub, Library and Performance.
  import type { Scores, ScoreReason } from '#lib/modelScores.js';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { Progress } from '#lib/components/ui/progress/index.js';

  interface Props {
    scores: Scores;
    /** One line of numbers; the bars and reasons open on request. */
    compact?: boolean;
  }
  let { scores, compact = false }: Props = $props();

  const rows = [
    ['hardware', 'ms.hardware'],
    ['task', 'ms.task'],
    ['quality', 'ms.quality'],
    ['speed', 'ms.speed'],
  ] as const;

  const text = (r: ScoreReason) => t(r.key as TKey, r.params);
  const num = (v: number | null) => (v === null ? '–' : String(v));
  let summary = $derived(rows.map(([k, label]) => `${t(label)} ${num(scores[k].value)}`).join(' · '));
</script>

{#snippet bars()}
  <ul class="flex flex-col gap-2" data-testid="score-breakdown">
    {#each rows as [key, label]}
      {@const s = scores[key]}
      <li class="flex flex-col gap-1" data-score={key}>
        <div class="flex items-center justify-between gap-2 text-xs">
          <span class="font-medium">{t(label)}</span>
          <span class="font-num text-muted-foreground">
            {s.value === null ? t('ms.no_data') : s.value}
            {#if s.value !== null}<span class="ml-1">· {s.measured ? t('ms.measured') : t('ms.estimated')}</span>{/if}
          </span>
        </div>
        <Progress value={s.value ?? 0} class="h-1.5" aria-label={t(label)} />
        <ul class="flex flex-col gap-0.5 text-xs text-muted-foreground">
          {#each s.reasons as reason}<li>{text(reason)}</li>{/each}
        </ul>
      </li>
    {/each}
  </ul>
{/snippet}

{#if compact}
  <details class="text-xs" data-testid="score-details">
    <summary class="cursor-pointer select-none text-muted-foreground" title={t('ms.show_reasons')} aria-label={t('ms.summary_label')}>{summary}</summary>
    <div class="mt-2">{@render bars()}</div>
  </details>
{:else}
  {@render bars()}
{/if}
