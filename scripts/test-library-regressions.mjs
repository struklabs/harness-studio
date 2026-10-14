import assert from 'node:assert/strict';
import { test } from 'node:test';
import { registerHooks } from 'node:module';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { transpileModule } from 'typescript';
import { compileModule } from 'svelte/compiler';

const state = globalThis.__libraryRegression = {
  calls: [], files: [], definitions: [], language: 'tr-TR',
  app: { servers: [], openPlaygroundTab(tab) { this.tab = tab; } },
  imageSession: { model: 'sd_turbo-q8_0' },
  tts: { models: [], async load() { state.calls.push('tts'); } },
  asr: { async load() { state.calls.push('asr'); } },
  speech: {},
  async invoke(command) {
    state.calls.push(command);
    if (command === 'scan_local_models') return state.scan ? state.scan() : state.files;
    if (command === 'ensure_library_definitions') return [];
    if (command === 'sync_image_definitions') return [];
    throw new Error(command);
  },
};
state.domain = {
  get definitions() { return state.definitions; },
  async refreshDefinitions() { state.calls.push('definitions'); },
};
const mocks = {
  ipc: 'export const invoke = (...args) => globalThis.__libraryRegression.invoke(...args);',
  app: 'export const app = globalThis.__libraryRegression.app; export const DEFAULT_SERVER_ID = "default";',
  i18n: 'export const t = key => key; export const locale = () => globalThis.__libraryRegression.language;',
  toast: 'export const toast = { success() {}, error() {} }; export const ACTION_TOAST_DURATION = 1;',
  queue: 'export class DownloadQueue { jobs = []; }',
  domain: `export const domain = globalThis.__libraryRegression.domain;
    export const samePath = (a, b) => a.replaceAll(String.fromCharCode(92), '/').toLowerCase() === b.replaceAll(String.fromCharCode(92), '/').toLowerCase();`,
  follow: 'export const defaultLoadTarget = () => null; export const isChatLoadable = () => false; export const shouldAutoLoad = () => false;',
  image: 'export const isImageDefinition = () => false;',
  imageSession: 'export const imageSession = globalThis.__libraryRegression.imageSession;',
  speech: 'export const speech = globalThis.__libraryRegression.speech;',
  tts: 'export const tts = globalThis.__libraryRegression.tts;',
  asr: 'export const asr = globalThis.__libraryRegression.asr;',
  network: 'export const localizeError = String;',
  activity: 'export const holdReleaseWhile = () => {};',
  engineInstall: 'export const engineInstall = { busy() { return false; } };',
};
registerHooks({
  resolve(specifier, context, next) {
    const byName = {
      '#lib/ipc.js': 'ipc', '#lib/stores/app.svelte.js': 'app', '#lib/i18n.svelte.js': 'i18n',
      'svelte-sonner': 'toast', '#lib/toast.js': 'toast',
      '#lib/components/hub/downloadQueue.svelte.js': 'queue', '#lib/stores/domain.svelte.js': 'domain',
      '#lib/downloadFollowUp.js': 'follow', '#lib/components/playground/image.js': 'image',
      '#lib/components/playground/image.svelte.js': 'imageSession', '#lib/components/playground/speech.svelte.js': 'speech',
      '#lib/components/playground/tts.svelte.js': 'tts', '#lib/components/playground/asr.svelte.js': 'asr',
      '#lib/network.js': 'network', '#lib/windowActivity.js': 'activity',
      '#lib/stores/engineInstall.svelte.js': 'engineInstall',
    };
    if (byName[specifier]) return { url: `mock:${byName[specifier]}`, shortCircuit: true };
    if (specifier.startsWith('#lib/'))
      return next(new URL(`../src/lib/${specifier.slice(5).replace(/\.js$/, '.ts')}`, import.meta.url).href, context);
    if (specifier.startsWith('./') && context.parentURL) {
      const url = new URL(`${specifier}.ts`, context.parentURL);
      if (existsSync(fileURLToPath(url))) return next(url.href, context);
    }
    return next(specifier, context);
  },
  load(url, context, next) {
    if (url.startsWith('mock:')) return { format: 'module', source: mocks[url.slice(5)], shortCircuit: true };
    if (url.endsWith('.svelte.ts')) {
      const source = transpileModule(readFileSync(fileURLToPath(url), 'utf8'), { compilerOptions: { target: 99, module: 99 } }).outputText;
      return { format: 'module', source: compileModule(source, { filename: fileURLToPath(url), generate: 'server' }).js.code, shortCircuit: true };
    }
    return next(url, context);
  },
});
const { libraryKindKey } = await import('../src/lib/libraryDefinitions.ts');
const { inTab, tabCounts } = await import('../src/lib/components/library/filters.ts');
const { formatBytes } = await import('../src/lib/format.ts');
const { library } = await import('../src/lib/stores/library.svelte.ts');
const { downloads, modelOfJob, openInPlayground } = await import('../src/lib/stores/downloads.svelte.ts');
const file = (name, kind, task, architecture) => ({ id: name, name, filename: name, path: `C:/Models/${name}`, kind, task, architecture, is_ready: true });

test('library audio subtitles follow the real engine and task', () => {
  for (const name of ['kokoro-82m-q8_0.gguf', 'pocket-tts-english-q8_0.gguf']) {
    assert.equal(libraryKindKey(file(name, 'tts'), 'crispasr'), 'lib.kind_tts_crispasr');
    assert.equal(libraryKindKey(file(name, 'tts')), 'lib.kind_tts_crispasr');
  }
  assert.equal(libraryKindKey(file('piper-orig.gguf', 'tts', undefined, 'audiocpp'), 'audio.cpp'), 'lib.kind_tts_audiocpp');
  assert.equal(libraryKindKey(file('apollo-orig.gguf', 'pipeline', undefined, 'audiocpp'), 'audio.cpp'), 'lib.kind_audio_process');
  assert.equal(libraryKindKey(file('kokoro.onnx', 'tts'), 'sherpa-onnx'), 'lib.kind_tts');
  assert.equal(libraryKindKey(file('ner', 'pipeline'), 'onnxruntime'), 'lib.kind_pipeline');
});

test('image category and counts include both diffusers image tasks', () => {
  const entries = ['text-to-image', 'image-to-image', 'image-classification'].map(task => ({ model: file(task, 'pipeline', task), record: { runtime: 'python-hf' }, draft_for: [] }));
  assert.deepEqual(entries.map(e => inTab(e, 'image')), [true, true, false]);
  assert.equal(tabCounts(entries).image, 2);
});

test('shared size formatter promotes 1000 MB, keeping locale decimals', () => {
  assert.equal(formatBytes(999 * 1024 ** 2), '999 MB');
  assert.equal(formatBytes(1000 * 1024 ** 2), '0,98 GB');
  assert.equal(formatBytes(1012.91 * 1024 ** 2), '0,99 GB');
  state.language = 'en-US';
  assert.equal(formatBytes(1011.39 * 1024 ** 2), '0.99 GB');
  assert.equal(formatBytes(1024 ** 3), '1 GB');
  assert.equal(formatBytes(null), '0 B');
  state.language = 'tr-TR';
});

test('a changed library rescans rather than reusing pre-completion directory contents', async () => {
  let resolveOld;
  state.scan = () => new Promise(resolve => { resolveOld = resolve; });
  const pending = library.refresh();
  library.markChanged();
  const current = library.refresh();
  assert.equal(current, pending);
  state.scan = () => Promise.resolve([file('new.gguf', 'tts')]);
  resolveOld([]);
  assert.deepEqual(await current, [file('new.gguf', 'tts')]);
  assert.equal(library.loaded, true);
  state.scan = null;
});

test('every completed download refreshes definitions and speech pickers, including repo folders', async () => {
  for (const model of [
    file('segmind/tiny-sd', 'pipeline', 'text-to-image'),
    file('cstr/kokoro-82m-GGUF/kokoro-82m-q8_0.gguf', 'tts'),
    file('cstr/pocket-tts-GGUF/pocket-tts-english-q8_0.gguf', 'tts'),
    file('audio-cpp/audio.cpp-gguf/Piper-TTS-GGUF/piper-en-us-lessac-medium-orig.gguf', 'tts'),
    file('audio-cpp/audio.cpp-gguf/Apollo-GGUF/apollo-orig.gguf', 'pipeline'),
  ]) {
    state.files = [model];
    state.calls = [];
    const version = library.version;
    const job = { repo_id: model.name.split('/').slice(0, 2).join('/'), variant_name: model.filename.split('/').at(-1).replace(/\.gguf$/, '') };
    await downloads.onCompleted(job);
    assert.equal(library.version, version + 1);
    assert.deepEqual(library.models, [model]);
    assert.ok(state.calls.includes('definitions'));
    assert.ok(state.calls.includes('tts'));
    assert.ok(state.calls.includes('asr'));
    assert.equal(modelOfJob(job, [model]), model);
  }
  assert.equal(modelOfJob({ repo_id: 'segmind/tiny-sd', variant_name: 'Python' }, [file('segmind/tiny-sd', 'pipeline')]).name, 'segmind/tiny-sd');
});

test('opening either diffusers image task selects its definition over the previous image model', async () => {
  for (const task of ['text-to-image', 'image-to-image']) {
    const model = file('segmind/tiny-sd', 'pipeline', task);
    state.definitions = [{ id: 'tiny', alias: 'tiny-sd', base: { kind: 'local', path: model.path } }];
    state.imageSession.model = 'sd_turbo-q8_0';
    await openInPlayground(model, 'python-hf');
    assert.equal(state.imageSession.model, 'tiny-sd');
    assert.equal(state.app.tab, 'image');
  }
});

test('an older speech overview cannot overwrite the models discovered after a download', async () => {
  const { tts } = await import('../src/lib/components/playground/tts.svelte.ts');
  const { asr } = await import('../src/lib/components/playground/asr.svelte.ts');
  const invoke = state.invoke;
  try {
    for (const store of [tts, asr]) {
      let resolveOld;
      state.invoke = () => new Promise(resolve => { resolveOld = resolve; });
      const old = store.load();
      const overview = { models: [{ name: 'new-model', voices: [] }], catalog: [], runtime_installed: true };
      state.invoke = async () => overview;
      await store.load();
      resolveOld({ models: [], catalog: [], runtime_installed: false });
      await old;
      assert.deepEqual(store.overview, overview);
      assert.equal(store.models[0].name, 'new-model');
    }
  } finally {
    state.invoke = invoke;
  }
});
