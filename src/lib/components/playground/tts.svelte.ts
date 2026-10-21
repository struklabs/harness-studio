// One text-to-speech state for the Speech tab and the chat's "Read Aloud" buttons: the
// installed voice models, the selection, the downloads and the message player.
import { toast } from 'svelte-sonner';
import { invoke } from '#lib/ipc.js';
import { t } from '#lib/i18n.svelte.js';
import { parseError, rawError } from '#lib/errors.js';
import { localizeError } from '#lib/network.js';
import { app } from '#lib/stores/app.svelte.js';
import { engineInstall } from '#lib/stores/engineInstall.svelte.js';
import { familyFromError } from '#lib/engines.js';
import type { RuntimeDownloadProgress } from '#lib/types.js';
import { allTtsModels, fetchSpeech, pickTtsModel, pickTtsVoice, ttsRequirement, ttsSupportsSpeed, type TtsModelInfo, type TtsOverview } from './tts';
import { chunkSpeechText, plainSpeechText } from './ttsText';
import { readSpeakReplies, speakBlocker, writeSpeakReplies } from './speakReplies';

const FAMILY = 'sherpa-onnx';

class TtsStore {
  overview = $state<TtsOverview | null>(null);
  model = $state('');
  voice = $state('');
  speed = $state(1);
  /** Chat switch: speak every finished reply (see speakReplies.ts). */
  speakReplies = $state(readSpeakReplies(typeof localStorage === 'undefined' ? undefined : localStorage));
  setSpeakReplies(on: boolean) {
    this.speakReplies = on;
    writeSpeakReplies(typeof localStorage === 'undefined' ? undefined : localStorage, on);
  }
  /** Catalog id being downloaded, and its progress. */
  installingModel = $state<string | null>(null);
  modelProgress = $state<RuntimeDownloadProgress | null>(null);
  get installingRuntime(): boolean {
    return engineInstall.busy(this.models.find(m => m.name === this.model)?.engine ?? FAMILY);
  }

  models = $derived<TtsModelInfo[]>(allTtsModels(this.overview));
  private loadToken = 0;

  async load() {
    const token = ++this.loadToken;
    try {
      const overview = await invoke<TtsOverview>('get_tts_overview');
      if (token !== this.loadToken) return;
      this.overview = overview;
    } catch {
      if (token !== this.loadToken) return;
      this.overview = this.overview ?? { models: [], catalog: [], runtime_installed: false };
    }
    this.model = pickTtsModel(this.model, this.models);
    this.voice = pickTtsVoice(this.voice, this.models, this.model);
  }

  /** Downloads a recommended voice model; failures are reported as a toast. */
  async installModel(id: string): Promise<void> {
    if (this.installingModel) return;
    this.installingModel = id;
    const timer = setInterval(async () => {
      try {
        this.modelProgress = await invoke<RuntimeDownloadProgress | null>('get_tts_install_progress');
      } catch {
        /* best effort */
      }
    }, 500);
    try {
      const model = await invoke<TtsModelInfo>('install_tts_model', { id });
      toast.success(t('speak.model_installed', { name: model.name }));
      await this.load();
      this.model = model.name;
      this.voice = '';
    } catch (e) {
      toast.error(t('speak.install_failed', { error: localizeError(e) }));
    } finally {
      clearInterval(timer);
      this.modelProgress = null;
      this.installingModel = null;
    }
  }

  /** Installs the sherpa-onnx runtime (the shared one-click engine installer). */
  async installRuntime(family = FAMILY): Promise<boolean> {
    if (!(await engineInstall.install(family))) return false;
    await this.load();
    return true;
  }
}

export const tts = new TtsStore();

/** Plays one chat message at a time: the text is cut into gateway-sized pieces, each piece is made
 *  while the previous one plays. */
class ReadAloud {
  /** Message being prepared or read. */
  activeId = $state<string | null>(null);
  /** True until the first piece plays. */
  preparing = $state(false);
  private controller: AbortController | null = null;
  private audio: HTMLAudioElement | null = null;
  private urls: string[] = [];

  stop() {
    this.controller?.abort();
    this.controller = null;
    if (this.audio) {
      this.audio.pause();
      this.audio.src = '';
      this.audio = null;
    }
    for (const u of this.urls) URL.revokeObjectURL(u);
    this.urls = [];
    this.activeId = null;
    this.preparing = false;
  }

  /** Starts reading `markdown` as message `id`, or stops when that message is already playing.
   *  `auto` (Speak Replies) stays quiet when speech is not set up: the switch shows why inline,
   *  so every reply does not add another toast. */
  async toggle(id: string, markdown: string, auto = false): Promise<void> {
    if (this.activeId === id) {
      this.stop();
      return;
    }
    this.stop();
    const pieces = chunkSpeechText(plainSpeechText(markdown));
    if (pieces.length === 0) {
      toast.error(t('speak.nothing_to_read'));
      return;
    }
    if (!tts.overview) await tts.load();
    const missing = tts.overview ? ttsRequirement(tts.models, tts.model, tts.overview.runtime_installed) : null;
    const blocker = speakBlocker(tts.models.length > 0, !missing, app.gatewayRunning);
    if (blocker) {
      if (!auto) {
        toast.error(blocker === 'no_model' ? t('speak.no_model_chat')
          : blocker === 'engine' ? t('speak.engine_missing', { family: missing ?? '' })
          : t('speak.need_gateway'));
      }
      return;
    }
    const c = new AbortController();
    this.controller = c;
    this.activeId = id;
    this.preparing = true;
    const make = (input: string) =>
      fetchSpeech(app.inferenceUrl, { model: tts.model, input, voice: tts.voice, speed: ttsSupportsSpeed(tts.models, tts.model) ? tts.speed : 1 }, app.inferenceHeaders(), c.signal);
    try {
      let next = make(pieces[0]);
      for (let i = 0; i < pieces.length; i++) {
        const current = await next;
        if (c.signal.aborted) return;
        // The next piece is made while this one plays.
        if (i + 1 < pieces.length) {
          next = make(pieces[i + 1]);
          next.catch(() => {});
        }
        this.preparing = false;
        await this.play(current.blob, c.signal);
        if (c.signal.aborted) return;
      }
    } catch (e) {
      if (!c.signal.aborted) {
        const msg = e instanceof Error ? e.message : String(e);
        const family = familyFromError(msg);
        const parsed = parseError(rawError(e));
        const helper = parsed?.key === 'tts.crispasr_helper_missing' ? String(parsed.params.helper ?? '') : null;
        toast.error(
          t('speak.read_failed', { error: family ? t('lib.runtime_needs_hint', { runtime: family }) : localizeError(e) }),
          helper
            ? { action: { label: t('speak.download_helper'), onClick: () => {
              void invoke('download_crispasr_helper', { id: helper }).then(() => toast.success(t('crisp.helper_queued'))).catch(e => toast.error(localizeError(e)));
            } } }
            : family && engineInstall.canInstall(family)
            ? { action: { label: t('eng.install', { runtime: family }), onClick: () => void engineInstall.install(family) } }
            : undefined,
        );
      }
    } finally {
      if (this.controller === c) this.stop();
    }
  }

  /** Plays a recording that came with the reply itself (a natively speaking model), as message `id`. */
  async playClip(id: string, blob: Blob): Promise<void> {
    this.stop();
    const c = new AbortController();
    this.controller = c;
    this.activeId = id;
    try {
      await this.play(blob, c.signal);
    } catch (e) {
      if (!c.signal.aborted) toast.error(t('speak.read_failed', { error: localizeError(e) }));
    } finally {
      if (this.controller === c) this.stop();
    }
  }

  private play(blob: Blob, signal: AbortSignal): Promise<void> {
    return new Promise((resolve, reject) => {
      const url = URL.createObjectURL(blob);
      this.urls.push(url);
      const audio = new Audio(url);
      this.audio = audio;
      const done = () => {
        audio.onended = audio.onerror = null;
        signal.removeEventListener('abort', done);
        resolve();
      };
      audio.onended = done;
      audio.onerror = () => {
        audio.onended = audio.onerror = null;
        signal.removeEventListener('abort', done);
        reject(new Error(t('speak.play_failed')));
      };
      signal.addEventListener('abort', done);
      audio.play().catch((e) => {
        signal.removeEventListener('abort', done);
        reject(e);
      });
    });
  }
}

export const readAloud = new ReadAloud();
