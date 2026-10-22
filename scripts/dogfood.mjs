#!/usr/bin/env node
// Dogfooding as repeatable QA: uses a real headless Harness (`harness serve`) the way a user
// does, through the CLI and the Control API. Run it inside the throwaway instance that
// sdk/test-server.mjs starts (new data directory, free loopback port):
//
//   node sdk/test-server.mjs -- node scripts/dogfood.mjs
//
// Needs: HARNESS_BIN (default target/debug/harness) and DOGFOOD_GGUF (a tiny GGUF, in CI the
// cached stories260K) in the environment; network for the llama.cpp runtime and the official
// registry. Steps (a failed step fails the run; the rest still run so one report shows all):
//
//   1. contract   X-Harness-API-Version header and GET /capabilities
//   2. runtime    install the managed llama.cpp build (CPU)
//   3. chat       one chat with the tiny model
//   4. tool       one tool call through the tool engine
//   5. skill      install the official `release-notes` skill from the registry
//   6. registry   install the official `json_format` tool from the registry
//   7. training   a 5-step toy fine-tune, only when `features.training_backend` is true
//                 (skipped otherwise; there is no training backend in the app yet)
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..');
const exe = process.platform === 'win32' ? 'harness.exe' : 'harness';
const bin = process.env.HARNESS_BIN ?? join(repo, 'target', 'debug', exe);
const gguf = process.env.DOGFOOD_GGUF;
const dataDir = process.env.HARNESS_SDK_TEST_DATA_DIR ?? process.env.HARNESS_DATA_DIR;
if (!dataDir) {
  console.error('run this inside sdk/test-server.mjs (HARNESS_SDK_TEST_DATA_DIR is not set)');
  process.exit(2);
}
if (!existsSync(bin)) {
  console.error(`${bin} not found: run \`cargo build -p harness-cli\` or set HARNESS_BIN`);
  process.exit(2);
}
if (!gguf || !existsSync(gguf)) {
  console.error('DOGFOOD_GGUF must name a tiny .gguf file');
  process.exit(2);
}

const instance = JSON.parse(readFileSync(join(dataDir, 'instance.json'), 'utf8'));
const { tls, host, port } = instance.api;
const apiBase = `${tls ? 'https' : 'http'}://${host.includes(':') ? `[${host}]` : host}:${port}/api/harness/v1`;
const token = readFileSync(join(dataDir, 'api-token'), 'utf8').trim();

/** Runs the CLI against the running instance; returns stdout, throws with stderr on failure. */
function harness(args, { timeoutMs = 300_000 } = {}) {
  const r = spawnSync(bin, ['--data-dir', dataDir, ...args], {
    encoding: 'utf8',
    timeout: timeoutMs,
    stdio: ['ignore', 'pipe', 'pipe'],
    windowsHide: true,
  });
  if (r.error) throw r.error;
  if (r.status !== 0) {
    throw new Error(`harness ${args.join(' ')} exited with ${r.status}\n${r.stdout}\n${r.stderr}`);
  }
  return r.stdout;
}

/** A step is retried once when `retry` is set: the runtime and registry come from GitHub. */
const results = [];
async function step(name, fn, { retry = false } = {}) {
  const started = Date.now();
  let error;
  for (let attempt = 0; attempt < (retry ? 2 : 1); attempt++) {
    try {
      const detail = await fn();
      results.push({ name, status: 'ok', detail, ms: Date.now() - started });
      console.log(`ok      ${name}${detail ? `: ${detail}` : ''}`);
      return;
    } catch (e) {
      error = e;
      if (attempt === 0 && retry) console.log(`retry   ${name}: ${String(e.message).split('\n')[0]}`);
    }
  }
  results.push({ name, status: 'fail', detail: String(error?.message ?? error), ms: Date.now() - started });
  console.error(`FAILED  ${name}: ${error?.message ?? error}`);
}
function skip(name, why) {
  results.push({ name, status: 'skipped', detail: why, ms: 0 });
  console.log(`skipped ${name}: ${why}`);
}
const assert = (cond, message) => {
  if (!cond) throw new Error(message);
};

let capabilities;
await step('contract', async () => {
  const health = await fetch(`${apiBase}/health`);
  assert(health.headers.get('x-harness-api-version') === '1', 'X-Harness-API-Version header is not 1');
  const res = await fetch(`${apiBase}/capabilities`);
  assert(res.ok, `GET /capabilities answered ${res.status}`);
  capabilities = await res.json();
  assert(capabilities.api_version === 1, `api_version ${capabilities.api_version}`);
  assert(capabilities.module_manifest_versions.includes(1), 'manifest version 1 is not supported');
  // Authenticated endpoints carry the header too.
  const status = await fetch(`${apiBase}/status`, { headers: { Authorization: `Bearer ${token}` } });
  assert(status.ok && status.headers.get('x-harness-api-version') === '1', 'authenticated response lacks the version header');
  return `API v${capabilities.api_version}, app ${capabilities.app_version}`;
});

await step('runtime', () => {
  harness(['--json', '--yes', 'runtime', 'managed', 'install', '--backend', 'cpu'], { timeoutMs: 600_000 });
  const status = JSON.parse(harness(['--json', 'runtime', 'managed', 'status']));
  return `llama.cpp ${status.installed?.version ?? status.version ?? 'installed'}`;
}, { retry: true });

await step('chat', () => {
  const out = harness(
    ['run', gguf, '--max-tokens', '24', '--temperature', '0', '--timeout', '240', '--unload', 'Once upon a time'],
    { timeoutMs: 360_000 },
  );
  assert(out.trim().length > 0, 'the model answered with nothing');
  return `${out.trim().length} characters`;
});

await step('tool', () => {
  const work = mkdtempSync(join(tmpdir(), 'harness-dogfood-'));
  try {
    writeFileSync(join(work, 'hello.txt'), 'dogfood-marker-4711\n');
    const out = harness(['--json', '--yes', 'tools', 'run', 'read_file', '--args', '{"path":"hello.txt"}', '--workdir', work]);
    assert(out.includes('dogfood-marker-4711'), `read_file did not return the file: ${out.slice(0, 300)}`);
    return 'read_file';
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});

await step('skill', () => {
  harness(['--yes', 'skills', 'install', 'release-notes']);
  const skills = JSON.parse(harness(['--json', 'skills', 'list']));
  assert(skills.some((s) => s.name === 'release-notes' && !s.error), 'release-notes is not in the skill list');
  return 'release-notes';
}, { retry: true });

await step('registry', () => {
  harness(['--yes', 'tools', 'install', 'json_format']);
  const tools = JSON.parse(harness(['--json', 'tools', 'installed']));
  assert(tools.some((t) => t.name === 'json_format' && !t.error), 'json_format is not in the installed tools');
  return 'json_format';
}, { retry: true });

if (capabilities?.features?.training_backend) {
  await step('training', () => {
    throw new Error('capabilities.features.training_backend is true: add the 5-step toy fine-tune smoke test here');
  });
} else {
  skip('training', 'no training backend (capabilities.features.training_backend is false)');
}

const failed = results.filter((r) => r.status === 'fail');
console.log(`\n${results.length - failed.length}/${results.length} steps ok (${results.filter((r) => r.status === 'skipped').length} skipped)`);
if (process.env.GITHUB_STEP_SUMMARY) {
  const rows = results.map((r) => `| ${r.name} | ${r.status} | ${(r.ms / 1000).toFixed(1)} s | ${String(r.detail ?? '').split('\n')[0].replaceAll('|', '/')} |`);
  writeFileSync(
    process.env.GITHUB_STEP_SUMMARY,
    ['## Dogfood', '', '| Step | Result | Time | Detail |', '| --- | --- | --- | --- |', ...rows, ''].join('\n'),
    { flag: 'a' },
  );
}
process.exitCode = failed.length ? 1 : 0;
