<script lang="ts">
  import { t } from '#lib/i18n.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import { slotColor } from '#lib/serverColor.js';
  import { segmentPosition, segmentTime, speakerLanes, type DiarizationResult } from './diarization.js';

  let { result, onCopy, onDownload }: { result: DiarizationResult; onCopy: () => void; onDownload: () => void } = $props();
  const lanes = $derived(speakerLanes(result.segments));
  const hasConfidence = $derived(result.segments.some(s => s.confidence !== undefined));
</script>

<section class="flex flex-col gap-3 rounded-md border p-3" aria-labelledby="diarization-result-title">
  <div class="flex flex-wrap items-center justify-between gap-2">
    <h3 id="diarization-result-title" class="text-sm font-semibold">{t('process.diarization')}</h3>
    <span class="text-sm text-muted-foreground">{t('process.speaker_count', { count: result.speakers })}</span>
  </div>
  <p class="break-all text-xs text-muted-foreground">{result.model}</p>
  <div class="flex flex-wrap gap-2">
    <Button variant="outline" size="sm" onclick={onCopy}>{t('process.copy_json')}</Button>
    <Button variant="outline" size="sm" onclick={onDownload}>{t('process.download_json')}</Button>
  </div>
  <h4 class="text-sm font-medium">{t('process.timeline')}</h4>
  <div class="flex flex-col gap-2" role="group" aria-label={t('process.timeline')}>
    {#each lanes as lane, index (lane.id)}
      <div class="flex items-center gap-3" role="group" aria-label={t('process.speaker_label', { speaker: lane.speaker })}>
        <span class="w-24 shrink-0 truncate text-xs" title={lane.id}>{t('process.speaker_label', { speaker: lane.speaker })}</span>
        <div class="relative h-6 min-w-0 flex-1 rounded bg-muted">
          {#each lane.segments as segment}
            {@const position = segmentPosition(segment, result.duration)}
            {@const label = t('process.segment_label', { speaker: segment.speaker, start: segmentTime(segment.start), end: segmentTime(segment.end) })}
            <div class="absolute inset-y-0 rounded-sm" style:left={`${position.left}%`} style:width={`${position.width}%`} style:background={slotColor(index)} role="img" aria-label={label} title={label}></div>
          {/each}
        </div>
      </div>
    {/each}
    <div class="flex justify-between pl-[108px] font-num text-xs text-muted-foreground"><span>{segmentTime(0)}</span><span>{segmentTime(result.duration)}</span></div>
  </div>
  {#if !result.segments.length}
    <p class="text-sm text-muted-foreground">{t('process.no_segments')}</p>
  {:else}
    <div class="overflow-x-auto">
      <table class="w-full text-left text-sm">
        <caption class="sr-only">{t('process.segments')}</caption>
        <thead class="text-muted-foreground"><tr class="border-b"><th scope="col" class="px-2 py-2">{t('process.time_range')}</th><th scope="col" class="px-2 py-2">{t('process.speaker')}</th>{#if hasConfidence}<th scope="col" class="px-2 py-2">{t('process.confidence')}</th>{/if}</tr></thead>
        <tbody>
          {#each result.segments as segment}
            <tr class="border-b last:border-0">
              <td class="whitespace-nowrap px-2 py-2 font-num">{segmentTime(segment.start)}–{segmentTime(segment.end)}</td>
              <td class="px-2 py-2">{t('process.speaker_label', { speaker: segment.speaker })}</td>
              {#if hasConfidence}<td class="px-2 py-2 font-num">{segment.confidence === undefined ? t('process.confidence_missing') : `${Math.round(segment.confidence * 100)}%`}</td>{/if}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>
