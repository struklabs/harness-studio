# Control API SDKs

Clients of the Harness Studio [Control API](../docs/guide/en/09-cli-api-mcp.md), generated from its OpenAPI document:

| Package | Folder | Built on |
| --- | --- | --- |
| `@struklabs/harness-studio-sdk` (npm) | [`typescript/`](typescript/README.md) | [openapi-typescript](https://openapi-ts.dev) types + [openapi-fetch](https://openapi-ts.dev/openapi-fetch/) |
| `harness-studio-sdk` (PyPI) | [`python/`](python/README.md) | [openapi-python-client](https://github.com/openapi-generators/openapi-python-client) (httpx, attrs) |

Neither is published yet: build them from this folder (Node.js is needed for both, uv for Python).

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
