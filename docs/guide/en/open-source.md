# Open Source

Türkçe: [../tr/README.md](../tr/README.md)

**What you do here:** learn how Harness Studio is licensed and built, how to contribute, what is free, who maintains what, and what the Registry is and is not.

Harness Studio is an open-source project by [struklabs](https://github.com/struklabs). The application, its engines and the SDKs are developed in the open in one repository, and everything you need to run local models stays free.

## License and What Is Included

- **The core app is MIT.** The desktop app (the `src/` interface and the `src-tauri/` shell), the Rust crates in `crates/` (core services, gateway, Control API, MCP server, CLI, ONNX sidecar) and the documentation are released under the [MIT License](../../../LICENSE). You may use, modify and redistribute them, including commercially, under its terms.
- **The SDKs are MIT.** The TypeScript package `@struklabs/harness-studio-sdk` (npm) and the Python package `harness-studio-sdk` (PyPI) are generated from the Control API description in `sdk/openapi.json` and live in the same repository under `sdk/`. They are published by hand from `main` after their tests pass against a real `harness serve`; no release is built from a pull request.
- **Third-party software keeps its own license.** Engines such as llama.cpp, models and Registry entries have their own terms; see [Third-Party Notices](../../en/THIRD_PARTY.md). A model you download is not covered by the MIT License of the app.
- **No account is needed.** Nothing in the app or the SDKs requires signing in to a struklabs service, and the app sends no telemetry. See [Security and Privacy](../../en/SECURITY_MODEL.md).

## One Repository, Real Module Boundaries

The project is a single monorepo on purpose: one change can touch the interface, the Rust services and the docs together, and one CI run checks them all. The modules inside it are separated by crate dependencies that CI enforces:

| Crate | Role | May depend on |
|---|---|---|
| `harness-onnx` | ONNX Runtime sidecar | no other crate |
| `harness-core` | Services, storage, engines | `harness-onnx` |
| `harness-gateway` | OpenAI-compatible gateway | `harness-core` |
| `harness-mcp` | MCP server | `harness-core` |
| `harness-api` | Control API | `harness-core`, `harness-mcp` |
| `harness-cli` | The `harness` command | `harness-api`, `harness-core`, `harness-gateway`, `harness-mcp` |
| `harness-studio` (`src-tauri/`) | Desktop shell | `harness-api`, `harness-core`, `harness-gateway` |

Only the desktop shell may use the UI toolkit (Tauri), so the gateway, the Control API, the MCP server and the CLI never depend on the interface. Run the check yourself with `npm run check:crates`; it fails with the offending edge, for example `harness-gateway -> harness-studio`. A new crate has to be added to `scripts/check-crate-deps.mjs`, which makes its place in the layers a visible decision in review. The versions and compatibility rules of what crosses these boundaries are in [Versioned Contracts](../../en/CONTRACTS.md).

## Contributing

Contributions from anyone are welcome: bug reports, ideas, documentation, translations and code. [CONTRIBUTING.md](../../../CONTRIBUTING.md) has the development setup, the checks to run, the commit style and the pull request process.

- **Start with an issue or a discussion** for anything larger than a small fix, so the approach is agreed before you spend time on it.
- **Fork, branch from `main`, open a pull request.** Nobody, maintainers included, pushes to `main` directly. A maintainer review and passing CI are required.
- **Your contribution is MIT.** By submitting it you confirm you may do so and license it under the same terms. There is no Contributor License Agreement; a `Signed-off-by` line is welcome but optional.
- **Interface text needs both languages.** Every visible string goes through the locale files with Turkish and English keys.
- **Keep module boundaries.** A pull request that adds a crate dependency the table above does not allow fails the crate check.
- **Changelog lines go in the pull request description**, not into `docs/CHANGELOG.md`; maintainers consolidate them at release time.

## Free Core and Paid Services

The boundary is simple. Everything that runs on your computer is the MIT core: downloading and running models, the Playground, the gateway, the tools you install, the Control API, the CLI, the MCP server and the SDKs. Local features are not removed, limited or locked behind a payment or an account.

Anything paid would be a separate, optional service operated outside this repository, such as hosted or managed offerings or commercial support, and would never be required by the core. Today the project offers no paid feature; for sponsorship, partnership or commercial support questions see [SUPPORT.md](../../../SUPPORT.md). If that boundary ever changes, the change will be announced in the changelog and this page.

## Upstream First, Forks Last

Harness Studio builds on other open-source projects: llama.cpp, whisper.cpp, stable-diffusion.cpp, ONNX Runtime and others. When something is wrong or missing in one of them:

1. **Contribute upstream.** A fix or feature in the upstream project helps everyone and costs this project no maintenance. Report or fix it there and link the upstream issue in yours.
2. **Write an adapter second.** If upstream cannot or will not take it, an adapter in Harness Studio that translates between the engine and the backend contract (see [Versioned Contracts](../../en/CONTRACTS.md)) keeps the engine itself unmodified.
3. **Fork last.** The app can install a runtime from a GitHub fork you name, but the project avoids maintaining forks of its own. A fork you bring is your responsibility; runtimes that are not official builds need your explicit approval in the app.

## Updates and Support Responsibility

- **Application updates.** The maintainers publish releases on [GitHub Releases](https://github.com/struklabs/harness-studio/releases); you check for a new version and install it under **Settings > Updates**, and the app verifies the update signature first. Release notes list what changed.
- **Engines and models.** The app installs engines and models you choose and verifies downloads (SHA-256 for engines) and warns about unverified third-party runtimes. Keeping an engine current is your choice in **Runtimes**; the maintainers keep the integration working with new upstream releases on a best-effort basis.
- **Support is community support.** Harness Studio is maintained by volunteers; replies in [Issues](https://github.com/struklabs/harness-studio/issues) and [Discussions](https://github.com/struklabs/harness-studio/discussions) may take a few days. Security problems go to the private channel in [SECURITY.md](../../../SECURITY.md).
- **Third-party packages are the publisher's.** For a tool, a module backend or a runtime from someone else, the publisher is responsible for it and for its updates; the app shows permissions before you enable anything.

## The Registry Holds Official Content Only

The Registry is the official catalog of tools, MCP servers and skills, kept in [struklabs/harness-registry](https://github.com/struklabs/harness-registry) and read by the app from one pinned revision, so the list and the files it points to always match.

- **Official only.** Every entry is published by StrukLabs, has an id that starts with `struklabs.`, and is reviewed in that repository. The app rejects an index that contains an entry from another publisher.
- **No community publishing.** There is no upload or submission service and no marketplace. Nobody can publish into the Registry by uploading a package.
- **No package signing.** Entries are not signed by their authors. Integrity comes from the pinned revision of the official repository, which holds the list and the files together, and from the checks the app runs on every entry (schema, publisher, permissions). That is not a promise that a package is safe: read the permissions before you enable one.
- **Everything else is discovery, not the Registry.** Skills and MCP servers from public repositories appear in a separate **GitHub** view of Discover. They are not endorsed, and installing a skill needs a pinned commit and your confirmation; see [GitHub Discovery](github-discovery.md).
- **Want something in the Registry?** Open an issue or a discussion in the main repository, or contribute the tool upstream to its own project. The maintainers decide whether to add an official entry.
