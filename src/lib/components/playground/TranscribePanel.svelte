<script lang="ts">
  import Field from '../dashboard/parts/Field.svelte';
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  import { t, locale } from '$lib/i18n.svelte';
  import { invoke } from '$lib/ipc';
  import { toast } from 'svelte-sonner';
  import { localizeError } from '$lib/network';
  import type { GatewayOverview } from '$lib/types';
  import { app } from '$lib/stores/app.svelte';
  import { domain } from '$lib/stores/domain.svelte';
  import { libraryDefinitions } from '$lib/stores/downloads.svelte';
  import { Button } from '$lib/components/ui/button';
  import EngineNeeded from '../common/EngineNeeded.svelte';
  import { familyFromError } from '$lib/engines';
  import { Badge } from '$lib/components/ui/badge';
  import { Input } from '$lib/components/ui/input';
  import { Label } from '$lib/components/ui/label';
  import { formatBytes } from '$lib/format';
  import { cn } from '$lib/utils';
  import { isMac } from '$lib/platform';
  import { holdReleaseWhile } from '$lib/windowActivity';
  import { AudioLines, Compass, Copy, FileAudio, LoaderCircle, MessageSquarePlus, Mic, Power, Square, TriangleAlert, Upload, X } from '@lucide/svelte';
  import { Progress } from '$lib/components/ui/progress';
  import { Download } from '@lucide/svelte';
  import { onDestroy, onMount } from 'svelte';
  import { fetchTranscription, isSpeechDefinition, isOnnxCtcDefinition, type AudioFormat, type AudioTask, type TranscribeResult } from './transcribe';
  import { MAX_RECORD_SECONDS, RecordError } from './recorder';
  import { createRecordingSession, recordingClock as clock } from './recordingSession';
  import { speech } from './speech.svelte';
  import { isSherpaChoice, speechModel } from './speechSetup';
  import { languagesText } from './asr';
  import { asr } from './asr.svelte';
  import { tts } from './tts.svelte';

  /** Speech definitions: the gateway loads the selected model on the first request (JIT). */
  let models = $derived(
    domain.definitions.filter(isSpeechDefinition),
  );
  /** Installed sherpa-onnx recognition models (Parakeet, SenseVoice...): run per request, no definition. */
  let sherpaModels = $derived(asr.models);
  let sherpaChosen = $derived(isSherpaChoice(speech.model, domain.definitions, asr.names));
  let ctcChosen = $derived(models.some(d => d.alias === speech.model && isOnnxCtcDefinition(d)));
  let noModel = $derived(models.length === 0 && sherpaModels.length === 0);
  let noModelNotice = $derived(noModel && !libraryDefinitions.busy);
  let installingRuntime = $derived(tts.installingRuntime);
  let progressPct = $derived(
    asr.modelProgress && asr.modelProgress.total_bytes > 0 ? (asr.modelProgress.downloaded_bytes / asr.modelProgress.total_bytes) * 100 : 0,
  );
  const more = (list: string, count: number) => t('sasr.languages_more', { list, count });

  onMount(() => void asr.load());
  // sherpa-onnx models only transcribe; translation to English is a Whisper task.
  $effect(() => {
    if (sherpaChosen || ctcChosen) task = 'transcriptions';
  });

  async function installRuntime() {
    if (await tts.installRuntime()) await asr.load();
  }
  async function installModel(id: string) {
    const name = await asr.installModel(id);
    if (name) speech.model = name;
  }
  let task = $state<AudioTask>('transcriptions');
  let format = $state<AudioFormat>('json');
  let language = $state('');
  let prompt = $state('');
  let file = $state<File | null>(null);
  let busy = $state(false);
  let error = $state('');
  /** Engine family the failed run was missing; shown with its install button. */
  let needFamily = $state<string | null>(null);
  let result = $state<TranscribeResult | null>(null);
  let controller: AbortController | null = null;
  let recState = $state<'idle' | 'starting' | 'recording'>('idle');
  // A recording in progress, recorded/picked audio not transcribed yet or a transcript on
  // screen would be lost with the page: keep it while hidden to the tray (the request itself
  // counts as a page request).
  $effect(() => holdReleaseWhile('transcribe-audio', () => recState !== 'idle' || !!file || !!result));

  $effect(() => {
    speech.model = speechModel(speech.model, domain.definitions, asr.names);
  });
  $effect(() => {
    void domain.refreshDefinitions();
  });

  let canRun = $derived(!busy && recState !== 'recording' && app.gatewayRunning && !!file && !!speech.model);

  async function run() {
    if (!canRun || !file) return;
    controller?.abort();
    const c = new AbortController();
    controller = c;
    busy = true;
    error = '';
    needFamily = null;
    try {
      result = await fetchTranscription(app.inferenceUrl, task, file, speech.model, { language: ctcChosen ? '' : language, prompt: ctcChosen ? '' : prompt, format }, app.inferenceHeaders(), c.signal);
    } catch (e) {
      if (!c.signal.aborted) {
        const msg = e instanceof Error ? e.message : String(e);
        // The gateway passes the load failure through as raw keys ("…: runtime.family_not_installed:
        // whisper.cpp"); say what is missing and where to install it instead.
        needFamily = familyFromError(msg);
        error = needFamily ? '' : msg;
        result = null;
      }
    } finally {
      if (controller === c) {
        controller = null;
        busy = false;
      }
    }
  }

  const ACCEPT = 'audio/*,video/webm,video/mp4,.wav,.mp3,.flac,.ogg,.opus,.m4a,.webm';
  let fileInput = $state<HTMLInputElement | null>(null);
  let dragging = $state(false);

  function pick(e: Event) {
    file = (e.currentTarget as HTMLInputElement).files?.[0] ?? null;
  }

  // ---- Microphone: PCM captured in the browser, encoded to a 16 kHz WAV (no ffmpeg needed) ----
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
      result = null;
      error = '';
      needFamily = null;
      if (fileInput) fileInput.value = '';
    },
    error: (e) => {
      recError = e instanceof RecordError && e.code !== 'failed' ? t(`asr.rec_${e.code}`) : t('asr.rec_failed', { error: localizeError(e) });
    },
  });
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
    if (busy) return;
    const dropped = e.dataTransfer?.files?.[0];
    if (dropped) file = dropped;
  }

  let enablingGateway = $state(false);
  /** Same call as the Settings > Gateway switch: the stored settings with `enabled` on. */
  async function enableGateway() {
    enablingGateway = true;
    try {
      const current = app.gateway ?? (await invoke<GatewayOverview>('get_gateway_status'));
      const next = await invoke<GatewayOverview>('set_gateway_settings', {
        settings: { ...current.settings, enabled: true },
      });
      app.setGateway(next);
      toast.success(t('asr.gateway_on'));
    } catch (e) {
      toast.error(t('asr.gateway_on_failed', { error: localizeError(e) }));
    } finally {
      enablingGateway = false;
    }
  }

  async function copy() {
    if (result) await navigator.clipboard.writeText(result.text.trim());
  }
</script>

<div class="mx-auto flex w-full max-w-[860px] flex-col gap-5 px-4 py-6">
  {#if isMac && !sherpaChosen && !ctcChosen}
    <!-- whisper.cpp ships no macOS server build: the Hub / gateway hints below only help once
         a self-built whisper-server is added as a local runtime. -->
    <div class="flex items-start gap-2 rounded-md border border-info/30 bg-info/10 px-3 py-2 text-xs text-foreground" role="note">
      <TriangleAlert class="mt-px size-4 shrink-0 text-info" aria-hidden="true" />
      <span class="min-w-0 flex-1">{t('asr.macos_note')}</span>
    </div>
  {/if}
  {#if !app.gatewayRunning}
    <div class="flex items-start gap-2 rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-foreground" role="status">
      <TriangleAlert class="mt-px size-4 shrink-0 text-warning" aria-hidden="true" />
      <span class="min-w-0 flex-1">{t('asr.need_gateway')}</span>
      <Button variant="outline" size="sm" class="shrink-0" disabled={enablingGateway} onclick={enableGateway}>
        {#if enablingGateway}<LoaderCircle class="animate-spin" />{:else}<Power />{/if}
        {t('asr.enable_gateway')}
      </Button>
    </div>
  {/if}
  {#if noModelNotice}
    <div class="flex items-start gap-2 rounded-md border border-border bg-muted/40 px-3 py-2 text-xs text-foreground" role="status">
      <TriangleAlert class="mt-px size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
      <span class="min-w-0 flex-1">{t('sasr.no_model')}</span>
      <Button
        variant="outline"
        size="sm"
        class="shrink-0"
        onclick={() => {
          app.requestedHubSearch = 'whisper';
          app.requestedTab = 'hub';
        }}
      >
        <Compass />{t('asr.find_model')}
      </Button>
    </div>
  {/if}

  <section class="flex flex-col gap-3" aria-labelledby="asr-input">
    <h3 id="asr-input" class="sr-only">{t('asr.title')}</h3>
    <div class="flex flex-col gap-1.5">
      <Label for="asr-model" class="text-sm font-semibold text-foreground">{t('asr.model_label')}</Label>
      <select
        id="asr-model"
        bind:value={speech.model}
        disabled={busy || noModel}
        class="h-9 rounded-md border border-input bg-background px-2 text-sm text-foreground"
      >
        {#if sherpaModels.length > 0}
          <optgroup label={t('sasr.group_definitions')}>
            {#each models as d (d.id)}
              <option value={d.alias}>{d.alias}</option>
            {/each}
          </optgroup>
          <optgroup label={t('sasr.group_sherpa')}>
            {#each sherpaModels as m (m.id)}
              <option value={m.name}>{m.name}</option>
            {/each}
          </optgroup>
        {:else}
          {#each models as d (d.id)}
            <option value={d.alias}>{d.alias}</option>
          {/each}
        {/if}
      </select>
    </div>
    <Field label={t('asr.file_label')} id="asr-file" note={t('asr.file_desc')}>
      <!-- The real input stays in the DOM (hidden) so the picker and tests can set files. -->
      <input id="asr-file" bind:this={fileInput} type="file" accept={ACCEPT} onchange={pick} disabled={busy} class="sr-only" tabindex="-1" />
      <div
        role="group"
        aria-label={t('asr.file_label')}
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
    </Field>
    <div class="grid gap-3 sm:grid-cols-3">
      <div class="flex flex-col gap-1.5">
        <Label for="asr-task" class="text-sm font-semibold text-foreground">{t('asr.task_label')}</Label>
        <select id="asr-task" bind:value={task} disabled={busy || sherpaChosen || ctcChosen} class="h-9 rounded-md border border-input bg-background px-2 text-sm text-foreground">
          <option value="transcriptions">{t('asr.task_transcribe')}</option>
          <option value="translations">{t('asr.task_translate')}</option>
        </select>
        {#if sherpaChosen || ctcChosen}<span class="text-xs text-muted-foreground">{t('sasr.only_transcribes')}</span>{/if}
      </div>
      <div class="flex flex-col gap-1.5">
        <Label for="asr-format" class="text-sm font-semibold text-foreground">{t('asr.format_label')}</Label>
        <select id="asr-format" bind:value={format} disabled={busy} class="h-9 rounded-md border border-input bg-background px-2 text-sm text-foreground">
          <option value="json">{t('asr.format_text')}</option>
          <option value="srt">SRT</option>
          <option value="vtt">WebVTT</option>
        </select>
      </div>
      {#if !ctcChosen}
      <div class="flex flex-col gap-1.5">
        <Label for="asr-lang" class="text-sm font-semibold text-foreground">{t('asr.language_label')}</Label>
        <Input id="asr-lang" bind:value={language} placeholder={t('asr.language_placeholder')} disabled={busy} />
      </div>
      {/if}
    </div>
    {#if !ctcChosen}
      <div class="flex flex-col gap-1.5">
        <Label for="asr-prompt" class="text-sm font-semibold text-foreground">{t('asr.prompt_label')}</Label>
        <Input id="asr-prompt" bind:value={prompt} placeholder={t('asr.prompt_placeholder')} disabled={busy} />
      </div>
    {/if}
    <div class="flex items-center gap-3">
      <Button size="sm" onclick={run} disabled={!canRun}>
        <AudioLines />
        {busy ? t('asr.running') : t('asr.run')}
      </Button>
      {#if !file}
        <span class="text-xs text-muted-foreground">{t('asr.need_file')}</span>
      {/if}
    </div>
  </section>

  {#if asr.overview && asr.overview.catalog.length > 0}
    <section class="flex flex-col gap-2" aria-labelledby="sasr-catalog">
      <h4 id="sasr-catalog" class="text-sm font-semibold text-foreground">{t('sasr.catalog_title')}</h4>
      <p class="text-xs text-muted-foreground">{t('sasr.catalog_desc')}</p>
      {#if !asr.overview.runtime_installed}
        <div class="flex items-start gap-2 rounded-md border border-border bg-muted/40 px-3 py-2 text-xs text-foreground" role="status">
          <TriangleAlert class="mt-px size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
          <span class="min-w-0 flex-1">{t('sasr.no_runtime')}</span>
          <Button variant="outline" size="sm" class="shrink-0" disabled={installingRuntime} onclick={installRuntime}>
            {#if installingRuntime}<LoaderCircle class="animate-spin" />{:else}<Download />{/if}
            {installingRuntime ? t('speak.installing') : t('speak.install_runtime')}
          </Button>
        </div>
      {/if}
      <ul class="flex flex-col gap-2">
        {#each asr.overview.catalog as row (row.id)}
          <li class="flex flex-wrap items-center gap-3 rounded-md border border-border px-3 py-2">
            <div class="flex min-w-0 flex-1 flex-col gap-0.5">
              <span class="flex flex-wrap items-center gap-2 text-sm font-medium text-foreground">
                {row.title}
                {#if row.recommended}<Badge variant="outline">{t('sasr.recommended')}</Badge>{/if}
              </span>
              <span class="text-xs text-muted-foreground">
                {languagesText(row.languages, more)} · {formatBytes(row.size_bytes)} · {t('sasr.license', { license: row.license })}
              </span>
              {#if asr.installingModel === row.id && asr.modelProgress}
                <Progress value={progressPct} class="mt-1" aria-label={t('sasr.installing')} />
              {/if}
            </div>
            {#if row.installed}
              <Badge variant="secondary">{t('sasr.installed')}</Badge>
            {:else}
              <Button variant="outline" size="sm" disabled={!!asr.installingModel} onclick={() => installModel(row.id)}>
                {#if asr.installingModel === row.id}<LoaderCircle class="animate-spin" />{:else}<Download />{/if}
                {asr.installingModel === row.id ? t('sasr.installing') : t('sasr.install')}
              </Button>
            {/if}
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  {#if needFamily}
    <div class="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive [overflow-wrap:anywhere]" role="alert">
      <EngineNeeded family={needFamily} onInstalled={() => (needFamily = null)} />
    </div>
  {:else if error}
    <div class="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive [overflow-wrap:anywhere]" role="alert">
      {error}
    </div>
  {/if}

  {#if result}
    <section class="flex flex-col gap-3" aria-labelledby="asr-result">
      <div class="flex flex-wrap items-center gap-2">
        <h3 id="asr-result" class="text-sm font-semibold text-foreground">{t('asr.result_title')}</h3>
        <AdvancedOnly><span class="font-num text-xs text-muted-foreground">{t('emb.stat_time', { value: Math.round(result.ms) })}</span></AdvancedOnly>
        {#if result.language}<Badge variant="outline">{result.language.charAt(0).toUpperCase() + result.language.slice(1)}</Badge>{/if}
        {#if result.duration !== null}<Badge variant="outline" class="font-num">{t('asr.stat_duration', { value: result.duration.toLocaleString(locale(), { minimumFractionDigits: 1, maximumFractionDigits: 1 }) })}</Badge>{/if}
        <Button variant="ghost" size="sm" onclick={copy}><Copy />{t('asr.copy')}</Button>
        <Button variant="ghost" size="sm" onclick={() => result && app.addToChat(result.text.trim())} disabled={!result.text.trim()}>
          <MessageSquarePlus />{t('asr.to_chat')}
        </Button>
      </div>
      {#if result.segments?.length}
        <div class="flex flex-col gap-2">
          {#each result.segments as segment}
            <div class="flex gap-3 text-sm">
              <span class="font-num shrink-0 text-muted-foreground">{segment.start.toFixed(2)}–{segment.end.toFixed(2)}</span>
              <span class="whitespace-pre-wrap">{segment.text}</span>
            </div>
          {/each}
        </div>
      {/if}
      <pre class="whitespace-pre-wrap rounded-lg border border-border bg-muted/30 p-3 text-sm text-foreground [overflow-wrap:anywhere]">{result.text.trim()}</pre>
    </section>
  {/if}
</div>
