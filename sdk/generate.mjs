#!/usr/bin/env node
// Generates the Control API clients. sdk/openapi.json is committed (the one source); the
// generated client code is not:
//
//   sdk/openapi.json                        <- `cargo run -p harness-api --example dump_openapi`
//   sdk/typescript/src/schema.ts            <- openapi-typescript (pinned in sdk/typescript)   [gitignored]
//   sdk/python/src/harness_studio_api/      <- openapi-python-client (pinned below, run by uv) [gitignored]
//
// Usage: node sdk/generate.mjs [--from-snapshot] [--check] [--skip-typescript] [--skip-python]
//   (none)           refresh openapi.json from the Rust code, then generate both clients.
//   --from-snapshot  generate from the committed openapi.json without cargo (the packages'
//                    build and test commands run this).
//   --check          only compare openapi.json with the Rust code; fail when it is out of date.
// Needs: cargo (not with --from-snapshot); `npm ci` in sdk/typescript; uv (`uv`/`uvx` on PATH
// or UV=<path to uv>) for Python.
import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const PYTHON_GENERATOR = 'openapi-python-client==0.29.1';
const RUFF = 'ruff==0.16.9';

const sdk = dirname(fileURLToPath(import.meta.url));
const repo = join(sdk, '..');
const args = process.argv.slice(2);
const check = args.includes('--check');
const fromSnapshot = args.includes('--from-snapshot');
const specFile = join(sdk, 'openapi.json');

function run(cmd, cmdArgs, opts = {}) {
  const r = spawnSync(cmd, cmdArgs, { stdio: ['ignore', 'pipe', 'inherit'], encoding: 'utf8', ...opts });
  if (r.error) throw r.error;
  if (r.status !== 0) {
    process.stderr.write(r.stdout ?? '');
    throw new Error(`${cmd} ${cmdArgs.join(' ')} exited with ${r.status}`);
  }
  return r.stdout;
}

/** uv as `[command, ...prefix]`: `uv tool run` runs a pinned tool without installing it. */
function uv() {
  if (process.env.UV) return [process.env.UV, 'tool', 'run'];
  for (const name of ['uvx', 'uv']) {
    const r = spawnSync(name, ['--version'], { stdio: 'ignore' });
    if (r.status === 0) return name === 'uvx' ? ['uvx'] : ['uv', 'tool', 'run'];
  }
  throw new Error('uv not found: install it (https://docs.astral.sh/uv/) or set UV=<path to uv>');
}

/** The OpenAPI document as the Rust code prints it (`to_pretty_json`, verbatim: a JSON round
 * trip in JavaScript would turn `0.0` into `0`). */
function currentSpec() {
  const text = run('cargo', ['run', '-q', '-p', 'harness-api', '--example', 'dump_openapi'], { cwd: repo });
  JSON.parse(text);
  return text.replaceAll('\r\n', '\n').trimEnd() + '\n';
}

/**
 * The document as openapi-python-client reads it. Two valid OpenAPI 3.1 forms it rejects
 * (dropping the schema and every endpoint that uses it) are rewritten; the wire format is the
 * same:
 * - a `default` on an object-valued property (`limits: RateLimits = {...}`): removed, the
 *   server applies it anyway;
 * - an inline `allOf` variant of a `oneOf` (serde's internally tagged enums: `{kind: "github",
 *   ...GithubInstall}`): moved to its own component, `<Parent><Kind>`.
 */
function pythonSpec(spec) {
  const schemas = spec.components.schemas;
  const isObjectRef = (s) =>
    !!s && (s.$ref || ['oneOf', 'anyOf', 'allOf'].some((k) => s[k]?.some((m) => m.$ref)));
  const visit = (s) => {
    if (!s || typeof s !== 'object') return;
    for (const p of Object.values(s.properties ?? {})) {
      if ('default' in p && isObjectRef(p)) delete p.default;
      visit(p);
    }
    for (const k of ['oneOf', 'anyOf', 'allOf']) s[k]?.forEach(visit);
    if (s.items) visit(s.items);
  };
  const pascal = (v) => String(v).replace(/(^|[_\-\s])(\w)/g, (_, __, c) => c.toUpperCase());
  for (const [name, schema] of Object.entries({ ...schemas })) {
    visit(schema);
    for (const k of ['oneOf', 'anyOf']) {
      schema[k]?.forEach((member, i) => {
        if (member.$ref || !member.allOf) return;
        const tag = member.allOf.flatMap((m) => Object.values(m.properties ?? {})).find((p) => p.enum?.length === 1);
        let hoisted = name + pascal(tag ? tag.enum[0] : `Variant${i + 1}`);
        while (schemas[hoisted]) hoisted += '_';
        schemas[hoisted] = member;
        schema[k][i] = { $ref: `#/components/schemas/${hoisted}` };
      });
    }
  }
  return spec;
}

const work = mkdtempSync(join(tmpdir(), 'harness-sdk-'));
try {
  // 1. The OpenAPI document.
  let specText;
  if (fromSnapshot) {
    specText = readFileSync(specFile, 'utf8');
  } else {
    specText = currentSpec();
    const have = existsSync(specFile) ? readFileSync(specFile, 'utf8').replaceAll('\r\n', '\n') : null;
    if (check) {
      if (have !== specText) {
        console.error('sdk/openapi.json is out of date: run `node sdk/generate.mjs` and commit it.');
        process.exitCode = 1;
      } else {
        console.log('sdk/openapi.json is up to date.');
      }
    } else {
      writeFileSync(specFile, specText);
    }
  }
  if (!check) {
    const specPath = join(work, 'openapi.json');
    writeFileSync(specPath, specText);

    // 2. TypeScript types.
    if (!args.includes('--skip-typescript')) {
      const ts = join(sdk, 'typescript');
      const bin = join(ts, 'node_modules', 'openapi-typescript', 'bin', 'cli.js');
      if (!existsSync(bin)) throw new Error('run `npm ci` in sdk/typescript first');
      run(process.execPath, [bin, specPath, '-o', join(ts, 'src', 'schema.ts'), '--root-types', '--root-types-no-schema-prefix', '--default-non-nullable=false', '--alphabetize'], { cwd: ts });
    }

    // 3. Python package.
    if (!args.includes('--skip-python')) {
      const [cmd, ...prefix] = uv();
      const out = join(work, 'python');
      const pySpec = join(work, 'openapi.python.json');
      writeFileSync(pySpec, JSON.stringify(pythonSpec(JSON.parse(specText)), null, 2));
      // A warning means a schema or an endpoint was left out of the client: fail instead.
      run(cmd, [...prefix, PYTHON_GENERATOR, 'generate', '--fail-on-warning', '--path', pySpec, '--meta', 'none', '--output-path', out, '--config', join(sdk, 'python', 'generator.yaml')]);
      run(cmd, [...prefix, RUFF, 'check', '--fix-only', '--quiet', '--select', 'I,F401,UP', '--target-version', 'py310', out]);
      run(cmd, [...prefix, RUFF, 'format', '--quiet', '--line-length', '100', out]);
      writeFileSync(join(out, 'py.typed'), ''); // PEP 561: the package ships its types
      const target = join(sdk, 'python', 'src', 'harness_studio_api');
      rmSync(target, { recursive: true, force: true });
      cpSync(out, target, { recursive: true });
    }
    console.log('SDK generated.');
  }
} finally {
  rmSync(work, { recursive: true, force: true });
}
