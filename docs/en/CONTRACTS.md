# Versioned Contracts

[Open Source guide](../guide/en/open-source.md)

Harness Studio is one repository, but its parts talk to each other and to the outside through contracts. This page lists each contract, where it is defined, which version it carries today and what a change must do. Crate-level boundaries (which crate may depend on which) are enforced separately by `npm run check:crates`; see [Open Source](../guide/en/open-source.md#one-repository-real-module-boundaries).

## Overview

| Contract | Defined in | Version marker | Today |
|---|---|---|---|
| Backend (engine family) | `crates/harness-core/src/engine/` | Family id, compiled in with the app version | 15 families, see below |
| Module manifest | `crates/harness-core/src/modules/mod.rs`, `bundled.json` | `manifest_version` | 1 |
| Registry index | `crates/harness-core/src/tools/official_registry.rs`, [harness-registry](https://github.com/struklabs/harness-registry) | `schema_version` | 1 |
| Control API | `crates/harness-api/`, `sdk/openapi.json` | App version (`info.version`) | Same as the app |
| Local data files | `crates/harness-core/src/store.rs`, `domain::store::all_schemas()` | `schema_version` per file | Per file |

## Backend Contract

A backend is an engine family: llama.cpp, whisper.cpp, stable-diffusion.cpp, ONNX Runtime and the others the app can run. Backends are compiled into the app; there is no dynamic plugin loading, so a new backend is a contribution to this repository (see [upstream first](../guide/en/open-source.md#upstream-first-forks-last)).

What a family implements (`Engine`, and `ServerEngine` or `JobEngine`):

- **Identity.** A stable family id (`llama.cpp`, `whisper.cpp`, `stable-diffusion.cpp`, `onnxruntime`, `sherpa-onnx`, `mlx`, `mistral.rs`, `lemonade` and so on) stored with installed runtimes. Runtimes recorded before families existed read as `llama.cpp`. An id is never reused for something else.
- **Release source.** Where builds come from and how the right archive is picked for an OS, architecture and backend (`vulkan`, `cuda`, `rocm`, `metal`, `cpu`).
- **Capabilities.** What a build supports, read from the binary itself (flags from `--help`, version, devices) and cached by file identity, so the app never assumes a flag the binary does not have.
- **Parameters.** A parameter catalog (`params*.toml`) that defines every setting the app may pass, with types and protected flags.
- **Server engines** add the launch command line, the readiness endpoint and what "ready" means, the OpenAI-compatible base URL, the metrics source and a memory estimate. **Job engines** (llama-bench) add the job binary and progress parsing.

The wire side of a backend is OpenAI-compatible HTTP: the gateway talks to a server through the base URL the engine reports and does not depend on engine internals.

Versioning rules:

- Adding an optional capability or a trait method with a default does not need a new version.
- Changing the meaning of a family id, a stored runtime field or a parameter name is breaking: keep reading the old form (a migration) and note it in the release notes.
- A backend that cannot meet the contract on a platform must say so through its compatibility result instead of failing at launch.

## Module Manifest Contract

A module is an optional feature (for example speech, agents) that is installed, updated, repaired and removed without touching your models and data. Its manifest (`ModuleManifest`) is checked when it is read:

| Field | Meaning |
|---|---|
| `id`, `version` | Module id and its own version (`major.minor.patch`). |
| `manifest_version` | Version of this contract. The app accepts only manifest version `1`; anything else is reported as incompatible. |
| `name`, `description`, `functions`, `hardware`, `permissions` | Text keys for both languages (`en`, `tr`). |
| `platforms` | `windows-x86_64`, `linux-x86_64`, `macos-aarch64`. The current platform must be listed. |
| `components` | Downloadable parts with `id`, `version`, `sources`, and size estimates. |
| `shared_component_ids` | Components shared with other modules (a removal keeps them while another module uses them). |
| `min_app_version` | The lowest app version that can run the module. |
| `planned` | `true` for a module that is announced but not installable. |

Unknown fields are rejected, so a manifest written for a newer contract fails loudly instead of being half understood. Versioning rules:

- A new optional field is added together with a new `manifest_version`, and the app keeps accepting the previous version for at least one minor release.
- Raising `min_app_version` is how a module says it needs a newer app; the app shows it as incompatible and does not install it.
- A module's own `version` follows semantic versioning; a component version changes whenever its bytes change.

## Registry Index Contract

The Registry index lists the official tools, MCP servers and skills. It is read from one pinned revision of the official repository together with the files it points to. `schema_version` is `1`; any other value is rejected.

Each entry has an id (`struklabs.<type>.<name>`), a `type` (`tool`, `skill` or `mcp`), `publisher` (`StrukLabs`), `version`, `license`, `platforms` (`windows`, `macos`, `linux`), a `permissions` block (network, file read and write, shell), `tags`, `min_harness_version` and an `artifact` path that must match the id. The app rejects an index with a missing permissions block, a duplicate id, an entry from another publisher, a request for shell access, a network wildcard or a non-semantic version. JSON schemas for each artifact type are in the registry repository. The Registry has only official content; see [Open Source](../guide/en/open-source.md#the-registry-holds-official-content-only).

Versioning rules: a new optional entry field is added to the schema and the app in the same release; raising `schema_version` is a breaking change that needs an app release first, because an older app rejects the new index.

## Control API Contract

The Control API is described by `sdk/openapi.json`, generated from the Rust code (`cargo run -q -p harness-api --example dump_openapi`). CI fails when the file is out of date, and the TypeScript and Python SDKs are generated from it and tested against a real `harness serve`. The description carries the app version. Within a 0.x version, additive changes (new endpoints, new optional fields) are made freely; removing or renaming an endpoint or field, or changing its meaning, is noted in the release notes.

## Local Data File Contract

Settings, definitions, profiles, presets, download queue, benchmark history, measured model performance and other files in the configuration folder are versioned JSON. A file carries `schema_version`; every change of shape adds a migration step, and `domain::store::all_schemas()` lists them all. A file written by a newer app than the one running is opened read-only rather than rewritten, so going back to an older version never destroys data. Adding a file means adding its schema there.

## Changing a Contract

1. Decide whether the change is additive (optional, defaulted) or breaking.
2. For a breaking change: bump the version marker, add a migration or keep accepting the old version, and update this page in the same pull request.
3. Add or update a test that reads a fixture of the old version.
4. Put the change in the pull request's changelog lines so maintainers can include it in the release notes.
