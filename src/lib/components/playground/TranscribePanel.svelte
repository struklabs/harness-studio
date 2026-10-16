<script lang="ts">
  import Field from '../dashboard/parts/Field.svelte';
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  import { t, locale } from '#lib/i18n.svelte.js';
  import { invoke } from '#lib/ipc.js';
  import { toast } from 'svelte-sonner';
  import { localizeError } from '#lib/network.js';
  import type { GatewayOverview } from '#lib/types.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { domain } from '#lib/stores/domain.svelte.js';
  import { libraryDefinitions } from '#lib/stores/downloads.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import EngineNeeded from '../common/EngineNeeded.svelte';
  import { familyFromError } from '#lib/engines.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import { formatBytes } from '#lib/format.js';
  import { isMac } from '#lib/platform.js';
  import { holdReleaseWhile } from '#lib/windowActivity.js';
  import { AudioLines, Compass, Copy, LoaderCircle, MessageSquarePlus, Power, TriangleAlert } from '@lucide/svelte';
  import { Progress } from '#lib/components/ui/progress/index.js';
  import { Download } from '@lucide/svelte';
  import { onDestroy, untrack } from 'svelte';
  import { fetchTranscription, isTranscriptionDefinition, isAudioLlmDefinition, audioLlmTranslates, isOnnxCtcDefinition, crispasrCanTranslate, isCrispasrDefinition, isTranscribeCppDefinition, transcribeCppCanTranslate, type AudioFormat, type AudioTask, type TranscribeResult } from './transcribe';
  import AudioFilePicker from './AudioFilePicker.svelte';
  import { speech } from './speech.svelte';
  import { isSherpaChoice, speechModel } from './speechSetup';
  import { languagesText } from './asr';
  import { asr } from './asr.svelte';
  import { tts } from './tts.svelte';
  import { library } from '#lib/stores/library.svelte.js';

  /** Speech definitions and llama.cpp chat models that hear: the gateway loads the selected model on
   *  the first request (JIT). */
  let models = $derived(
    domain.definitions.filter(d => isTranscriptionDefinition(d, library.models)),
  );
  /** Installed sherpa-onnx recognition models (Parakeet, SenseVoice...): run per request, no definition. */
  let sherpaModels = $derived(asr.models);
  let sherpaChosen = $derived(isSherpaChoice(speech.model, domain.definitions, asr.names));
  let ctcChosen = $derived(models.some(d => d.alias === speech.model && isOnnxCtcDefinition(d)));
  /** The library's mlx-audio recognition models (Whisper, Parakeet in MLX's format), by definition alias. */
  let engineModels = $derived(asr.engineModels);
  let engineChosen = $derived(engineModels.find(m => m.name === speech.model) ?? null);
  /** A llama.cpp chat model that hears (LFM2.5-Audio, Qwen3-ASR...): only some of them translate. */
  let audioLlm = $derived(models.find(d => d.alias === speech.model && isAudioLlmDefinition(d, library.models)) ?? null);
  /** A model that only transcribes (sherpa-onnx, CTC, Parakeet MLX, LFM2.5-Audio, Qwen3-ASR): translation is
   *  Whisper's (and some chat models' that hear). */
  let onlyTranscribes = $derived(sherpaChosen || ctcChosen || (engineChosen !== null && !engineChosen.translates) || (audioLlm !== null && !audioLlmTranslates(audioLlm, library.models)));
  let crispChosen = $derived(models.some(d => d.alias === speech.model && isCrispasrDefinition(d)));
  let transcribeCppChosen = $derived(models.some(d => d.alias === speech.model && isTranscribeCppDefinition(d)));
  let crispNoTranslate = $derived(models.some(d => d.alias === speech.model && ((isCrispasrDefinition(d) && !crispasrCanTranslate(d, library.models)) || (isTranscribeCppDefinition(d) && !transcribeCppCanTranslate(d, library.models)))));
  let noModel = $derived(models.length === 0 && sherpaModels.length === 0 && engineModels.length === 0);
  let noModelNotice = $derived(noModel && !libraryDefinitions.busy);
  let installingRuntime = $derived(tts.installingRuntime);
  let progressPct = $derived(
    asr.modelProgress && asr.modelProgress.total_bytes > 0 ? (asr.modelProgress.downloaded_bytes / asr.modelProgress.total_bytes) * 100 : 0,
  );
  const more = (list: string, count: number) => t('sasr.languages_more', { list, count });

  $effect(() => {
    void library.version;
    untrack(() => {
      void asr.load();
      void library.refresh();
    });
  });
  // sherpa-onnx models only transcribe; translation to English is a Whisper task.
  $effect(() => {
    if (onlyTranscribes || crispNoTranslate) task = 'transcriptions';
  });
  // mlx-audio's worker answers the text only (no subtitles).
  $effect(() => {
    if (engineChosen && format !== 'json') format = 'json';
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
  // The UI's language: naming it saves whisper a language detection pass (on a CPU build as long
  // as the transcription itself). Cleared, whisper detects the language.
  const uiLanguage = locale().slice(0, 2);
  let language = $state(uiLanguage);
  /** The language sent: the prefilled UI language goes to whisper only; other recognition models
   *  (sherpa-onnx, CrispASR, transcribe.cpp, MLX) keep detecting unless the user types one
   *  (an English-only transcribe.cpp model refuses any other language). */
  let sentLanguage = $derived(
    ctcChosen ? '' : language === uiLanguage && (sherpaChosen || crispChosen || transcribeCppChosen || engineChosen || audioLlm) ? '' : language
  );
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
    speech.model = speechModel(speech.model, domain.definitions, asr.names, asr.engineNames, library.models);
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
      result = await fetchTranscription(app.inferenceUrl, task, file, speech.model, { language: sentLanguage, prompt: ctcChosen || engineChosen ? '' : prompt, format, textOnly: !!engineChosen }, app.inferenceHeaders(), c.signal);
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
  {#if isMac && !sherpaChosen && !ctcChosen && !engineChosen && !crispChosen}
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
        {#if sherpaModels.length > 0 || engineModels.length > 0}
          {#if models.length > 0}
            <optgroup label={t('sasr.group_definitions')}>
              {#each models as d (d.id)}
                <option value={d.alias}>{d.alias}</option>
              {/each}
            </optgroup>
          {/if}
          {#if engineModels.length > 0}
            <optgroup label={t('sasr.group_mlx')}>
              {#each engineModels as m (m.id)}
                <option value={m.name}>{m.name}</option>
              {/each}
            </optgroup>
          {/if}
          {#if sherpaModels.length > 0}
            <optgroup label={t('sasr.group_sherpa')}>
              {#each sherpaModels as m (m.id)}
                <option value={m.name}>{m.name}</option>
              {/each}
            </optgroup>
          {/if}
        {:else}
          {#each models as d (d.id)}
            <option value={d.alias}>{d.alias}</option>
          {/each}
        {/if}
      </select>
    </div>
    <Field label={t('asr.file_label')} id="asr-file" note={t('asr.file_desc')}>
      <AudioFilePicker id="asr-file" label={t('asr.file_label')} accept="audio/*,video/webm,video/mp4,.wav,.mp3,.flac,.ogg,.opus,.m4a,.webm" bind:file bind:recState {busy} onRecorded={() => { result = null; error = ''; needFamily = null; }} />
    </Field>
    <div class="grid gap-3 sm:grid-cols-3">
      <div class="flex flex-col gap-1.5">
        <Label for="asr-task" class="text-sm font-semibold text-foreground">{t('asr.task_label')}</Label>
        <select id="asr-task" bind:value={task} disabled={busy || onlyTranscribes || crispNoTranslate} class="h-9 rounded-md border border-input bg-background px-2 text-sm text-foreground">
          <option value="transcriptions">{t('asr.task_transcribe')}</option>
          {#if !crispNoTranslate}<option value="translations">{t('asr.task_translate')}</option>{/if}
        </select>
        {#if onlyTranscribes || crispNoTranslate}<span class="text-xs text-muted-foreground">{t('sasr.only_transcribes')}</span>{/if}
      </div>
      <div class="flex flex-col gap-1.5">
        <Label for="asr-format" class="text-sm font-semibold text-foreground">{t('asr.format_label')}</Label>
        <select id="asr-format" bind:value={format} disabled={busy || !!engineChosen} class="h-9 rounded-md border border-input bg-background px-2 text-sm text-foreground">
          <option value="json">{t('asr.format_text')}</option>
          <option value="srt">SRT</option>
          <option value="vtt">WebVTT</option>
        </select>
        {#if engineChosen}<span class="text-xs text-muted-foreground">{t('sasr.text_only')}</span>{/if}
      </div>
      {#if !ctcChosen}
      <div class="flex flex-col gap-1.5">
        <Label for="asr-lang" class="text-sm font-semibold text-foreground">{t('asr.language_label')}</Label>
        <Input id="asr-lang" bind:value={language} placeholder={t('asr.language_placeholder')} disabled={busy} />
        <span class="text-xs text-muted-foreground">{t('asr.language_hint')}</span>
      </div>
      {/if}
    </div>
    {#if !ctcChosen && !engineChosen}
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
      {#if result.truncated}
        <p class="flex items-start gap-2 rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-foreground" role="status">
          <TriangleAlert class="mt-px size-4 shrink-0 text-warning" aria-hidden="true" />
          <span class="min-w-0 flex-1">{t(result.truncated === 'tokens' ? 'asr.truncated_tokens' : 'asr.truncated')}</span>
        </p>
      {/if}
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
</div>
