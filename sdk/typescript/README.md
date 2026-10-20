# @struklabs/harness-studio-sdk (TypeScript)

Typed client of the [Harness Studio](https://github.com/struklabs/harness-studio) Control API. Every path, parameter, request body and answer comes from the API's OpenAPI document; requests go through [openapi-fetch](https://openapi-ts.dev/openapi-fetch/).

Published on npm as **0.1.0** (registry checked October 20, 2026):

```bash
npm install @struklabs/harness-studio-sdk
```

For new API operations in this checkout, including W5 browser administration, build from source until the next SDK release:

```bash
cd sdk/typescript
npm ci
npm run build    # generates src/schema.ts from sdk/openapi.json, then compiles
```

## Use

```ts
import { connect } from '@struklabs/harness-studio-sdk/node';
import { unwrap } from '@struklabs/harness-studio-sdk';

// Finds the running app or `harness serve` through <data dir>/instance.json
// (HARNESS_DATA_DIR, else the app's folder) and uses the session token.
const client = connect();

const status = await unwrap(client.GET('/api/harness/v1/status'));
console.log(status.version, status.servers.running);

const library = await unwrap(client.GET('/api/harness/v1/models'));
await unwrap(client.POST('/api/harness/v1/load', { body: { alias: library.definitions[0].alias } }));
```

- `createHarnessClient({ baseUrl, token })` connects to a given address (another computer, a named token). It works in browsers too; `@struklabs/harness-studio-sdk/node` adds `connect()` and `discover()`, which read files.
- `client.GET / POST / PUT / PATCH / DELETE` return `{ data, error, response }`. `unwrap(...)` returns `data` or throws a `HarnessApiError` with `status`, `code` (`api.not_found`, `api.insufficient_scope`...), `messageKey` and `params`.
- `streamEvents(client, { types, signal })` yields the event bus (`GET /events`) as `{ type, event }`; `readServerSentEvents(response)` reads any event stream, for example `/servers/{id}/logs/stream`.
- Schema types are exported by name: `import type { ModelDefinition, ServerView } from '@struklabs/harness-studio-sdk'`.

Examples: [`examples/status.mjs`](examples/status.mjs), [`examples/load-model.mjs`](examples/load-model.mjs).

## Development

`src/schema.ts` is generated from `sdk/openapi.json` and not committed: `npm run build` (or `npm run generate`) writes it. Tests: see [`sdk/README.md`](../README.md#tests). `npm run typecheck` checks the sources, tests and examples.
