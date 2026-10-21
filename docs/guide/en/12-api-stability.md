# API Stability and Versioning

**What you do here:** decide whether you can build on the Control API, the gateway, the SDKs, or a module manifest without being surprised by an update, and find out how long a deprecated feature keeps working.

This chapter is for people who write code against Harness Studio: scripts, the TypeScript and Python SDKs, apps that talk to the gateway, and module developers. If you only use the app, you can skip it.

## What is versioned

| Surface | Version | How you see it |
| --- | --- | --- |
| Control API | Major version **1**, in the path: `/api/harness/v1` | Every response carries `X-Harness-API-Version: 1`. `GET /api/harness/v1/capabilities` (no token) lists the versions and features. |
| Gateway (OpenAI, Anthropic, and Ollama compatible endpoints plus `/v1/harness/*`) | The same major version 1 | Every response carries `X-Harness-API-Version: 1`. `GET /v1/harness/capabilities` (no token) answers like the Control API. |
| SDKs (`@struklabs/harness-studio-sdk`, `harness-studio-sdk`) | Generated from the Control API contract; they have their own package version | The package version follows semantic versioning; a new SDK major version only follows a new API major version. |
| Module manifest | `manifest_version` inside the manifest, **1** today | The app checks it before it downloads or writes anything for a module. The schema is [`sdk/module-manifest.v1.schema.json`](../../../sdk/module-manifest.v1.schema.json). |

`capabilities` answers with `api_version`, `min_api_version` and `max_api_version` (the majors this build serves), `app_version`, `module_manifest_versions` (the manifest versions it installs), and a `features` map of flags such as `training_backend`. Check a feature flag instead of the app version when you depend on an optional feature. A flag that is missing or `false` means: do not rely on it.

The gateway's model-facing endpoints (`/v1/chat/completions` and the others) follow the compatibility of the OpenAI and Anthropic APIs they imitate; the Harness additions (`/v1/harness/*`, `X-Harness-*` headers) follow the policy below.

## The v1 contract is frozen

[`sdk/openapi.json`](../../../sdk/openapi.json) describes the Control API. The version 1 contract is also kept as a frozen copy, `sdk/contract/openapi.v1.json`, and a check in CI (`node sdk/check-breaking.mjs`) compares the current description with it on every pull request that touches the API.

**Allowed inside v1 (not breaking):**

- A new endpoint, method, or optional query parameter.
- A new optional request field.
- A new field in a response, a new error code, or a new value for an open-ended string.
- A new scope that nothing existing needs.
- A new optional feature flag in `capabilities`.
- Bug fixes that make the API behave the way its description already says.

Write clients that ignore response fields they do not know.

**Breaking (not allowed inside v1; the CI check fails):**

- Removing or renaming a path, a method, or an `operationId` (the SDK method name).
- Removing a parameter or a response field, changing a type, or making a response field optional or nullable.
- Adding a required parameter, request field, or request body, narrowing the accepted values of a request field, or making a field required.
- Removing a documented success response.

A breaking change needs a new major version: a new base path (`/api/harness/v2`), a new `x-harness-api-version` in the description, and a new frozen file next to the old one (`node sdk/check-breaking.mjs --freeze`). The frozen v1 file is never rewritten.

## Deprecation policy

1. A feature that will go away is marked `deprecated: true` in the OpenAPI description (the SDKs carry the marker into their documentation) and is listed in the release notes. Marking something deprecated is not a breaking change.
2. A deprecated feature keeps working **for at least 6 months and at least 2 minor releases** after the release that announced it, whichever is longer.
3. It is removed only in a new API major version. While a new major version is in preview, the previous major version keeps being served; when a major version is retired, it is announced at least 12 months ahead and `capabilities.min_api_version` tells clients which majors a given build still serves.
4. Security fixes can shorten this window. If one does, the release notes say what changed and why.

Harness Studio is pre-1.0 software. Until the app reaches 1.0, the **application** can change in minor releases; the **contract** described here (the `/v1` paths, the manifest version, the frozen description) is the part that does not break inside v1.

## Module manifests

Every module manifest carries `manifest_version`. The rules:

- A build installs manifests whose version is in the range it lists in `capabilities.module_manifest_versions`. A manifest outside the range is refused with **This manifest version is unsupported. Update the app.** before anything is downloaded or written.
- Inside one `manifest_version`, fields are not removed or retyped; new fields are added as optional ones only after the version that introduced them is announced.
- A new `manifest_version` is introduced together with the app that reads it. The previous manifest version is still installed for the deprecation window above, so a module written for version 1 keeps installing after version 2 appears.
- `min_app_version` in the manifest is checked as well, together with the platform list; an older app refuses a module that needs a newer one.

A module developer can validate a manifest against the schema file, read the supported versions from `capabilities`, and compare `app_version` with the module's `min_app_version` before offering an install.

## For contributors

- After a Control API change, run `node sdk/generate.mjs`; commit the refreshed `sdk/openapi.json`. Then run `node sdk/check-breaking.mjs`. If it reports a breaking change, make the change additive, or prepare a new major version together with the maintainers.
- To bump the supported manifest versions, change `MANIFEST_VERSION_MIN` and `MANIFEST_VERSION_MAX` in `crates/harness-core/src/modules/mod.rs` and add a new schema file; a Rust test checks the schema file against the manifest type.
- The scheduled dogfood job (`.github/workflows/dogfood.yml`) uses the CLI, the Control API, and the registry the way a user does, and checks the version header and `capabilities` on every run.
