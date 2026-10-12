// One text-to-speech state for the Speech tab and the chat's "Read Aloud" buttons: the
// installed voice models, the selection, the downloads and the message player.
import { toast } from 'svelte-sonner';
import { invoke } from '$lib/ipc';
import { t } from '$lib/i18n.svelte';
import { localizeError } from '$lib/network';
import { app } from '$lib/stores/app.svelte';
import { runtimesStore } from '$lib/stores/runtimes.svelte';
import type { RuntimeDownloadProgress, RuntimeEntry, RuntimeReleasePick } from '$lib/types';
import { fetchSpeech, pickTtsModel, pickTtsVoice, type TtsModelInfo, type TtsOverview } from './tts';
import { chunkSpeechText, plainSpeechText } from './ttsText';

const FAMILY = 'sherpa-onnx';
const REPO = 'k2-fsa/sherpa-onnx';

class TtsStore {
  overview = $state<TtsOverview | null>(null);
  model = $state('');
  voice = $state('');
  speed = $state(1);
  /** Catalog id being downloaded, and its progress. */
  installingModel = $state<string | null>(null);
  modelProgress = $state<RuntimeDownloadProgress | null>(null);
  installingRuntime = $state(false);

  models = $derived<TtsModelInfo[]>(this.overview?.models ?? []);

  async load() {
    try {
      this.overview = await invoke<TtsOverview>('get_tts_overview');
    } catch {
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

  /** Installs the sherpa-onnx runtime (the same preview + install commands as Settings > Runtime). */
  async installRuntime(): Promise<boolean> {
    if (this.installingRuntime) return false;
    this.installingRuntime = true;
    try {
      const pick = await invoke<RuntimeReleasePick>('preview_runtime_release', {
        query: { family: FAMILY, repo: REPO, tag: null, backend: null, asset: null },
      });
      if (pick.unverified.length > 0) {
        toast.error(t('hub.compat_install_unverified'));
        return false;
      }
      const entry = await invoke<RuntimeEntry>('install_runtime_github', {
        request: {
          family: pick.family ?? FAMILY,
          repo: pick.repo,
          tag: pick.tag,
          asset: pick.assets[0]?.name ?? null,
          backend: null,
          name: 'sherpa-onnx',
          id: null,
          allow_unverified: false,
        },
      });
      toast.success(t('rt.added', { name: entry.name }));
      await runtimesStore.load(true);
      await this.load();
      return true;
    } catch (e) {
      toast.error(t('hub.compat_install_failed', { error: localizeError(e) }));
      return false;
    } finally {
      this.installingRuntime = false;
    }
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

  /** Starts reading `markdown` as message `id`, or stops when that message is already playing. */
  async toggle(id: string, markdown: string): Promise<void> {
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
    if (!app.gatewayRunning) {
      toast.error(t('speak.need_gateway'));
      return;
    }
    if (!tts.overview) await tts.load();
    if (tts.models.length === 0) {
      toast.error(t('speak.no_model_chat'));
      return;
    }
    if (tts.overview && !tts.overview.runtime_installed) {
      toast.error(t('speak.no_runtime'));
      return;
    }
    const c = new AbortController();
    this.controller = c;
    this.activeId = id;
    this.preparing = true;
    const make = (input: string) =>
      fetchSpeech(app.inferenceUrl, { model: tts.model, input, voice: tts.voice, speed: tts.speed }, app.inferenceHeaders(), c.signal);
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
        const family = /runtime\.family_not_installed:\s*([\w.+-]+)/.exec(msg)?.[1];
        toast.error(t('speak.read_failed', { error: family ? t('lib.runtime_needs_hint', { runtime: family }) : localizeError(e) }));
      }
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
