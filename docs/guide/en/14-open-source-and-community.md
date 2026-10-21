# Open Source, Contributions, and Community Content

**What you do here:** find out what is open source, how the SDK packages relate to the app, how to contribute (code, modules, adapters), how community content is meant to be shown, and what works without an account.

## The app is MIT-licensed

The Harness Studio application, the `harness` command-line program, the gateway, the Control API, the MCP server, and the SDK sources are released under the [MIT License](../../../LICENSE). You can use, change, and redistribute them under that license. Model files, engines, and tools you download have their own licenses; the Model Hub and the registry show them next to each item. Third-party notices are in [THIRD_PARTY.md](../../en/THIRD_PARTY.md).

## Everything works locally

Harness Studio needs no account. Downloading models and engines needs a network connection; chatting, the gateway, tools, skills, agents, and the SDKs run on your computer. The app sends no telemetry (see [Security and Privacy](../../en/SECURITY_MODEL.md)). Remote or cloud models, a Hugging Face token, and GitHub discovery are things you turn on yourself.

Optional online services may be offered in the future. If they appear, they will be opt-in additions: nothing that works locally today will start to require an account, and the local features will stay in the open-source app.

## SDK packages

The Control API has two generated clients:

| Package | Registry | Folder |
| --- | --- | --- |
| `@struklabs/harness-studio-sdk` | npm | [`sdk/typescript`](../../../sdk/typescript/README.md) |
| `harness-studio-sdk` | PyPI | [`sdk/python`](../../../sdk/python/README.md) |

They are generated from the committed API description (`sdk/openapi.json`), so they cover the same endpoints as the app. The Control API major version is **1**, and the v1 contract is frozen: CI compares every change with `sdk/contract/openapi.v1.json` and rejects removals and type changes (`node sdk/check-breaking.mjs`). Each package has its own semantic version. A new SDK major version only follows a new API major version, and `GET /api/harness/v1/capabilities` tells a client which API versions a running app serves. Details and the deprecation window: [API Stability and Versioning](12-api-stability.md).

Packages are published by a maintainer through `.github/workflows/sdk-publish.yml` (by hand or for an `sdk-v<version>` tag), never from pull requests. The npm package is published. Until a PyPI release is announced in the SDK README, install the Python client from the source folder (`pip install ./sdk/python`).

## Contributing

[CONTRIBUTING.md](../../../CONTRIBUTING.md) is the full guide: how to build, which checks to run, commit style, and the pull request process. In short:

- Contributions are pull requests against `struklabs/harness-studio`; nobody pushes to `main` directly.
- No CLA is needed. Contributions are licensed under the MIT License. A `git commit -s` sign-off (Developer Certificate of Origin) is welcome but optional.
- Run `npm run check`, `npm run check:locales`, and the Rust checks first. Add a test for every behavior change, and EN and TR strings for every new interface text.
- Documentation is in English. Turkish translations are synced at release time.

### Modules, adapters, and engines belong upstream

If you build support for a new engine, tool adapter, or module, propose it in an issue or a discussion first and send it as a pull request, instead of maintaining a long-lived fork. That way it is built and tested on every platform, gets the permission and security review described in [Security and Privacy](../../en/SECURITY_MODEL.md), and ships with the next release. Module manifests have a versioned schema ([`sdk/module-manifest.v1.schema.json`](../../../sdk/module-manifest.v1.schema.json)); the app refuses a manifest whose version it does not support. Today only official module bundles are installed (see [Modules](../../en/MODULES.md)); there are no community-built packages.

Registry content (official tools and skills) lives in the separate `struklabs/harness-registry` repository; send registry changes there.

## Community modules and skills

Public skills and MCP repositories can already be reviewed from **Discover > GitHub** (see [Discover Public GitHub Content](github-discovery.md)); the app shows the repository's license and other metadata and never trusts it as verified.

Community content is meant to be listed apart from the official catalog:

- A listing shows its **author**, **repository**, and **license**, and carries a **Community** badge so it is never mistaken for an official item.
- The owner of a repository can claim its listing and manage it (description, versions, removal).
- Community content runs with the permissions you grant after reviewing it, the same as any other tool or skill.

This listing and claiming flow is the planned design and is not part of the app yet; this page will be updated when it ships.
