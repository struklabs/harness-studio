<script lang="ts">
  import { t, locale } from '#lib/i18n.svelte.js';
  import { invoke } from '#lib/ipc.js';
  import { toast } from 'svelte-sonner';
  import { parseError, rawError } from '#lib/errors.js';
  import { localizeError } from '#lib/network.js';
  import type { GatewayOverview } from '#lib/types.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import EngineNeeded from '../common/EngineNeeded.svelte';
  import { engineInstall } from '#lib/stores/engineInstall.svelte.js';
  import { familyFromError } from '#lib/engines.js';
  import { Badge } from '#lib/components/ui/badge/index.js';
  import { Label } from '#lib/components/ui/label/index.js';
  import { Textarea } from '#lib/components/ui/textarea/index.js';
  import { Progress } from '#lib/components/ui/progress/index.js';
  import AdvancedOnly from '../common/AdvancedOnly.svelte';
  import { formatBytes, fixed } from '#lib/format.js';
  import { Download, LoaderCircle, Power, Square, Volume2, TriangleAlert } from '@lucide/svelte';
  import { onDestroy, untrack } from 'svelte';
  import { library } from '#lib/stores/library.svelte.js';
  import { blobToBase64, fileToProject, outputName } from './projectOutputs';
  import { fetchSpeech, pickTtsModel, pickTtsVoice, voicesOf, ttsRequirement, ttsRunBlocker, ttsSupportsSpeed, type SpeechResult } from './tts';
  import { readAloud, tts } from './tts.svelte';
  import { MAX_SPEECH_CHARS } from './ttsText';
  import VoiceCloneSection from './VoiceCloneSection.svelte';
  import { canClone, engineDisabled, cloneBlocker, cloneNeedsText, voiceRefOf } from './voiceClone';

  let text = $state('');
  let busy = $state(false);
  let error = $state('');
  let missingHelper = $state<string | null>(null);
  let helperBusy = $state(false);
  /** Engine family the failed run was missing; shown with its install button. */
  let needFamily = $state<string | null>(null);
  let result = $state<{ url: string; info: SpeechResult } | null>(null);
  let controller: AbortController | null = null;
  let cloneFile = $state<File | null>(null);
  let cloneConsent = $state(false);
  let cloneText = $state('');

  $effect(() => {
    void library.version;
    untrack(() => void tts.load());
  });
  onDestroy(() => {
    controller?.abort();
    if (result) URL.revokeObjectURL(result.url);
  });

  $effect(() => {
    tts.model = pickTtsModel(tts.model, tts.models);
    tts.voice = pickTtsVoice(tts.voice, tts.models, tts.model);
  });

  let supportsSpeed = $derived(ttsSupportsSpeed(tts.models, tts.model));
  let voices = $derived(voicesOf(tts.models, tts.model));
  let missingFamily = $derived(tts.overview ? ttsRequirement(tts.models, tts.model, tts.overview.runtime_installed) : null);
  // A missing engine that has a one-click install does not block Run: Run installs it first.
  let engineAutoInstallable = $derived(!!missingFamily && engineInstall.canInstall(missingFamily) && engineInstall.offered(missingFamily));
  let installingEngine = $state<string | null>(null);
  let runtimeMissing = $derived(!!missingFamily && !engineAutoInstallable);
  let cloning = $derived(canClone(tts.models, tts.model));
  let cloneTextNeeded = $derived(cloneNeedsText(tts.models, tts.model));
  let cloneStop = $derived(cloning ? cloneBlocker(cloneFile, cloneConsent, cloneTextNeeded, cloneText) : null);
  let engineOff = $derived(engineDisabled(tts.models, tts.model, !!app.settings?.audio_cpp_enabled));
  let runBlocker = $derived(ttsRunBlocker(busy, runtimeMissing, app.gatewayRunning, tts.models.length, text));
  // A clone request needs its sample and the permission box; without a sample the model speaks in its own voice.
  let cloneActive = $derived(cloning && cloneFile !== null);
  let canRun = $derived(runBlocker === null && !engineOff && (!cloneActive || cloneStop === null));
  let runHint = $derived(
    runBlocker === 'runtime' ? t('speak.run_need_runtime', { family: missingFamily ?? 'sherpa-onnx' })
      : runBlocker === 'gateway' ? t('speak.run_need_gateway')
      : runBlocker === 'model' ? t('speak.run_need_model')
      : runBlocker === 'text' ? t('speak.run_need_text')
      : engineOff ? t('audio_cpp.needs_engine')
      : cloneActive && cloneStop === 'text' ? t('voice.need_text')
      : cloneActive && cloneStop === 'consent' ? t('voice.need_consent')
      : cloneActive && cloneStop === 'size' ? t('voice.need_smaller') : '',
  );
  let progressPct = $derived(
    tts.modelProgress && tts.modelProgress.total_bytes > 0 ? (tts.modelProgress.downloaded_bytes / tts.modelProgress.total_bytes) * 100 : 0,
  );

  async function run() {
    if (!canRun) return;
    readAloud.stop();
    controller?.abort();
    const c = new AbortController();
    controller = c;
    busy = true;
    error = '';
    if (missingFamily && engineAutoInstallable) {
      const family = missingFamily;
      installingEngine = family;
      const ok = await tts.installRuntime(family);
      installingEngine = null;
      if (!ok) {
        error = t('eng.auto_failed', { runtime: family, error: engineInstall.state(family).error ?? '' });
        if (controller === c) controller = null;
        busy = false;
        return;
      }
    }
    // The previous audio (and its truncation notice) does not outlive a new request.
    if (result) URL.revokeObjectURL(result.url);
    result = null;
    missingHelper = null;
    needFamily = null;
    try {
      const clone = cloneActive && cloneFile ? await voiceRefOf(cloneFile, cloneText) : undefined;
      const info = await fetchSpeech(
        app.inferenceUrl,
        { model: tts.model, input: text.trim(), voice: tts.voice, speed: supportsSpeed ? tts.speed : 1, clone },
        app.inferenceHeaders(),
        c.signal,
      );
      result = { url: URL.createObjectURL(info.blob), info };
      void blobToBase64(info.blob).then((data) => fileToProject(outputName(text, 'wav', 'speech'), data, { source: 'speech', model: tts.model }));
    } catch (e) {
      if (!c.signal.aborted) {
        const msg = e instanceof Error ? e.message : String(e);
        needFamily = familyFromError(msg);
        const parsed = parseError(rawError(e));
        missingHelper = parsed?.key === 'tts.crispasr_helper_missing' ? String(parsed.params.helper ?? '') : null;
        error = needFamily ? '' : localizeError(e);
      }
    } finally {
      if (controller === c) {
        controller = null;
        busy = false;
      }
    }
  }

  async function downloadHelper() {
    if (!missingHelper) return;
    helperBusy = true;
    try {
      await invoke('download_crispasr_helper', { id: missingHelper });
      toast.success(t('crisp.helper_queued'));
      missingHelper = null;
    } catch (e) {
      toast.error(localizeError(e));
    } finally {
      helperBusy = false;
    }
  }

  function cancel() {
    controller?.abort();
    controller = null;
    busy = false;
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

  const seconds = (b: SpeechResult) => (b.sampleRate ? Math.max(0, (b.blob.size - 44) / 2 / b.sampleRate) : null);
</script>

<div class="mx-auto flex w-full max-w-[860px] flex-col gap-5 px-4 py-6">
  <h3 id="tts-title" class="text-base font-semibold text-foreground">{t('speak.title')}</h3>

  {#if !app.gatewayRunning}
    <div class="flex items-start gap-2 rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-foreground" role="status">
      <TriangleAlert class="mt-px size-4 shrink-0 text-warning" aria-hidden="true" />
      <span class="min-w-0 flex-1">{t('speak.need_gateway')}</span>
      <Button variant="outline" size="sm" class="shrink-0" disabled={enablingGateway} onclick={enableGateway}>
        {#if enablingGateway}<LoaderCircle class="animate-spin" />{:else}<Power />{/if}
        {t('asr.enable_gateway')}
      </Button>
    </div>
  {/if}
  {#if runtimeMissing}
    <div class="flex items-start gap-2 rounded-md border border-border bg-muted/40 px-3 py-2 text-xs text-foreground" role="status">
      <TriangleAlert class="mt-px size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
      <span class="min-w-0 flex-1">{t('speak.engine_missing', { family: missingFamily ?? 'sherpa-onnx' })}</span>
      <Button variant="outline" size="sm" class="shrink-0" disabled={tts.installingRuntime} onclick={() => tts.installRuntime(missingFamily ?? 'sherpa-onnx')}>
        {#if tts.installingRuntime}<LoaderCircle class="animate-spin" />{:else}<Download />{/if}
        {tts.installingRuntime ? t('speak.installing') : t('speak.install_engine', { family: missingFamily ?? 'sherpa-onnx' })}
      </Button>
    </div>
  {/if}

  {#if tts.overview}
    <section class="flex flex-col gap-2" aria-labelledby="tts-catalog">
      <h4 id="tts-catalog" class="text-sm font-semibold text-foreground">{t('speak.catalog_title')}</h4>
      {#if tts.models.length === 0}
        <p class="text-xs text-muted-foreground">{t('speak.no_model')}</p>
      {/if}
      <ul class="flex flex-col gap-2">
        {#each tts.overview.catalog as row (row.id)}
          <li class="flex flex-wrap items-center gap-3 rounded-md border border-border px-3 py-2">
            <div class="flex min-w-0 flex-1 flex-col gap-0.5">
              <span class="flex flex-wrap items-center gap-2 text-sm font-medium text-foreground">
                {row.title}
                {#if row.recommended}<Badge variant="outline">{t('speak.recommended')}</Badge>{/if}
              </span>
              <span class="text-xs text-muted-foreground">
                {row.languages.join(', ')} · {t('speak.voices_count', { count: row.voices })} · {formatBytes(row.size_bytes)} · {t('speak.license', { license: row.license })}
              </span>
              {#if tts.installingModel === row.id && tts.modelProgress}
                <Progress value={progressPct} class="mt-1" aria-label={t('speak.installing')} />
              {/if}
            </div>
            {#if row.installed}
              <Badge variant="secondary">{t('speak.installed')}</Badge>
            {:else}
              <Button variant="outline" size="sm" disabled={!!tts.installingModel} onclick={() => tts.installModel(row.id)}>
                {#if tts.installingModel === row.id}<LoaderCircle class="animate-spin" />{:else}<Download />{/if}
                {tts.installingModel === row.id ? t('speak.installing') : t('speak.install')}
              </Button>
            {/if}
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  <section class="flex flex-col gap-3" aria-labelledby="tts-title">
    <div class="grid gap-3 sm:grid-cols-3">
      <div class="flex flex-col gap-1.5">
        <Label for="tts-model" class="text-sm font-semibold text-foreground">{t('speak.model_label')}</Label>
        <select
          id="tts-model"
          bind:value={tts.model}
          disabled={busy || tts.models.length === 0}
          class="h-9 rounded-md border border-input bg-background px-2 text-sm text-foreground"
        >
          {#each tts.models as m (m.id)}
            <option value={m.name}>{m.name}</option>
          {/each}
        </select>
      </div>
      <div class="flex flex-col gap-1.5">
        <Label for="tts-voice" class="text-sm font-semibold text-foreground">{t('speak.voice_label')}</Label>
        <select
          id="tts-voice"
          bind:value={tts.voice}
          disabled={busy || voices.length === 0}
          class="h-9 rounded-md border border-input bg-background px-2 text-sm text-foreground"
        >
          <option value="">{t('speak.voice_default')}</option>
          {#each voices as v (v.id)}
            <option value={v.id}>{v.id}{v.language ? ` (${v.language})` : ''}</option>
          {/each}
        </select>
      </div>
      {#if supportsSpeed}
        <div class="flex flex-col gap-1.5">
          <Label for="tts-speed" class="text-sm font-semibold text-foreground">{t('speak.speed_label')}</Label>
          <div class="flex h-9 items-center gap-2">
            <input id="tts-speed" type="range" min="0.5" max="2" step="0.05" bind:value={tts.speed} disabled={busy} class="min-w-0 flex-1" />
            <span class="font-num w-10 text-right text-xs text-muted-foreground">{fixed(tts.speed, 2)}×</span>
          </div>
        </div>
      {/if}
    </div>
    {#if cloning}
      <VoiceCloneSection bind:file={cloneFile} bind:consent={cloneConsent} bind:text={cloneText} needsText={cloneTextNeeded} {busy} {engineOff} />
    {/if}
    <div class="flex flex-col gap-1.5">
      <Label for="tts-text" class="text-sm font-semibold text-foreground">{t('speak.text_label')}</Label>
      <Textarea
        id="tts-text"
        bind:value={text}
        maxlength={MAX_SPEECH_CHARS}
        rows={5}
        placeholder={t('speak.text_placeholder')}
        disabled={busy}
      />
      <span class="font-num self-end text-xs text-muted-foreground">{t('speak.chars', { count: text.length, max: MAX_SPEECH_CHARS })}</span>
    </div>
    <div class="flex flex-wrap items-center gap-3">
      <Button size="sm" onclick={run} disabled={!canRun} aria-describedby={runHint ? 'tts-run-hint' : undefined}>
        {#if busy}<LoaderCircle class="animate-spin" />{:else}<Volume2 />{/if}
        {busy ? t('speak.running') : t('speak.run')}
      </Button>
      {#if busy}
        <Button variant="ghost" size="sm" onclick={cancel}><Square />{t('speak.stop')}</Button>
      {/if}
      <div class="flex flex-wrap items-center gap-2" role="status">
        {#if installingEngine}
          {@const pct = engineInstall.state(installingEngine).pct}
          <span class="text-xs text-muted-foreground">{t(pct === null ? 'eng.auto_installing' : 'eng.auto_installing_pct', { runtime: installingEngine, pct: pct ?? 0 })}</span>
        {/if}
        {#if runHint}
          <span id="tts-run-hint" class="text-xs text-muted-foreground">{runHint}</span>
          {#if runBlocker === 'runtime'}
            <Button variant="outline" size="sm" disabled={tts.installingRuntime} onclick={() => tts.installRuntime(missingFamily ?? 'sherpa-onnx')}>
              {#if tts.installingRuntime}<LoaderCircle class="animate-spin" />{:else}<Download />{/if}
              {tts.installingRuntime ? t('speak.installing') : t('speak.install_engine', { family: missingFamily ?? 'sherpa-onnx' })}
            </Button>
          {/if}
        {/if}
      </div>
    </div>
  </section>

  {#if needFamily}
    <div class="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive [overflow-wrap:anywhere]" role="alert">
      <EngineNeeded family={needFamily} auto onInstalled={() => { needFamily = null; void run(); }} />
    </div>
  {:else if error}
    <div class="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive [overflow-wrap:anywhere]" role="alert">
      {error}
      {#if missingHelper}
        <Button variant="outline" size="sm" disabled={helperBusy} onclick={downloadHelper}>
          <Download />{t('speak.download_helper')}
        </Button>
      {/if}
    </div>
  {/if}

  {#if result}
    <section class="flex flex-col gap-3" aria-labelledby="tts-result">
      <div class="flex flex-wrap items-center gap-2">
        <h4 id="tts-result" class="text-sm font-semibold text-foreground">{t('speak.result_title')}</h4>
        <AdvancedOnly><span class="font-num text-xs text-muted-foreground">{t('emb.stat_time', { value: Math.round(result.info.ms) })}</span></AdvancedOnly>
        {#if result.info.voice}<Badge variant="outline">{result.info.voice}</Badge>{/if}
        {#if seconds(result.info) !== null}
          <Badge variant="outline" class="font-num">{t('asr.stat_duration', { value: (seconds(result.info) ?? 0).toLocaleString(locale(), { minimumFractionDigits: 1, maximumFractionDigits: 1 }) })}</Badge>
        {/if}
        <Button variant="ghost" size="sm" href={result.url} download="speech.wav"><Download />{t('speak.download')}</Button>
      </div>
      {#if result.info.truncated}
        <p class="flex items-start gap-2 rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-foreground" role="status">
          <TriangleAlert class="mt-px size-4 shrink-0 text-warning" aria-hidden="true" />
          <span class="min-w-0 flex-1">{t(result.info.truncated === 'frames' ? 'speak.truncated_frames' : 'speak.truncated')}</span>
        </p>
      {/if}
      <!-- svelte-ignore a11y_media_has_caption -->
      <audio controls autoplay src={result.url} class="w-full" aria-label={t('speak.result_title')}></audio>
    </section>
  {/if}
</div>
