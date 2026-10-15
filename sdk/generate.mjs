#!/usr/bin/env node
// Regenerates the Control API clients from the Rust source of truth:
//
//   1. sdk/openapi.json                         <- `cargo run -p harness-api --example dump_openapi`
//   2. sdk/typescript/src/schema.ts             <- openapi-typescript (pinned in sdk/typescript)
//   3. sdk/python/src/harness_studio_api/       <- openapi-python-client (pinned below, run by uv)
//
// Usage: node sdk/generate.mjs [--check] [--skip-typescript] [--skip-python] [--spec <file>]
//   --check   regenerate into a temporary folder and fail when the committed files differ.
//   --spec    use this OpenAPI file instead of building it with cargo.
// Needs: cargo; `npm ci` in sdk/typescript; uv (`uv`/`uvx` on PATH or UV=<path to uv>).
import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const PYTHON_GENERATOR = 'openapi-python-client==0.29.1';
const RUFF = 'ruff==0.16.9';

const sdk = dirname(fileURLToPath(import.meta.url));
const repo = join(sdk, '..');
const args = process.argv.slice(2);
const check = args.includes('--check');
const specArg = args.includes('--spec') ? args[args.indexOf('--spec') + 1] : null;

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

/** Every file under `dir`, relative, with LF line endings (Windows checkouts may differ). */
function snapshot(dir) {
  const out = new Map();
  if (!existsSync(dir)) return out;
  const walk = (d) => {
    for (const name of readdirSync(d)) {
      const p = join(d, name);
      if (name === '__pycache__') continue;
      if (statSync(p).isDirectory()) walk(p);
      else out.set(relative(dir, p).replaceAll('\\', '/'), readFileSync(p, 'utf8').replaceAll('\r\n', '\n'));
    }
  };
  walk(dir);
  return out;
}

function same(label, file, want) {
  const have = existsSync(file) ? readFileSync(file, 'utf8').replaceAll('\r\n', '\n') : null;
  if (have !== want) console.error(`${label}: out of date`);
  return have === want;
}

function diff(label, want, have) {
  const changed = [];
  for (const [k, v] of want) if (have.get(k) !== v) changed.push(k);
  for (const k of have.keys()) if (!want.has(k)) changed.push(k);
  if (changed.length) console.error(`${label}: out of date (${changed.slice(0, 8).join(', ')}${changed.length > 8 ? ', ...' : ''})`);
  return changed.length === 0;
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
let ok = true;
try {
  // 1. The OpenAPI document.
  const spec = specArg
    ? readFileSync(specArg, 'utf8')
    : run('cargo', ['run', '-q', '-p', 'harness-api', '--example', 'dump_openapi'], { cwd: repo });
  // As printed (`to_pretty_json`): a JSON round trip in JavaScript would turn `0.0` into `0`.
  JSON.parse(spec);
  const specText = spec.replaceAll('\r\n', '\n').trimEnd() + '\n';
  const specFile = join(sdk, 'openapi.json');
  if (check) ok = same('sdk/openapi.json', specFile, specText) && ok;
  else writeFileSync(specFile, specText);
  const specPath = join(work, 'openapi.json');
  writeFileSync(specPath, specText);

  // 2. TypeScript types.
  if (!args.includes('--skip-typescript')) {
    const ts = join(sdk, 'typescript');
    const bin = join(ts, 'node_modules', 'openapi-typescript', 'bin', 'cli.js');
    if (!existsSync(bin)) throw new Error('run `npm ci` in sdk/typescript first');
    const out = join(work, 'schema.ts');
    run(process.execPath, [bin, specPath, '-o', out, '--root-types', '--root-types-no-schema-prefix', '--default-non-nullable=false', '--alphabetize'], { cwd: ts });
    const text = readFileSync(out, 'utf8').replaceAll('\r\n', '\n');
    const target = join(ts, 'src', 'schema.ts');
    if (check) ok = same('sdk/typescript/src/schema.ts', target, text) && ok;
    else writeFileSync(target, text);
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
    if (check) ok = diff('sdk/python/src/harness_studio_api', snapshot(out), snapshot(target)) && ok;
    else {
      rmSync(target, { recursive: true, force: true });
      cpSync(out, target, { recursive: true });
    }
  }
} finally {
  rmSync(work, { recursive: true, force: true });
}
if (!ok) {
  console.error('Run `node sdk/generate.mjs` and commit the result.');
  process.exit(1);
}
console.log(check ? 'SDK is up to date.' : 'SDK regenerated.');
