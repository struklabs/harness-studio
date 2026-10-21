# Control API SDKs

Clients of the Harness Studio [Control API](../docs/guide/en/09-cli-api-mcp.md), generated from its OpenAPI document:

| Package | Folder | Built on |
| --- | --- | --- |
| `@struklabs/harness-studio-sdk` (npm) | [`typescript/`](typescript/README.md) | [openapi-typescript](https://openapi-ts.dev) types + [openapi-fetch](https://openapi-ts.dev/openapi-fetch/) |
| `harness-studio-sdk` (PyPI) | [`python/`](python/README.md) | [openapi-python-client](https://github.com/openapi-generators/openapi-python-client) (httpx, attrs) |

Publication checked on October 20, 2026: the [npm registry](https://registry.npmjs.org/@struklabs%2fharness-studio-sdk/latest) reports TypeScript version **0.1.0**. The [PyPI project](https://pypi.org/project/harness-studio-sdk/) returns 404: Python is **source-only**. Install the published TypeScript release with `npm install @struklabs/harness-studio-sdk`; install Python with `pip install ./sdk/python` (Node.js and uv are required for generation). New API operations in this checkout require building from source until a corresponding SDK release is published.

## Coverage

Both source clients derive HTTP paths, request/response types, and operations from the same committed OpenAPI snapshot. TypeScript uses openapi-fetch for every generated path; Python creates sync/async endpoint modules and generation fails on warnings rather than silently dropping endpoints. Run `node sdk/check-coverage.mjs` after generating both clients to compare every documented method/path with generated outputs. Browser bootstrap/session, settings, service lifecycle, logs, encrypted backup/restore, and update endpoints are included. Cookies/CSRF are a browser concern; automation normally uses scoped bearer tokens. The published npm 0.1.0 client predates the W5 browser operations; source coverage does not imply registry publication.

SSE endpoints have generated HTTP signatures but streaming uses the hand-written SSE helpers. `/mcp` is an authenticated JSON-RPC transport, not an OpenAPI REST operation; use an MCP client. No SDK endpoint coverage is claimed for native Tauri IPC commands. Server setup: [Linux server guide](../docs/guide/en/10-server-mode.md).

## Versioning and the frozen v1 contract

The Control API is versioned in its path (`/api/harness/v1`), every response carries `X-Harness-API-Version`, and `GET /api/harness/v1/capabilities` (the gateway has `GET /v1/harness/capabilities`) lists the supported versions, module manifest versions and feature flags. `sdk/contract/openapi.v1.json` is the frozen v1 contract: `node sdk/check-breaking.mjs` fails (in the SDK CI job) when `openapi.json` removes or retypes anything in it; additions (also new response enum values and event types) are fine. A breaking change needs a new API major version. The module manifest has its own schema version, with the schema in [`module-manifest.v1.schema.json`](module-manifest.v1.schema.json). Stability policy and deprecation window: [API Stability and Versioning](../docs/guide/en/12-api-stability.md).

## What is generated

Only the OpenAPI document is committed; the client code generated from it is not (it is gitignored):

| File | Source | In git |
| --- | --- | --- |
| `openapi.json` | `cargo run -p harness-api --example dump_openapi` (the same document as `GET /api/harness/v1/openapi.json`) | yes |
| `typescript/src/schema.ts` | openapi-typescript 7.13.0 | no |
| `python/src/harness_studio_api/` | openapi-python-client 0.29.1, formatted with ruff 0.16.9 | no |

Everything else (connecting, finding the running instance, errors, server-sent events, tests, examples) is written by hand and committed.

- After a Control API change: `node sdk/generate.mjs` refreshes `openapi.json` from the Rust code and generates both clients. Commit `openapi.json`. The Rust test `sdk_openapi_snapshot_is_current` and `node sdk/generate.mjs --check` fail while it is out of date.
- The packages generate their code from the committed `openapi.json` by themselves (`node sdk/generate.mjs --from-snapshot`, no cargo): `npm run build` / `npm test` in `typescript/`; building or installing the Python package (a hatch build hook) and its pytest session.

The generator needs `npm ci` in `sdk/typescript`, and [uv](https://docs.astral.sh/uv/) (or `UV=<path to uv>`) for Python: the Python generator and ruff run pinned through `uv tool run`, nothing is installed.

openapi-python-client rejects two valid OpenAPI 3.1 forms the API uses: a default value on an object-typed field, and inline `allOf` variants of a `oneOf` (serde's tagged enums). `generate.mjs` rewrites only its own copy of the document: the defaults are dropped (the server applies them) and the variants become named schemas such as `AddRuntimeRequestGithub`. The wire format is the same. The generator runs with `--fail-on-warning`, so a schema it cannot read fails the generation instead of silently dropping endpoints.

## Tests

The tests run against a real `harness serve` on a throwaway data directory in the temp folder, on a free loopback port, with no gateway:

```bash
cargo build -p harness-cli
node sdk/test-server.mjs -- npm test --prefix sdk/typescript
```

For Python, from `sdk/python`: `uv sync --group test`, then `node ../test-server.mjs -- uv run --group test pytest`. `HARNESS_BIN` picks another `harness` binary. Without the test server, the tests that need one are skipped.

## Publishing

[`.github/workflows/sdk-publish.yml`](../.github/workflows/sdk-publish.yml) publishes `@struklabs/harness-studio-sdk` to npm and `harness-studio-sdk` to PyPI. It runs only by hand from `main` (Actions > Publish SDKs, choosing npm, PyPI or both) or for a pushed `sdk-v<version>` tag, which must match the versions in `typescript/package.json` and `python/pyproject.toml`; branch pushes and pull requests never start it. Each job first checks that `openapi.json` is current and runs its package's tests against a real `harness serve`.

- npm publishes with provenance. The first publish uses the `NPM_TOKEN` secret of the `npm` environment; after a trusted publisher is set up for this workflow on npmjs.com, npm publishes through OIDC and the secret can be removed.
- PyPI uses trusted publishing only, from the `pypi` environment; no token is stored.
