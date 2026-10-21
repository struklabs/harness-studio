<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import { Mic, Square, LoaderCircle, X } from '@lucide/svelte';
  import { t } from '#lib/i18n.svelte.js';
  import { localizeError } from '#lib/network.js';
  import { app } from '#lib/stores/app.svelte.js';
  import { domain } from '#lib/stores/domain.svelte.js';
  import { runtimesStore } from '#lib/stores/runtimes.svelte.js';
  import { library } from '#lib/stores/library.svelte.js';
  import { security } from '#lib/stores/security.svelte.js';
  import { holdReleaseWhile } from '#lib/windowActivity.js';
  import { Button } from '#lib/components/ui/button/index.js';
  import EngineNeeded from '../common/EngineNeeded.svelte';
  import { familyFromError } from '#lib/engines.js';
  import { fetchTranscription } from './transcribe';
  import { RecordError } from './recorder';
  import { createRecordingSession, recordingClock, type RecordingState } from './recordingSession';
  import { speech } from './speech.svelte';
  import { asr } from './asr.svelte';
  import { dictationFamily, dictationRequirement, speechModel } from './speechSetup';

  let { onText, contextKey, blocked = false }: { onText: (text: string) => void; contextKey: number; blocked?: boolean } = $props();
  let recState = $state<RecordingState>('idle');
  let seconds = $state(0);
  let busy = $state(false);
  let checking = $state(false);
  let error = $state('');
  let setup = $state(false);
  /** Engine family dictation is missing; shown with its install button. */
  let needFamily = $state<string | null>(null);
  let setupSection = $state<'runtime' | 'gateway'>('runtime');
  let generation = 0;
  let controller: AbortController | null = null;
  let request: { url: string; headers: Record<string, string>; model: string } | null = null;
  const microphone = createRecordingSession({
    state: (next) => (recState = next),
    seconds: (next) => (seconds = next),
    level: () => {},
    file: (file) => void transcribe(file),
    error: (e) => {
      error = e instanceof RecordError && e.code !== 'failed' ? t(`asr.rec_${e.code}`) : t('asr.rec_failed', { error: localizeError(e) });
    },
  });
  $effect(() => holdReleaseWhile('chat-dictation', () => recState !== 'idle' || busy));
  function cancel() {
    generation++;
    microphone.cancel();
    controller?.abort();
    controller = null;
    busy = false;
    checking = false;
    request = null;
  }
  $effect(() => {
    void contextKey;
    untrack(() => { cancel(); error = ''; setup = false; needFamily = null; });
  });
  $effect(() => { if (blocked) untrack(cancel); });
  onDestroy(cancel);

  async function record() {
    if (blocked || checking || busy || recState !== 'idle') return;
    const token = ++generation;
    error = '';
    setup = false;
    needFamily = null;
    checking = true;
    await Promise.all([domain.refreshDefinitions(), runtimesStore.load(true), asr.load(), library.refresh()]);
    if (token !== generation) return;
    checking = false;
    speech.model = speechModel(speech.model, domain.definitions, asr.names, asr.engineNames, library.models);
    const missing = dictationRequirement(speech.model, runtimesStore.list, app.gatewayRunning, asr.names, domain.definitions, asr.engineNames, library.models);
    if (missing) {
      setup = true;
      setupSection = missing === 'gateway' ? 'gateway' : 'runtime';
      if (missing === 'runtime') needFamily = dictationFamily(speech.model, asr.names, domain.definitions, asr.engineNames, library.models);
      else error = t(missing === 'model' ? 'pg.dictation_need_model' : 'asr.need_gateway');
      return;
    }
    request = { url: app.inferenceUrl, headers: app.inferenceHeaders(), model: speech.model };
    await microphone.record();
  }
  async function transcribe(file: File) {
    if (!request) return;
    const c = new AbortController();
    controller = c;
    busy = true;
    try {
      const result = await fetchTranscription(request.url, 'transcriptions', file, request.model,
        { language: '', prompt: '', format: 'json', textOnly: asr.engineNames.includes(request.model) }, request.headers, c.signal);
      if (!c.signal.aborted) {
        if (result.text.trim()) onText(result.text);
        else error = t('pg.dictation_empty');
      }
    } catch (e) {
      if (!c.signal.aborted) {
        const message = localizeError(e);
        needFamily = familyFromError(String(e));
        setup = !!needFamily;
        setupSection = 'runtime';
        error = needFamily ? '' : message;
      }
    } finally {
      if (controller === c) { controller = null; busy = false; }
    }
  }
</script>

<span class="flex flex-wrap items-center gap-1">
  <Button type="button" size="sm" variant={recState === 'recording' ? 'destructive' : 'ghost'}
    disabled={blocked || checking || busy || recState === 'starting'}
    aria-label={recState === 'recording' ? t('asr.rec_stop') : t('pg.dictation')}
    title={t('pg.dictation_hint')} onclick={() => recState === 'recording' ? microphone.finish() : void record()}>
    {#if checking || busy || recState === 'starting'}<LoaderCircle class="animate-spin" />
    {:else if recState === 'recording'}<Square />{:else}<Mic />{/if}
    {recState === 'recording' ? t('asr.rec_stop') : busy ? t('asr.running') : t('pg.dictation')}
  </Button>
  {#if recState !== 'idle' || busy || checking}
    <span class="text-xs text-muted-foreground" role="status">
      {recState === 'recording' ? `${t('pg.dictation_recording')} ${recordingClock(seconds)}` : busy ? t('asr.running') : t('pg.dictation_starting')}
    </span>
    <Button type="button" variant="ghost" size="icon-sm" onclick={cancel} aria-label={t('pg.cancel')}><X /></Button>
  {/if}
  {#if needFamily}
    <span class="text-xs text-destructive" role="alert">
      <EngineNeeded family={needFamily} size="sm" auto onInstalled={() => { needFamily = null; setup = false; }} />
    </span>
  {:else if error}
    <span class="text-xs text-destructive" role="alert">{error}</span>
    {#if setup}
      <Button type="button" variant="link" size="sm" onclick={() => app.openTab('library')}>{t('pg.dictation_library')}</Button>
      <Button type="button" variant="link" size="sm" onclick={() => security.openSettings(setupSection)}>{t('pg.dictation_settings')}</Button>
    {/if}
  {/if}
</span>
