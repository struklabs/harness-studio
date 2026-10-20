#!/usr/bin/env node
// Runs a command against a throwaway `harness serve`: a new data directory in the temp folder
// (never the user's), a free loopback port, no gateway and no restored servers. The command
// gets HARNESS_SDK_TEST_DATA_DIR (the instance's data directory: `instance.json`, `api-token`).
//
//   node sdk/test-server.mjs -- npm test --prefix sdk/typescript
//
// The binary: HARNESS_BIN, else target/debug/harness (`cargo build -p harness-cli`).
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..');
const exe = process.platform === 'win32' ? 'harness.exe' : 'harness';
const bin = process.env.HARNESS_BIN ?? join(repo, 'target', 'debug', exe);
const sep = process.argv.indexOf('--');
const command = sep >= 0 ? process.argv.slice(sep + 1) : [];
if (!command.length) {
  console.error('usage: node sdk/test-server.mjs -- <command...>');
  process.exit(2);
}
if (!existsSync(bin)) {
  console.error(`${bin} not found: run \`cargo build -p harness-cli\` or set HARNESS_BIN`);
  process.exit(2);
}

const freePort = () =>
  new Promise((resolve, reject) => {
    const s = createServer();
    s.once('error', reject);
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });

const dataDir = mkdtempSync(join(tmpdir(), 'harness-sdk-test-'));
const port = await freePort();
const env = { ...process.env, HARNESS_DATA_DIR: dataDir };
const server = spawn(bin, ['serve', '--listen', '127.0.0.1', '--port', String(port), '--no-gateway', '--no-restore'], {
  env,
  stdio: ['ignore', 'inherit', 'inherit'],
});
let exited = false;
server.on('exit', () => (exited = true));

let code = 1;
try {
  // Ready when the owner has described itself and answers /health.
  const deadline = Date.now() + 60_000;
  for (;;) {
    if (exited) throw new Error('harness serve exited during startup');
    if (Date.now() > deadline) throw new Error('harness serve did not start within 60 s');
    if (existsSync(join(dataDir, 'instance.json'))) {
      const ok = await fetch(`http://127.0.0.1:${port}/api/harness/v1/health`).then((r) => r.ok, () => false);
      if (ok) break;
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  // On Windows through the shell (npm is a .cmd), as one quoted command line.
  const quote = (s) => (/[\s"]/.test(s) ? `"${s.replaceAll('"', '\\"')}"` : s);
  const options = { stdio: 'inherit', env: { ...env, HARNESS_SDK_TEST_DATA_DIR: dataDir } };
  const r =
    process.platform === 'win32'
      ? spawnSync(command.map(quote).join(' '), { ...options, shell: true })
      : spawnSync(command[0], command.slice(1), options);
  code = r.status ?? 1;
} catch (e) {
  console.error(String(e));
} finally {
  if (!exited) {
    const token = (() => {
      try {
        return readFileSync(join(dataDir, 'api-token'), 'utf8').trim();
      } catch {
        return '';
      }
    })();
    await fetch(`http://127.0.0.1:${port}/api/harness/v1/shutdown`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${token}` },
    }).catch(() => {});
    const stopped = await new Promise((resolve) => {
      if (exited) return resolve(true);
      const t = setTimeout(() => resolve(false), 15_000);
      server.once('exit', () => (clearTimeout(t), resolve(true)));
    });
    if (!stopped) server.kill();
  }
  rmSync(dataDir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
}
// Let pending fetch/child-process handles close before Node tears down libuv on Windows.
process.exitCode = code;
