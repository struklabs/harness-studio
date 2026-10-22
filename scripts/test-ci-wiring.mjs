import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

// Every scripts/test-*.mjs runs in CI: listed by name in an npm script or in a workflow.
test('every node test script is listed where CI runs it', () => {
  const { scripts } = JSON.parse(readFileSync('package.json', 'utf8'));
  const listed = Object.values(scripts).join('\n') + '\n' + readFileSync('.github/workflows/ci.yml', 'utf8');
  const missing = readdirSync('scripts')
    .filter((f) => /^test-.*\.mjs$/.test(f))
    .filter((f) => !listed.includes(`scripts/${f}`));
  assert.deepEqual(missing, [], 'add these to "test:unit" in package.json');
});

const workflow = readFileSync('.github/workflows/ci.yml', 'utf8').replaceAll('\r\n', '\n');
const secrets = workflow.match(/^  secrets:\n([\s\S]*?)(?=^  changes:)/m)?.[1];

test('secret scanning always runs and is required by ci-ok', () => {
  assert.ok(secrets);
  assert.doesNotMatch(secrets, /^    (?:if|needs):/m);
  assert.match(workflow, /pull_request:\n  push:\n    branches: \[main\]/);
  assert.match(secrets, /timeout-minutes: 10/);
  assert.match(secrets, /permissions:\n      contents: read\n    steps:/);
  assert.match(secrets, /actions\/checkout@v7/);
  assert.match(secrets, /ref: \$\{\{ github\.event\.pull_request\.head\.sha \|\| github\.sha \}\}/);
  assert.match(secrets, /fetch-depth: 0/);
  assert.match(secrets, /persist-credentials: false/);
  assert.match(workflow, /ci-ok:[\s\S]*?if: always\(\)[\s\S]*?needs: \[[^\]\n]*\bsecrets\b/);
  assert.doesNotMatch(secrets, /gitleaks\/gitleaks-action|continue-on-error/);
});

test('Gitleaks archive is pinned and verified before extraction', () => {
  assert.match(secrets, /https:\/\/github.com\/gitleaks\/gitleaks\/releases\/download\/v8\.24\.3\/gitleaks_8\.24\.3_linux_x64\.tar\.gz/);
  assert.match(secrets, /9991e0b2903da4c8f6122b5c3186448b927a5da4deef1fe45271c3793f4ee29c/);
  assert.ok(secrets.indexOf('sha256sum --check --strict') < secrets.indexOf('tar -xzf'));
});

function scan(event, base, head, options = {}) {
  const script = secrets.match(/      - name: Scan commits[\s\S]*?        run: \|\n([\s\S]*)/)[1]
    .split('\n').map((line) => line.replace(/^          /, '')).join('\n');
  const dir = mkdtempSync(join(tmpdir(), 'ci-secrets-'));
  const args = join(dir, 'args').replaceAll('\\', '/');
  const gitCalls = join(dir, 'git').replaceAll('\\', '/');
  try {
    const result = spawnSync('bash', ['-c', `
      git() {
        printf '%s\\n' "$*" >> "$GIT_CALLS"
        if [ "$1" = cat-file ]; then return "$MISSING_BASE"; fi
        if [ "$1" = fetch ]; then return "$FETCH_EXIT"; fi
      }
      gitleaks() { printf '%s\\n' "$@" > "$SCAN_ARGS"; return "$SCAN_EXIT"; }
      ${script}
    `], {
      encoding: 'utf8',
      timeout: 10_000,
      env: { ...process.env, EVENT: event, BASE_SHA: base, HEAD_SHA: head,
        SCAN_ARGS: args, GIT_CALLS: gitCalls, MISSING_BASE: '0', FETCH_EXIT: '0', SCAN_EXIT: '0', ...options },
    });
    if (result.error) throw result.error;
    return {
      status: result.status,
      args: existsSync(args) ? readFileSync(args, 'utf8') : '',
      git: existsSync(gitCalls) ? readFileSync(gitCalls, 'utf8') : '',
    };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

const base = 'a'.repeat(40);
const head = 'b'.repeat(40);

for (const event of ['pull_request', 'push']) {
  test(`${event} scans only base..head, fetching an unreachable base`, () => {
    const result = scan(event, base, head, { MISSING_BASE: '1' });
    assert.equal(result.status, 0);
    assert.match(result.args, new RegExp(`--log-opts=${base}\\.\\.${head}\\n`));
    assert.match(result.args, /--config\n\.gitleaks\.toml\n--redact\n--ignore-gitleaks-allow\n/);
    assert.ok(result.git.includes(`fetch --no-tags origin ${base}`));
  });
}

test('an available base does not need a network fetch', () => {
  const result = scan('pull_request', base, head);
  assert.equal(result.status, 0);
  assert.doesNotMatch(result.git, /fetch/);
});

test('a newly created main branch scans all ancestors of its head', () => {
  const result = scan('push', '0'.repeat(40), head);
  assert.equal(result.status, 0);
  assert.ok(result.args.includes(`--log-opts=${head}\n`));
  assert.equal(result.git, '');
});

for (const event of ['schedule', 'workflow_dispatch']) {
  test(`${event} scans full history`, () => {
    const result = scan(event, '', head);
    assert.equal(result.status, 0);
    assert.match(result.args, /--log-opts=--all --full-history\n/);
    assert.equal(result.git, '');
  });
}

test('invalid revisions, unavailable bases and scanner failures fail the job', () => {
  assert.notEqual(scan('pull_request', '', head).status, 0);
  assert.notEqual(scan('push', base, '--all').status, 0);
  assert.notEqual(scan('push', base, head, { MISSING_BASE: '1', FETCH_EXIT: '1' }).status, 0);
  assert.equal(scan('pull_request', base, head, { SCAN_EXIT: '1' }).status, 1);
});
