#!/usr/bin/env node
// Compare generated clients with every documented HTTP operation, not just a sample.
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const spec = JSON.parse(readFileSync(join(root, 'openapi.json'), 'utf8'));
const ts = readFileSync(join(root, 'typescript/src/schema.ts'), 'utf8');
const pyRoot = join(root, 'python/src/harness_studio_api/api');
const python = readdirSync(pyRoot, { recursive: true })
  .filter(name => name.endsWith('.py'))
  .map(name => readFileSync(join(pyRoot, name), 'utf8'));
const methods = ['get', 'put', 'post', 'delete', 'patch', 'head', 'options'];
let total = 0;
const missing = [];
for (const [path, item] of Object.entries(spec.paths)) {
  for (const method of methods) {
    const operation = item[method];
    if (!operation) continue;
    total++;
    // Both the typed path and its operation binding must exist.
    if (!ts.includes(`"${path}": {`) || !ts.includes(`${method}: operations["${operation.operationId}"]`)) {
      missing.push(`TypeScript: ${method.toUpperCase()} ${path}`);
    }
    // Python formats path parameters before issuing the request; the template stays in _get_kwargs.
    if (!python.some(text => text.includes(`"url": "${path}"`) && text.includes(`"method": "${method}"`) && text.includes('def sync_detailed(') && text.includes('async def asyncio_detailed('))) {
      missing.push(`Python: ${method.toUpperCase()} ${path}`);
    }
  }
}
if (missing.length) {
  console.error(missing.join('\n'));
  process.exitCode = 1;
} else {
  console.log(`Both generated source clients cover all ${total} HTTP operations across ${Object.keys(spec.paths).length} paths. SSE uses streaming helpers; MCP JSON-RPC is outside OpenAPI.`);
}
