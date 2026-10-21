#!/usr/bin/env node
// Fails when sdk/openapi.json breaks the frozen v1 contract.
//
//   sdk/contract/openapi.v<N>.json   the frozen contract of API major version N (committed once,
//                                    never edited by hand: additions to the API do not touch it)
//   sdk/openapi.json                 the current document (`info.x-harness-api-version` = N)
//
// A change is breaking when a client written against the frozen contract could stop working:
// a removed path, method, operationId, parameter or response property, a new required
// parameter, request field or body, a narrowed request type or enum, a response type that
// became wider or optional, a removed response enum value or event type, a removed 2xx
// response. Additions (new paths, new optional fields, new response properties, new response
// enum values, new event types: clients ignore values they do not know) are allowed.
//
// A breaking change is only allowed together with a new API major version (`/v2`, a new
// `x-harness-api-version`): the frozen v1 file stays, `--freeze` writes the v2 one. See
// docs/guide/en/12-api-stability.md.
//
// Usage: node sdk/check-breaking.mjs [--freeze] [--self-test]
//   (none)       compare sdk/openapi.json with the frozen contract of its major version.
//   --freeze     write sdk/contract/openapi.v<N>.json from sdk/openapi.json (only when absent).
//   --self-test  run the built-in cases of the comparison and exit.
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const METHODS = ['get', 'put', 'post', 'delete', 'patch', 'head', 'options'];

function resolveRef(doc, schema, seen = new Set()) {
  let s = schema;
  while (s && typeof s === 'object' && typeof s.$ref === 'string') {
    if (seen.has(s.$ref)) return {};
    seen.add(s.$ref);
    s = s.$ref
      .replace(/^#\//, '')
      .split('/')
      .reduce((o, k) => o?.[k.replaceAll('~1', '/').replaceAll('~0', '~')], doc);
  }
  return s ?? {};
}

/** The schema with `$ref` resolved and `allOf` members merged into one. */
function flatten(doc, schema) {
  const s = resolveRef(doc, schema);
  if (!Array.isArray(s.allOf)) return s;
  const merged = { ...s };
  delete merged.allOf;
  for (const member of s.allOf) {
    const m = flatten(doc, member);
    merged.properties = { ...merged.properties, ...m.properties };
    merged.required = [...new Set([...(merged.required ?? []), ...(m.required ?? [])])];
    if (m.type && !merged.type) merged.type = m.type;
    if (m.enum && !merged.enum) merged.enum = m.enum;
    if (m.items && !merged.items) merged.items = m.items;
    if (m.oneOf && !merged.oneOf) merged.oneOf = m.oneOf;
    if (m.anyOf && !merged.anyOf) merged.anyOf = m.anyOf;
  }
  return merged;
}

/**
 * The value that names a variant of a tagged union: its own single enum value (`"Stopped"`),
 * or the single value of one of its properties (`{"type": "download", ...}`). `undefined`
 * when the variant has none.
 */
function tagOf(doc, schema) {
  const s = flatten(doc, schema);
  const single = (p) => (p.const !== undefined ? p.const : p.enum?.length === 1 ? p.enum[0] : undefined);
  if (single(s) !== undefined) return JSON.stringify(single(s));
  for (const [name, p] of Object.entries(s.properties ?? {})) {
    const v = single(flatten(doc, p));
    if (v !== undefined) return `${name}=${JSON.stringify(v)}`;
  }
  return undefined;
}

const typesOf = (s) => {
  const t = s.type ?? (s.properties ? 'object' : s.items ? 'array' : undefined);
  return t === undefined ? null : new Set(Array.isArray(t) ? t : [t]);
};

/**
 * Problems that arise when values of `sub` (producer) must be accepted by `sup` (acceptor).
 * Requests: sub = old schema, sup = new schema (the new server must accept what clients sent).
 * Responses: sub = new schema, sup = old schema (clients must understand what the server sends).
 */
function subset(ctx, sub, sup, path, seen = new Set()) {
  const refKey = `${sub?.$ref ?? ''}>${sup?.$ref ?? ''}`;
  if ((sub?.$ref || sup?.$ref) && seen.has(refKey)) return []; // recursive schema: already compared
  const nextSeen = new Set(seen);
  if (sub?.$ref || sup?.$ref) nextSeen.add(refKey);

  const a = flatten(ctx.subDoc, sub);
  const b = flatten(ctx.supDoc, sup);
  const withDocs = (subVariant, supVariant, p = path) =>
    subset(ctx, subVariant, supVariant, p, nextSeen);

  if (b === true || Object.keys(b).length === 0) return [];

  const aVariants = a.oneOf ?? a.anyOf;
  const bVariants = b.oneOf ?? b.anyOf;
  if (aVariants) {
    if (ctx.sub === 'new' && bVariants) {
      // A response union (an event type, a status): a variant with a new tag is an addition
      // clients skip; every old tag must still be sent, and a kept tag must stay readable.
      const oldTags = new Set(bVariants.map((v) => tagOf(ctx.supDoc, v)));
      const newTags = new Set(aVariants.map((v) => tagOf(ctx.subDoc, v)));
      const problems = [];
      for (const tag of oldTags) {
        if (tag !== undefined && !newTags.has(tag)) problems.push(`${path}: variant ${tag} removed`);
      }
      aVariants.forEach((v, i) => {
        const tag = tagOf(ctx.subDoc, v);
        if (tag === undefined || oldTags.has(tag)) problems.push(...withDocs(v, sup, `${path}|${i}`));
      });
      return problems;
    }
    return aVariants.flatMap((v, i) => withDocs(v, sup, `${path}|${i}`));
  }
  if (bVariants) {
    const attempts = bVariants.map((v) => withDocs(sub, v));
    return attempts.some((p) => p.length === 0)
      ? []
      : [`${path}: ${ctx.describe(a)} is not covered by any variant`];
  }

  const problems = [];
  const aTypes = typesOf(a);
  const bTypes = typesOf(b);
  if (bTypes) {
    const ok =
      aTypes && [...aTypes].every((t) => bTypes.has(t) || (t === 'integer' && bTypes.has('number')));
    if (!ok) {
      problems.push(
        `${path}: type ${aTypes ? [...aTypes].join('|') : 'any'} vs ${[...bTypes].join('|')}`,
      );
      return problems;
    }
  }
  const same = (v, w) => JSON.stringify(v) === JSON.stringify(w);
  if (b.enum && ctx.sub === 'new') {
    // Response: new values are allowed (clients ignore them); every old value must stay.
    const values = a.enum ?? (a.const !== undefined ? [a.const] : null);
    if (values && b.enum.some((w) => !values.some((v) => same(v, w)))) {
      problems.push(`${path}: enum value removed`);
    }
  } else if (b.enum) {
    const values = a.enum ?? (a.const !== undefined ? [a.const] : null);
    if (!values || values.some((v) => !b.enum.some((w) => same(v, w)))) {
      problems.push(`${path}: enum values differ`);
    }
  }
  if ((aTypes?.has('object') || a.properties) && (bTypes?.has('object') || b.properties)) {
    const aProps = a.properties ?? {};
    const bProps = b.properties ?? {};
    const aReq = new Set(a.required ?? []);
    for (const name of b.required ?? []) {
      if (!aReq.has(name)) {
        problems.push(`${path}.${name}: ${ctx.requiredMessage}`);
      }
    }
    // A property the old schema had must still exist in the new one.
    const [oldProps, newProps] = ctx.sub === 'old' ? [aProps, bProps] : [bProps, aProps];
    for (const name of Object.keys(oldProps)) {
      if (!(name in newProps)) problems.push(`${path}.${name}: property removed`);
    }
    for (const [name, schema] of Object.entries(aProps)) {
      if (name in bProps) problems.push(...withDocs(schema, bProps[name], `${path}.${name}`));
    }
    if (b.additionalProperties === false) {
      for (const name of Object.keys(aProps)) {
        if (!(name in bProps)) problems.push(`${path}.${name}: not allowed by additionalProperties`);
      }
    }
  }
  if (a.items && b.items) problems.push(...withDocs(a.items, b.items, `${path}[]`));
  return problems;
}

function compareSchema(oldDoc, newDoc, oldSchema, newSchema, mode, path) {
  if (mode === 'request') {
    return subset(
      {
        subDoc: oldDoc,
        supDoc: newDoc,
        sub: 'old',
        requiredMessage: 'now required',
        describe: () => 'schema',
      },
      oldSchema ?? {},
      newSchema ?? {},
      path,
    );
  }
  return subset(
    {
      subDoc: newDoc,
      supDoc: oldDoc,
      sub: 'new',
      requiredMessage: 'no longer always present',
      describe: () => 'schema',
    },
    newSchema ?? {},
    oldSchema ?? {},
    path,
  );
}

function paramMap(doc, item, op) {
  const map = new Map();
  for (const p of [...(item.parameters ?? []), ...(op.parameters ?? [])]) {
    const r = resolveRef(doc, p);
    map.set(`${r.in}:${r.name}`, r);
  }
  return map;
}

/** Breaking changes from `oldDoc` (the frozen contract) to `newDoc`. */
export function breakingChanges(oldDoc, newDoc) {
  const problems = [];
  for (const [path, oldItem] of Object.entries(oldDoc.paths ?? {})) {
    const newItem = newDoc.paths?.[path];
    for (const method of METHODS) {
      const oldOp = oldItem[method];
      if (!oldOp) continue;
      const where = `${method.toUpperCase()} ${path}`;
      const newOp = newItem?.[method];
      if (!newOp) {
        problems.push(`${where}: operation removed`);
        continue;
      }
      if (oldOp.operationId !== newOp.operationId) {
        problems.push(`${where}: operationId ${oldOp.operationId} -> ${newOp.operationId}`);
      }
      // Parameters.
      const oldParams = paramMap(oldDoc, oldItem, oldOp);
      const newParams = paramMap(newDoc, newItem, newOp);
      for (const [key, p] of oldParams) {
        const q = newParams.get(key);
        if (!q) {
          problems.push(`${where}: parameter ${key} removed`);
          continue;
        }
        if (q.required && !p.required) problems.push(`${where}: parameter ${key} is now required`);
        for (const msg of compareSchema(oldDoc, newDoc, p.schema, q.schema, 'request', `param ${key}`)) {
          problems.push(`${where}: ${msg}`);
        }
      }
      for (const [key, q] of newParams) {
        if (!oldParams.has(key) && q.required) {
          problems.push(`${where}: new required parameter ${key}`);
        }
      }
      // Request body.
      const oldBody = oldOp.requestBody ? resolveRef(oldDoc, oldOp.requestBody) : null;
      const newBody = newOp.requestBody ? resolveRef(newDoc, newOp.requestBody) : null;
      if (oldBody && !newBody) problems.push(`${where}: request body removed`);
      if (newBody?.required && !oldBody?.required) problems.push(`${where}: request body is now required`);
      if (oldBody && newBody) {
        for (const [type, media] of Object.entries(oldBody.content ?? {})) {
          const other = newBody.content?.[type];
          if (!other) {
            problems.push(`${where}: request content type ${type} removed`);
            continue;
          }
          for (const msg of compareSchema(oldDoc, newDoc, media.schema, other.schema, 'request', 'request')) {
            problems.push(`${where}: ${msg}`);
          }
        }
      }
      // Responses: every documented success response stays, and clients can read it.
      for (const [code, oldRes] of Object.entries(oldOp.responses ?? {})) {
        const newRes = newOp.responses?.[code];
        if (!newRes) {
          if (/^2/.test(code)) problems.push(`${where}: response ${code} removed`);
          continue;
        }
        const o = resolveRef(oldDoc, oldRes);
        const n = resolveRef(newDoc, newRes);
        for (const [type, media] of Object.entries(o.content ?? {})) {
          const other = n.content?.[type];
          if (!other) {
            problems.push(`${where}: response ${code} content type ${type} removed`);
            continue;
          }
          for (const msg of compareSchema(oldDoc, newDoc, media.schema, other.schema, 'response', `response ${code}`)) {
            problems.push(`${where}: ${msg}`);
          }
        }
      }
    }
  }
  return problems;
}

function selfTest() {
  const op = (extra = {}) => ({
    operationId: 'op',
    responses: { 200: { description: '', content: { 'application/json': { schema: { $ref: '#/components/schemas/Out' } } } } },
    ...extra,
  });
  const event = (type, extra = {}) => ({
    type: 'object',
    required: ['type', ...Object.keys(extra)],
    properties: { type: { type: 'string', enum: [type] }, ...extra },
  });
  const out0 = {
    type: 'object',
    required: ['a'],
    properties: {
      a: { type: 'string' },
      b: { type: 'integer' },
      kind: { type: 'string', enum: ['x', 'y'] },
      status: { oneOf: [{ type: 'string', enum: ['Stopped'] }, { type: 'string', enum: ['Running'] }] },
      event: { oneOf: [event('started', { n: { type: 'integer' } }), event('stopped')] },
    },
  };
  const doc = (paths, out = out0) => ({
    paths,
    components: { schemas: { Out: out, In: { type: 'object', required: ['x'], properties: { x: { type: 'string' }, y: { type: 'string' }, mode: { type: 'string', enum: ['p', 'q'] } } } } },
  });
  const body = (required = true) => ({
    required,
    content: { 'application/json': { schema: { $ref: '#/components/schemas/In' } } },
  });
  const base = doc({ '/a': { post: op({ requestBody: body(), parameters: [{ name: 'q', in: 'query', schema: { type: 'string' } }] }) } });
  const clone = () => JSON.parse(JSON.stringify(base));
  const cases = [];
  const expectOk = (name, d) => cases.push([name, d, false]);
  const expectBreaking = (name, d) => cases.push([name, d, true]);

  expectOk('unchanged', clone());
  let d = clone();
  d.paths['/b'] = { get: op() };
  expectOk('new path', d);
  d = clone();
  d.components.schemas.Out.properties.c = { type: 'string' };
  expectOk('new response property', d);
  d = clone();
  d.components.schemas.In.properties.z = { type: 'string' };
  expectOk('new optional request property', d);
  d = clone();
  d.paths['/a'].post.parameters.push({ name: 'n', in: 'query', schema: { type: 'integer' } });
  expectOk('new optional parameter', d);
  d = clone();
  d.components.schemas.Out.properties.kind.enum.push('z');
  expectOk('new response enum value', d);
  d = clone();
  d.components.schemas.Out.properties.status.oneOf.push({ type: 'string', enum: ['Paused'] });
  expectOk('new value of a response union of single values', d);
  d = clone();
  d.components.schemas.Out.properties.event.oneOf.push(event('restarted', { at: { type: 'string' } }));
  expectOk('new event type', d);
  d = clone();
  d.components.schemas.In.properties.mode.enum.push('r');
  expectOk('new request enum value', d);

  d = clone();
  delete d.paths['/a'];
  expectBreaking('path removed', d);
  d = clone();
  d.paths['/a'].post.operationId = 'renamed';
  expectBreaking('operationId renamed', d);
  d = clone();
  delete d.components.schemas.Out.properties.b;
  expectBreaking('response property removed', d);
  d = clone();
  d.components.schemas.Out.properties.a.type = 'integer';
  expectBreaking('response property retyped', d);
  d = clone();
  d.components.schemas.Out.required = [];
  expectBreaking('response property became optional', d);
  d = clone();
  d.components.schemas.In.required = ['x', 'y'];
  expectBreaking('request property became required', d);
  d = clone();
  d.components.schemas.In.properties.x.type = 'integer';
  expectBreaking('request property retyped', d);
  d = clone();
  d.paths['/a'].post.parameters.push({ name: 'n', in: 'query', required: true, schema: { type: 'integer' } });
  expectBreaking('new required parameter', d);
  d = clone();
  d.paths['/a'].post.parameters = [];
  expectBreaking('parameter removed', d);
  d = clone();
  delete d.paths['/a'].post.responses['200'];
  d.paths['/a'].post.responses['201'] = { description: '' };
  expectBreaking('success response removed', d);
  d = clone();
  d.components.schemas.Out.properties.a = { type: ['string', 'null'] };
  expectBreaking('response became nullable', d);
  d = clone();
  d.components.schemas.In.properties.x = { type: 'string', enum: ['one'] };
  expectBreaking('request enum narrowed', d);
  d = clone();
  d.components.schemas.Out.properties.kind.enum = ['x'];
  expectBreaking('response enum value removed', d);
  d = clone();
  d.components.schemas.In.properties.mode.enum = ['p'];
  expectBreaking('request enum value removed', d);
  d = clone();
  d.components.schemas.Out.properties.status.oneOf.pop();
  expectBreaking('response union value removed', d);
  d = clone();
  d.components.schemas.Out.properties.event.oneOf.pop();
  expectBreaking('event type removed', d);
  d = clone();
  d.components.schemas.Out.properties.event.oneOf[0] = event('started');
  expectBreaking('field of a kept event type removed', d);
  d = clone();
  d.components.schemas.Out.properties.event.oneOf[1] = event('halted');
  expectBreaking('event type renamed', d);

  let failed = 0;
  for (const [name, candidate, shouldBreak] of cases) {
    const found = breakingChanges(base, candidate);
    const ok = shouldBreak ? found.length > 0 : found.length === 0;
    if (!ok) {
      failed++;
      console.error(`FAIL ${name}: expected ${shouldBreak ? 'breaking' : 'compatible'}, got ${JSON.stringify(found)}`);
    }
  }
  if (failed) process.exit(1);
  console.log(`check-breaking self-test: ${cases.length} cases ok`);
}

function main() {
  const args = process.argv.slice(2);
  if (args.includes('--self-test')) return selfTest();
  const sdk = dirname(fileURLToPath(import.meta.url));
  const dir = join(sdk, 'contract');
  const current = JSON.parse(readFileSync(join(sdk, 'openapi.json'), 'utf8'));
  const major = current.info?.['x-harness-api-version'];
  if (!Number.isInteger(major)) {
    console.error('sdk/openapi.json has no integer info.x-harness-api-version');
    process.exit(1);
  }
  const file = join(dir, `openapi.v${major}.json`);
  if (args.includes('--freeze')) {
    if (existsSync(file)) {
      console.error(`${file} already exists: a frozen contract is never rewritten`);
      process.exit(1);
    }
    mkdirSync(dir, { recursive: true });
    writeFileSync(file, readFileSync(join(sdk, 'openapi.json')));
    console.log(`froze API v${major} at ${file}`);
    return;
  }
  const frozen = existsSync(dir)
    ? readdirSync(dir)
        .map((f) => /^openapi\.v(\d+)\.json$/.exec(f))
        .filter(Boolean)
        .map((m) => Number(m[1]))
    : [];
  if (!existsSync(file)) {
    if (frozen.length && major > Math.max(...frozen)) {
      console.log(`API v${major} is a new major version (frozen: v${frozen.join(', v')}); run \`node sdk/check-breaking.mjs --freeze\` to freeze it.`);
      return;
    }
    console.error(`No frozen contract for API v${major} (${file}). Run \`node sdk/check-breaking.mjs --freeze\`.`);
    process.exit(1);
  }
  if (major < Math.max(...frozen)) {
    console.error(`sdk/openapi.json says API v${major} but v${Math.max(...frozen)} is already frozen`);
    process.exit(1);
  }
  const baseline = JSON.parse(readFileSync(file, 'utf8'));
  if (baseline.info?.['x-harness-api-version'] !== major) {
    console.error(`${file} is not a v${major} document`);
    process.exit(1);
  }
  const problems = breakingChanges(baseline, current);
  if (problems.length) {
    console.error(`sdk/openapi.json breaks the frozen API v${major} contract:`);
    for (const p of problems) console.error(`  - ${p}`);
    console.error(
      `\nA breaking change needs a new API major version (/v${major + 1}, x-harness-api-version ${major + 1}); ` +
        `otherwise make the change additive. Policy: docs/guide/en/12-api-stability.md`,
    );
    process.exit(1);
  }
  console.log(`API v${major}: no breaking changes against ${file}`);
}

main();
