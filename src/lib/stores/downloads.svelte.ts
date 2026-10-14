import { toast } from 'svelte-sonner';
import { ACTION_TOAST_DURATION } from '#lib/toast.js';
import { DownloadQueue } from '#lib/components/hub/downloadQueue.svelte.js';
import { t } from '#lib/i18n.svelte.js';
import { app, DEFAULT_SERVER_ID } from '#lib/stores/app.svelte.js';
import { localizeError } from '#lib/network.js';
import { defaultLoadTarget, isChatLoadable, shouldAutoLoad } from '#lib/downloadFollowUp.js';
import { library, repoRelative } from '#lib/stores/library.svelte.js';
import { domain, samePath } from '#lib/stores/domain.svelte.js';
import { invoke } from '#lib/ipc.js';
import { holdReleaseWhile } from '#lib/windowActivity.js';
import { suggestAlias } from '#lib/components/library/filters.js';
import { isImageDefinition } from '#lib/components/playground/image.js';
import { filesNeedingDefinition, playgroundTarget } from '#lib/libraryDefinitions.js';
import { imageSession } from '#lib/components/playground/image.svelte.js';
import { speech } from '#lib/components/playground/speech.svelte.js';
import { asr } from '#lib/components/playground/asr.svelte.js';
import { tts } from '#lib/components/playground/tts.svelte.js';
import type { DownloadProgress, LoadTarget, LocalModel, ModelDefinition, ModelKind } from '#lib/types.js';

/**
 * App-wide download queue. It lives outside the Model Hub component so progress
 * tracking (and completion toasts) keep working while the user is on another tab.
 */
export const downloads = new DownloadQueue();

/** File stem without `.gguf` / `.bin` (whisper ggml) and the `-00001-of-00003` shard suffix. */
function stemOf(rel: string): string {
  const file = rel.slice(rel.lastIndexOf('/') + 1);
  return file.replace(/\.(gguf|bin)$/i, '').replace(/-\d{5}-of-\d{5}$/i, '');
}

// A finished download changes the library on disk: rescan it (Dashboard / Benchmark / Hub badges
// follow `library.version`) and offer to open the new model in the Dashboard.
downloads.onCompleted = async (job) => {
  const message = t('hub.announce_completed', { name: job.variant_name });
  library.markChanged();
  const list = await library.refresh();
  const model = modelOfJob(job, list);
  // A vision projector that arrived after its model: the model's definition gets it now, else
  // the gateway serves the model without image input.
  await attachProjectors(job, list);
  // The backend made the definition of a finished image model (or of one that was waiting for this
  // companion file): say so, and offer the Image tab.
  if (await offerImageModels(job, list, message)) return;
  if (model?.kind === 'asr' && model.is_ready) {
    await offerSpeechModel(model, message);
    return;
  }
  if (!model) {
    toast.success(message);
    return;
  }
  // The Playground picker and the gateway list definitions, not files: give a downloaded chat /
  // embedding / rerank model one right away so it is usable there without a trip to the Dashboard.
  if (model.is_ready && ['llm', 'embedding', 'reranker'].includes(model.kind ?? '')) {
    try {
      await ensureDefinition(model);
    } catch (e) {
      console.warn('definition for downloaded model', e);
    }
  }
  const openInDashboard = {
    label: t('hub.open_in_dashboard'),
    onClick: () => {
      library.pendingSelectId = model.id;
      app.openTab('dashboard');
    },
  };
  if (!isChatLoadable(model)) {
    toast.success(message, { duration: ACTION_TOAST_DURATION, closeButton: true, action: openInDashboard });
    return;
  }
  // Settings > Storage > Downloads: load the finished chat model without being asked.
  if (shouldAutoLoad(app.settings?.auto_load_after_download ?? false, job, model, downloads.jobs)) {
    if (await autoLoad(model)) return;
  }
  toast.success(message, {
    duration: ACTION_TOAST_DURATION,
    closeButton: true,
    action: { label: t('hub.load_and_chat'), onClick: () => void loadAndChat(model) },
    cancel: openInDashboard,
  });
};

/**
 * An image model needs a definition with its companion files (VAE, text encoders). The backend
 * makes it when the last file arrived (the call here makes sure, idempotently); when this job
 * finished one, the toast says so and offers the Image tab. True when it toasted.
 */
async function offerImageModels(job: DownloadProgress, list: LocalModel[], message: string): Promise<boolean> {
  if (!list.some((m) => m.kind === 'image')) return false;
  try {
    const before = new Set(domain.definitions.map((d) => d.id));
    const made = await invoke<string[]>('sync_image_definitions');
    await domain.refreshDefinitions();
    const own = modelOfJob(job, list);
    const created = new Set(made);
    for (const d of domain.definitions) {
      if (!before.has(d.id) && isImageDefinition(d, list)) created.add(d.alias);
      if (own?.kind === 'image' && d.base.kind === 'local' && samePath(d.base.path, own.path)) created.add(d.alias);
    }
    if (created.size === 0) return false;
    toast.success(t('img.model_ready', { alias: [...created].join(', ') }), {
      duration: ACTION_TOAST_DURATION,
      closeButton: true,
      action: { label: t('img.open_tab'), onClick: () => app.openPlaygroundTab('image') },
    });
    return true;
  } catch (e) {
    console.warn('image definitions', e);
    toast.success(message);
    toast.error(t('img.definition_failed', { error: localizeError(e) }));
    return true;
  }
}

/** The library file a finished job produced (null while the rescan has not found it). */
export function modelOfJob(job: DownloadProgress, list: LocalModel[]): LocalModel | undefined {
  const wanted = job.variant_name.toLowerCase();
  return list.find((m) => {
    // A custom-URL job is named after its file and lands in the library root.
    if (job.repo_id === 'custom_url') return m.filename.toLowerCase() === wanted;
    const rel = repoRelative(m, job.repo_id);
    return rel !== null && stemOf(rel).toLowerCase() === wanted && !m.gguf?.is_mmproj;
  });
}

function loadTarget(): LoadTarget {
  return defaultLoadTarget(
    app.selectedServerId,
    DEFAULT_SERVER_ID,
    app.profiles.map((p) => p.id),
    app.isRunning || app.isBusy,
  );
}

/** Asks the Playground for a new chat on `alias` served by `serverId`. */
function openChat(serverId: string, alias: string) {
  domain.pendingChat = { serverId, alias };
  app.openPlaygroundTab('chat');
}

/**
 * Loads `model` (its definition is made when missing) with the Playground picker's default target
 * and opens a new chat on it. A model already running somewhere is reused, not restarted.
 */
export async function loadAndChat(model: LocalModel): Promise<void> {
  try {
    const alias = await ensureDefinition(model);
    const def = domain.definitions.find((d) => d.alias === alias);
    if (!def) throw new Error(alias);
    await domain.refreshStates();
    const live = domain.state(def.id)?.servers.find((s) => s.status === 'Running');
    if (live) {
      openChat(live.server_id, alias);
      return;
    }
    const target = loadTarget();
    const serverId = await invoke<string>('load_definition', { definitionId: def.id, target });
    await app.refreshProfiles();
    toast.success(t('pgm.loading_toast', { alias, server: app.serverLabel(serverId) }));
    openChat(serverId, alias);
    await domain.refreshStates();
  } catch (e) {
    toast.error(t('pgm.load_failed', { error: localizeError(e) }));
  }
}

/**
 * The "load automatically" setting: starts the model without taking the selected server away from
 * a model the user is using (a busy selection gets a new server). Returns false when it failed
 * (the usual completion toast then follows).
 */
async function autoLoad(model: LocalModel): Promise<boolean> {
  try {
    const alias = await ensureDefinition(model);
    const def = domain.definitions.find((d) => d.alias === alias);
    if (!def) return false;
    const target = loadTarget();
    const serverId = await invoke<string>('load_definition', { definitionId: def.id, target });
    await app.refreshProfiles();
    toast.success(t('hub.auto_loaded', { alias, server: app.serverLabel(serverId) }), {
      duration: ACTION_TOAST_DURATION,
      closeButton: true,
      action: { label: t('hub.chat_now'), onClick: () => openChat(serverId, alias) },
    });
    await domain.refreshStates();
    return true;
  } catch (e) {
    toast.error(t('pgm.load_failed', { error: localizeError(e) }));
    return false;
  }
}

/**
 * A downloaded whisper model is only usable through a model definition (the Speech tab lists
 * definitions): make one now unless the file already has one, then offer the Speech tab.
 */
async function offerSpeechModel(model: LocalModel, message: string) {
  try {
    const alias = await ensureDefinition(model);
    toast.success(t('asr.model_ready', { alias }), {
      duration: ACTION_TOAST_DURATION,
      closeButton: true,
      action: {
        label: t('asr.open_tab'),
        onClick: () => {
          app.openPlaygroundTab('transcription');
        },
      },
    });
  } catch (e) {
    toast.success(message);
    toast.error(t('asr.definition_failed', { error: String(e) }));
  }
}

// Started once, as soon as the app loads this module (idempotent, browser only).
if (typeof window !== 'undefined') {
  downloads.start();
  // A download finishing is followed up here (rescan, model definition): a page released while
  // one runs would miss it (the rebuilt page sees it as already finished).
  holdReleaseWhile('downloads', () => downloads.hasActivity);
}

/** The alias of the file's definition, made now (alias from the file name) if it has none. */
export async function ensureDefinition(model: LocalModel): Promise<string> {
  await domain.refreshDefinitions();
  const existing = domain.definitions.find((d) => d.base.kind === 'local' && samePath(d.base.path, model.path));
  if (existing) return existing.alias;
  const def = await invoke<ModelDefinition>('save_model_definition', {
    definition: {
      id: '',
      alias: suggestAlias(model.name, domain.definitions),
      base: { kind: 'local', path: model.path },
      mmproj: await projectorOf(model),
      load: {},
      created_at: 0,
      updated_at: 0,
    },
  });
  await domain.refreshDefinitions();
  return def.alias;
}

/** The vision projector next to an LLM (the library's pairing), or null. */
async function projectorOf(model: LocalModel): Promise<string | null> {
  if (model.kind && model.kind !== 'llm') return null;
  try {
    const found = await invoke<{ path: string } | null>('suggest_projector', { modelPath: model.path });
    return found?.path ?? null;
  } catch {
    return null;
  }
}

/**
 * The projector files this job brought: each is given to the definitions without a projector of
 * the LLM it pairs with (same pairing as the Dashboard's projector field).
 */
async function attachProjectors(job: DownloadProgress, list: LocalModel[]) {
  const wanted = job.variant_name.toLowerCase();
  const arrived = list.filter((m) => {
    if (m.kind !== 'mmproj') return false;
    const rel = repoRelative(m, job.repo_id);
    return rel !== null && stemOf(rel).toLowerCase() === wanted;
  });
  if (arrived.length === 0) return;
  try {
    await domain.refreshDefinitions();
    for (const llm of list.filter((m) => m.kind === 'llm')) {
      const defs = domain.definitions.filter(
        (d) => d.base.kind === 'local' && samePath(d.base.path, llm.path) && !d.mmproj
      );
      if (defs.length === 0) continue;
      const projector = await projectorOf(llm);
      if (!projector || !arrived.some((m) => samePath(m.path, projector))) continue;
      for (const d of defs) {
        await invoke<ModelDefinition>('save_model_definition', { definition: { ...d, mmproj: projector } });
      }
    }
    await domain.refreshDefinitions();
  } catch (e) {
    console.warn('projector for downloaded model', e);
  }
}

/** True while the default definitions of scanned library files are being made. */
export const libraryDefinitions = $state({ busy: false });
/** Library files already tried, by the library version: a file that cannot get one (missing companions) is not retried in a loop. */
const attempted = new Map<string, number>();

/**
 * Makes the default definition of every ready library file of `kinds` that was found by a scan and
 * has none (a Hub download makes its own when it finishes). The Playground pickers and the gateway
 * list definitions, so without this a model already in a library folder would not show up.
 */
export async function ensureLibraryDefinitions(kinds: readonly ModelKind[]): Promise<void> {
  const need = filesNeedingDefinition(library.models, domain.definitions, kinds).filter(
    (f) => attempted.get(f.path) !== library.version,
  );
  if (need.length === 0) return;
  for (const f of need) attempted.set(f.path, library.version);
  libraryDefinitions.busy = true;
  try {
    await invoke<string[]>('ensure_library_definitions', { kinds: [...new Set(need.map((f) => f.kind))] });
    await domain.refreshDefinitions();
  } catch (e) {
    console.warn('library definitions', e);
  } finally {
    libraryDefinitions.busy = false;
  }
}

/**
 * "Open in Playground": opens the tab that runs this library file with the file selected, making
 * its default definition first when it has none. Speech recognition (ONNX) and voice models run
 * per request and are picked by name.
 */
export async function openInPlayground(model: LocalModel, runtime?: string | null): Promise<void> {
  const target = playgroundTarget(model, runtime);
  if (!target) return;
  try {
    let alias = '';
    if (target.pick === 'definition') {
      if (model.kind === 'image') {
        await invoke<string[]>('ensure_library_definitions', { kinds: ['image'] });
        await domain.refreshDefinitions();
        const def = domain.definitions.find((d) => d.base.kind === 'local' && samePath(d.base.path, model.path));
        if (!def) {
          toast.error(t('lib.open_playground_incomplete'));
          return;
        }
        alias = def.alias;
      } else {
        alias = await ensureDefinition(model);
      }
      if (model.kind === 'image') imageSession.model = alias;
      else if (target.tab === 'transcription') speech.model = alias;
      else app.requestedPlaygroundModel = alias;
    } else if (target.speech === 'speak') {
      await tts.load();
      tts.model = (tts.models.find((x) => x.id === model.id) ?? tts.models.find((x) => x.name === model.name))?.name ?? model.name;
      tts.voice = '';
    } else {
      await asr.load();
      const known = [...asr.models, ...asr.engineModels];
      speech.model = (known.find((x) => x.id === model.id) ?? known.find((x) => x.name === model.name))?.name ?? model.name;
    }
    app.openPlaygroundTab(target.tab);
  } catch (e) {
    toast.error(t('lib.open_playground_failed', { error: localizeError(e) }));
  }
}
