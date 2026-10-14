<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import { FileAudio, LoaderCircle, Mic, Square, Upload, X } from '@lucide/svelte';
  import { Button } from '#lib/components/ui/button/index.js';
  import { t } from '#lib/i18n.svelte.js';
  import { cn } from '#lib/utils.js';
  import { formatBytes } from '#lib/format.js';
  import { holdReleaseWhile } from '#lib/windowActivity.js';
  import { localizeError } from '#lib/network.js';
  import { MAX_RECORD_SECONDS, RecordError, startRecording } from './recorder';
  import { createRecordingSession, recordingClock as clock, type RecordingState } from './recordingSession';
  let { id, label, accept, file = $bindable(null), recState = $bindable('idle'), busy = false, sampleRate = 16000, wavOnly = false, contextKey = '', onRecorded }: {
    id: string; label: string; accept: string; file?: File | null; recState?: RecordingState; busy?: boolean;
    sampleRate?: number; wavOnly?: boolean; contextKey?: string; onRecorded?: () => void;
  } = $props();
  let fileInput = $state<HTMLInputElement | null>(null);
  let dragging = $state(false);

  function pick(e: Event) {
    if (busy || recState !== 'idle') return;
    const picked = (e.currentTarget as HTMLInputElement).files?.[0];
    if (picked) acceptFile(picked);
  }

  // ---- Microphone: PCM captured in the browser, encoded to a WAV at the selected sample rate (no ffmpeg needed) ----
  let recSeconds = $state(0);
  let recLevel = $state(0);
  let recError = $state('');
  const microphone = createRecordingSession({
    state: (state) => {
      recState = state;
      if (state === 'idle') recLevel = 0;
    },
    seconds: (seconds) => (recSeconds = seconds),
    level: (peak) => (recLevel = Math.max(peak, recLevel * 0.85)),
    file: (audio) => {
      file = audio;
      onRecorded?.();
      if (fileInput) fileInput.value = '';
    },
    error: (e) => {
      recError = e instanceof RecordError && e.code !== 'failed' ? t(`asr.rec_${e.code}`) : t('asr.rec_failed', { error: localizeError(e) });
    },
  }, (level, limit) => startRecording(level, limit, sampleRate));
  function record() {
    if (busy) return;
    recError = '';
    void microphone.record();
  }
  function finishRecording() { microphone.finish(); }
  onDestroy(() => microphone.cancel());

  function clearFile() {
    file = null;
    if (fileInput) fileInput.value = '';
  }

  function drop(e: DragEvent) {
    e.preventDefault();
    dragging = false;
    if (busy || recState !== 'idle') return;
    const dropped = e.dataTransfer?.files?.[0];
    if (dropped) acceptFile(dropped);
  }

  function acceptFile(picked: File) {
    if (wavOnly && !picked.name.toLowerCase().endsWith('.wav')) { recError = t('process.limits'); return; }
    recError = '';
    file = picked;
  }
  $effect(() => holdReleaseWhile(`audio-picker-${id}`, () => recState !== 'idle'));
  $effect(() => { void contextKey; untrack(() => { microphone.cancel(); recError = ''; }); });
</script>
<!-- The real input stays in the DOM (hidden) so the picker and tests can set files. -->
<input {id} bind:this={fileInput} type="file" {accept} onchange={pick} disabled={busy} class="sr-only" tabindex="-1" />
<div
  role="group"
  aria-label={label}
  ondragover={(e) => {
    e.preventDefault();
    if (!busy) dragging = true;
  }}
  ondragleave={() => (dragging = false)}
  ondrop={drop}
  class={cn(
    'flex min-h-14 flex-wrap items-center gap-3 rounded-md border border-dashed px-3 py-2 transition-colors',
    dragging ? 'border-foreground/40 bg-muted/50' : 'border-input bg-background',
  )}
>
  {#if recState === 'recording'}
    <Button variant="destructive" size="sm" onclick={finishRecording}>
      <Square />{t('asr.rec_stop')}
    </Button>
    <span class="size-2 shrink-0 animate-pulse rounded-full bg-destructive" aria-hidden="true"></span>
    <span class="font-num text-sm text-foreground" role="timer" aria-label={t('asr.rec_elapsed')}>
      {clock(recSeconds)} / {clock(MAX_RECORD_SECONDS)}
    </span>
    <div
      class="h-2 min-w-24 flex-1 overflow-hidden rounded-full bg-muted"
      role="meter"
      aria-label={t('asr.rec_level')}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(Math.min(1, recLevel) * 100)}
    >
      <div class="h-full rounded-full bg-muted-foreground transition-[width] duration-100" style:width="{Math.round(Math.min(1, recLevel * 1.5) * 100)}%"></div>
    </div>
  {:else}
  <Button variant="outline" size="sm" onclick={() => fileInput?.click()} disabled={busy || recState !== 'idle'}>
    <Upload />{file ? t('asr.file_change') : t('asr.file_choose')}
  </Button>
  <Button variant="outline" size="sm" onclick={record} disabled={busy || recState !== 'idle'}>
    {#if recState === 'starting'}<LoaderCircle class="animate-spin" />{:else}<Mic />{/if}
    {t('asr.rec_start')}
  </Button>
  {#if file}
    <FileAudio class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
    <span class="min-w-0 flex-1 truncate text-sm text-foreground" title={file.name}>{file.name}</span>
    <span class="font-num text-xs text-muted-foreground">{formatBytes(file.size)}</span>
    <Button variant="ghost" size="icon-sm" onclick={clearFile} disabled={busy} aria-label={t('asr.file_clear')} title={t('asr.file_clear')}>
      <X />
    </Button>
  {:else}
    <span class="min-w-0 flex-1 text-sm text-muted-foreground">{t('asr.file_drop')}</span>
  {/if}
  {/if}
</div>
{#if recError}
  <p class="text-xs text-destructive" role="alert">{recError}</p>
{/if}
