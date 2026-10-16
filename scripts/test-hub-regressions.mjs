import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { parseSearchQuery } from '../src/lib/components/hub/searchParser.ts';
import { isCompanionVariant, pickProjectorVariant, recommendFloatVariant, recommendVariant } from '../src/lib/components/hub/recommend.ts';
import { needsPrismFork } from '../src/lib/components/hub/forkQuants.ts';
import { engineCompatibility } from '../src/lib/components/hub/engineCompatibility.ts';
import { localVariantFor } from '../src/lib/components/hub/variantFiles.ts';

test('cached engine requirements follow shared installed state after an install anywhere', () => {
  const installed = new Set();
  const isInstalled = (family) => installed.has(family);
  for (const family of ['whisper.cpp', 'stable-diffusion.cpp', 'sherpa-onnx', 'python-hf', 'onnxruntime', 'lemonade', 'mlx-audio']) {
    const cached = { status: 'needs_runtime', runtime: family };
    assert.equal(engineCompatibility(cached, isInstalled), cached);
    installed.add(family);
    assert.deepEqual(engineCompatibility(cached, isInstalled), { status: 'runnable', runtime: family });
    assert.equal(cached.status, 'needs_runtime');
    installed.delete(family);
    assert.equal(engineCompatibility(cached, isInstalled), cached);
  }
});

test('engine availability preserves platform and format refusals and unrelated requirements', () => {
  for (const compat of [
    { status: 'unsupported_platform', runtime: 'onnxruntime', os: 'macos', arch: 'x86_64' },
    { status: 'unsupported', format: 'ctranslate2' },
    { status: 'runnable', runtime: 'llama.cpp' },
  ]) {
    assert.equal(engineCompatibility(compat, () => true), compat);
  }
  const missing = { status: 'needs_runtime', runtime: 'sherpa-onnx' };
  assert.equal(engineCompatibility(missing, (family) => family === 'whisper.cpp'), missing);
});

const themeCss = readFileSync(new URL('../src/app.css', import.meta.url), 'utf8');
const cssRules = [...themeCss.matchAll(/([^{}]+)\{([^{}]*)\}/g)];

function luminance(hex) {
  const channels = hex.match(/[a-f\d]{2}/gi).map((channel) => {
    const value = parseInt(channel, 16) / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
}

function contrast(foreground, background) {
  const values = [luminance(foreground), luminance(background)].sort((a, b) => b - a);
  return (values[0] + 0.05) / (values[1] + 0.05);
}

test('unavailable HC card titles, owners and refusal labels retain AAA contrast', () => {
  for (const theme of ['hc-light', 'hc-dark']) {
    const selector = `[data-theme="${theme}"]`;
    const tokens = (selector) => Object.fromEntries(
      [...cssRules.find(([_, header]) => header.trim().endsWith(selector))[2].matchAll(/--([\w-]+):\s*(#[a-f\d]{6});/gi)]
        .map(([_, name, value]) => [name, value])
    );
    const base = tokens(selector);
    for (const palette of [base, { ...base, ...tokens(`[data-cvd="on"]${selector}`) }]) {
      for (const foreground of ['foreground', 'muted-foreground', 'warning']) {
        for (const surface of ['background', 'card', 'muted']) {
          assert.ok(contrast(palette[foreground], palette[surface]) >= 7, `${theme} ${foreground} on ${surface}`);
        }
      }
    }
  }
});

test('unavailable opacity override is HC-only and retains a non-colour state cue', () => {
  const rule = cssRules.find(([_, selector]) => selector.includes('[data-unavailable="true"]'));
  assert.ok(rule);
  assert.match(rule[1], /\[data-theme\^="hc-"\]\s+\[data-unavailable="true"\]\s*$/);
  assert.match(rule[2], /opacity:\s*1;/);
  assert.match(rule[2], /outline:\s*1px dashed var\(--border\);/);
  const card = readFileSync(new URL('../src/lib/components/hub/RepoCard.svelte', import.meta.url), 'utf8');
  assert.match(card, /data-unavailable=\{unsupported \|\| undefined\}/);
  assert.match(card, /unsupported && 'opacity-60'/);
  const drawer = readFileSync(new URL('../src/lib/components/hub/VariantDrawer.svelte', import.meta.url), 'utf8');
  assert.match(drawer, /data-unavailable=\{!!variant\.not_runnable \|\| undefined\}/);
});

test('Whisper variants become local only after their own file is downloaded', () => {
  const small = { name: 'ggml-small', shards: [{ filename: 'ggml-small.bin' }] };
  const local = { path: '/library/ggerganov/whisper.cpp/ggml-small.bin' };
  assert.equal(localVariantFor(small, new Map()), undefined);
  assert.equal(localVariantFor(small, new Map([['ggml-base.bin', local]])), undefined);
  assert.equal(localVariantFor(small, new Map([['ggml-small.bin', local]])), local);
});

test('split variants remain downloadable until every shard is local', () => {
  const split = { name: 'model', shards: [{ filename: 'Sub/Model-00001.gguf' }, { filename: 'Sub\\Model-00002.gguf' }] };
  const local = { path: '/library/owner/repo/Sub/Model-00001.gguf' };
  const files = new Map([['sub/model-00001.gguf', local]]);
  assert.equal(localVariantFor(split, files), undefined);
  files.set('sub/model-00002.gguf', local);
  assert.equal(localVariantFor(split, files), local);
  files.delete('sub/model-00001.gguf');
  assert.equal(localVariantFor(split, files), undefined);
});

test('repo-folder variants are local without individual files; prefilled variants are not', () => {
  const folder = { path: '/library/owner/repo' };
  const variant = { name: 'repo', shards: [] };
  assert.equal(localVariantFor(variant, new Map()), undefined);
  assert.equal(localVariantFor(variant, new Map(), folder), folder);
});

test('a downloaded file wins over the repo folder', () => {
  const folder = { path: '/library/owner/repo' };
  const file = { path: '/library/owner/repo/model.gguf' };
  const variant = { name: 'model', shards: [{ filename: 'model.gguf' }] };
  assert.equal(localVariantFor(variant, new Map([['model.gguf', file]]), folder), file);
  assert.equal(localVariantFor(variant, new Map(), folder), folder);
});

test('exact repository IDs retain GGUF and quantization names', () => {
  for (const repo of ['unsloth/Qwen3-4B-GGUF', 'owner/model-Q4_K_M', 'OWNER/Model.gguf']) {
    assert.deepEqual(parseSearchQuery(` ${repo} `), { cleanedQuery: repo, detectedQuant: null });
  }
  assert.deepEqual(parseSearchQuery('Qwen3 GGUF (Q4_K_M)'), { cleanedQuery: 'Qwen3', detectedQuant: 'Q4_K_M' });
});

const variant = (name, size) => ({ name, total_size_bytes: size, quantization: 'Q8_0', shards: [{ filename: `${name}.gguf` }] });

test('current diffusion weights precede larger archived weights', () => {
  const current = variant('sd_turbo-f16-q8_0', 1_880_000_000);
  for (const folder of ['old', 'Legacy', 'deprecated', 'nested/OLD']) {
    const archived = variant(`${folder}/sd_turbo-q8_0`, 3_140_000_000);
    const fits = new Map([[current.name, 'full_gpu'], [archived.name, 'full_gpu']]);
    assert.equal(recommendVariant([archived, current], fits), current);
    assert.equal(recommendVariant([archived], fits), archived);
    assert.equal(recommendVariant([archived, { ...current, total_size_bytes: 0 }], fits), archived);
    assert.equal(recommendVariant([{ ...archived, name: 'display-name' }, current], new Map([['display-name', 'full_gpu'], [current.name, 'full_gpu']])), current);
  }
});

test('ordinary variant quality and fit ranking is preserved', () => {
  const smaller = variant('current-small', 1_000);
  const larger = variant('current-large', 2_000);
  assert.equal(recommendVariant([smaller, larger], new Map([[smaller.name, 'full_gpu'], [larger.name, 'full_gpu']])), larger);
  assert.equal(recommendVariant([smaller, larger], new Map([[smaller.name, 'hybrid'], [larger.name, 'hybrid']])), smaller);
});

const gguf = (name, quantization, size) => ({ name, total_size_bytes: size, quantization, shards: [{ filename: `${name}.gguf` }] });

test('projectors, drafts, and image components are never the recommended main model', () => {
  const main = gguf('Model-Q8_0', 'Q8_0', 8_000_000_000);
  for (const companion of [
    gguf('mmproj-BF16', 'mmproj', 900_000_000),
    gguf('Model-mmproj-F16', 'F16', 900_000_000),
    gguf('MTP/mtp-Qwen3.8-27B-Q4_0', 'Q4_0', 1_370_000_000),
    gguf('draft-Model-Q4_0', 'Q4_0', 1_000_000_000),
    gguf('Model-assistant-Q4_0', 'Q4_0', 1_000_000_000),
    gguf('eagle3/model', 'Q4_0', 1_000_000_000),
    { name: 'ae', quantization: 'component', total_size_bytes: 300_000_000, shards: [{ filename: 'ae.safetensors' }] },
    { name: 'sd_xl_offset_example-lora_1.0', quantization: 'lora', total_size_bytes: 49_000_000, shards: [{ filename: 'sd_xl_offset_example-lora_1.0.safetensors' }] },
  ]) {
    for (const tier of ['full_gpu', 'hybrid', 'cpu_heavy']) {
      const fits = new Map([[companion.name, tier], [main.name, tier]]);
      assert.equal(recommendVariant([companion, main], fits), main, `${companion.name} ${tier}`);
    }
    // Main files out of memory: the companion is still not recommended.
    assert.equal(recommendVariant([companion, main], new Map([[companion.name, 'full_gpu'], [main.name, 'oom_risk']])), null, companion.name);
  }
  assert.equal(isCompanionVariant(main), false);
  assert.equal(isCompanionVariant(gguf('Model-Q4_K_M', 'Q4_K_M', 1)), false);
});

test('a main GGUF variant brings the best matching projector', () => {
  const v = (file, quantization, size = 1) => ({ name: file.replace('.gguf', ''), quantization, total_size_bytes: size, shards: [{ filename: file }] });
  const main = v('m-Q4_K_M.gguf', 'Q4_K_M', 100);
  const f16 = v('mmproj-m-F16.gguf', 'mmproj', 900);
  const bf16 = v('mmproj-m-BF16.gguf', 'mmproj', 900);
  const f32 = v('mmproj-m-F32.gguf', 'mmproj', 1800);
  assert.equal(pickProjectorVariant([main, f32, bf16, f16], main), f16);
  assert.equal(pickProjectorVariant([main, f32, bf16], main), bf16);
  assert.equal(pickProjectorVariant([main, f32], main), f32);
  const odd = v('mmproj-m-Q8_0.gguf', 'mmproj', 500);
  assert.equal(pickProjectorVariant([main, odd], main), odd);
  assert.equal(pickProjectorVariant([main], main), null);
  assert.equal(pickProjectorVariant([f16, main], f16), null);
  const sub = v('sub/m-Q4_K_M.gguf', 'Q4_K_M');
  const subProj = v('sub/mmproj-F32.gguf', 'mmproj');
  assert.equal(pickProjectorVariant([sub, f16, subProj], sub), subProj);
});

const prec = (name, quantization, size) => ({ name, quantization, total_size_bytes: size, shards: [{ filename: `${name}.safetensors` }] });

test('float-only repos recommend FP16 over FP32 and never an unparsed precision', () => {
  const unknown = prec('sd_xl_turbo_1.0', 'SAFETENSORS', 13_880_000_000);
  const fp16 = prec('sd_xl_turbo_1.0_fp16', 'FP16', 6_940_000_000);
  const fp32 = prec('sd_xl_base_fp32', 'FP32', 13_880_000_000);
  const bf16 = prec('x_bf16', 'BF16', 6_940_000_000);
  const all = (vs, c) => new Map(vs.map((v) => [v.name, c]));
  // Unparsed precision no longer wins in recommendVariant when a known one exists.
  assert.equal(recommendVariant([unknown, fp16], all([unknown, fp16], 'full_gpu')), null);
  assert.equal(recommendFloatVariant([unknown, fp16], all([unknown, fp16], 'full_gpu')), fp16);
  assert.equal(recommendFloatVariant([fp32, fp16], all([fp32, fp16], 'full_gpu')), fp16);
  assert.equal(recommendFloatVariant([fp32, bf16], all([fp32, bf16], 'tight')), bf16);
  // FP16 fits only partially while FP32 fits nothing better: still FP16.
  assert.equal(recommendFloatVariant([fp32, fp16], new Map([[fp32.name, 'hybrid'], [fp16.name, 'hybrid']])), fp16);
  // A tier that fits beats a float that does not.
  assert.equal(recommendFloatVariant([fp32, fp16], new Map([[fp32.name, 'full_gpu'], [fp16.name, 'hybrid']])), fp32);
  assert.equal(recommendFloatVariant([unknown], all([unknown], 'full_gpu')), null);
});

test('an unlabelled GGUF does not beat a known quant; unlabelled-only keeps working', () => {
  const unlabelled = { name: 'M', quantization: 'Unknown', total_size_bytes: 3_560_000_000, shards: [{ filename: 'M.gguf' }] };
  const q8 = { name: 'M-Q8_0', quantization: 'Q8_0', total_size_bytes: 1_890_000_000, shards: [{ filename: 'M-Q8_0.gguf' }] };
  const fits = new Map([[unlabelled.name, 'full_gpu'], [q8.name, 'full_gpu']]);
  assert.equal(recommendVariant([unlabelled, q8], fits), q8);
  assert.equal(recommendVariant([unlabelled], fits), unlabelled);
});

test('PrismML fork quant types are detected by label or file name', () => {
  assert.ok(needsPrismFork('PQ2_0'));
  assert.ok(needsPrismFork(undefined, 'Ternary-Bonsai-2-27B-PTQ1_0.gguf'));
  assert.ok(needsPrismFork('sub/Ternary-Bonsai-2-27B-pq2_0.gguf'));
  assert.ok(!needsPrismFork('Q2_0', 'TQ1_0', 'model-Q4_K_M.gguf', null));
});

test('ChrisCole Qwen edit CPU recommendation excludes split text encoders and VAE', () => {
  const main = gguf('split/diffusion_models/qwen-image-edit-2511-uncensored-Q4_K_M', 'Q4_K_M', 13_100_000_000);
  const encoder = gguf('split/text_encoders/Qwen2.5-VL-7B-Instruct-q4_0', 'Q4_0', 4_000_000_000);
  const projector = gguf('split/text_encoders/Qwen2.5-VL-7B-Instruct-mmproj-f16', 'mmproj', 1_700_000_000);
  const vae = { ...gguf('split/vae/qwen_image_vae', 'FP32', 250_000_000), shards: [{ filename: 'split/vae/qwen_image_vae.safetensors' }] };
  const variants = [encoder, projector, vae, main];
  assert.equal(recommendVariant(variants, new Map(variants.map(v => [v.name, 'cpu_heavy']))), main);
  for (const companion of [encoder, projector, vae]) {
    assert.equal(isCompanionVariant(companion), true);
    assert.equal(isCompanionVariant({ ...companion, name: 'display-name' }), true);
    assert.equal(recommendVariant([companion], new Map([[companion.name, 'full_gpu']])), null);
  }
  assert.equal(isCompanionVariant(main), false);
  for (const folder of ['text_encoder', 'vae', 'vision_encoder', 'audio_encoders', 'tokenizer']) {
    assert.equal(isCompanionVariant(gguf(`nested/${folder}/model-Q4_0`, 'Q4_0', 1)), true);
  }
});

test('every backend refusal has frontend mapping and both translations', () => {
  const read = (p) => readFileSync(new URL('../' + p, import.meta.url), 'utf8');
  const rust = read('crates/harness-core/src/hub_tasks.rs').split('pub enum NotRunnable {')[1].split('}')[0];
  const inference = read('src/lib/components/hub/inference.ts');
  const locales = readdirSync(new URL('../src/lib/locales/', import.meta.url)).filter((p) => p.endsWith('.ts')).map((p) => read('src/lib/locales/' + p)).join('\n');
  for (const [, variant] of rust.matchAll(/^    ([A-Z]\w*),/gm)) {
    const key = variant.replace(/[A-Z]/g, (c, i) => (i ? '_' : '') + c.toLowerCase());
    const locale = new RegExp('  ' + key + ": '([^']+)'").exec(inference)?.[1];
    assert.ok(locale, key);
    assert.equal(locales.split("'" + locale + "':").length - 1, 2, locale);
  }
  assert.match(inference, /foreign_speech_gguf: 'hub.refusal_short_foreign_speech_gguf'/);
  assert.equal(locales.split("'hub.refusal_short_foreign_speech_gguf':").length - 1, 2);
});
