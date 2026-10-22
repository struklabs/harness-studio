# Contributing to Harness Studio

Thank you for your interest in Harness Studio, an open-source project by [struklabs](https://github.com/struklabs). Bug reports, ideas, documentation fixes, translations, and code are all welcome.

[Türkçe bölüme geç](#türkçe)

## Ways to contribute

- **Report a bug or request a feature** through [GitHub Issues](https://github.com/struklabs/harness-studio/issues/new/choose). Please search first, and include your OS, GPU, backend (CUDA, ROCm, Vulkan, Metal, CPU), and Harness Studio version.
- **Ask a question or share an idea** in [GitHub Discussions](https://github.com/struklabs/harness-studio/discussions).
- **Report a security problem** privately. Do not open a public issue; see [SECURITY.md](SECURITY.md).
- **Improve the English docs.** English is the source of truth; Turkish translations are synced in one batch at release time, not per PR. Agents follow [AGENTS.md](AGENTS.md).
- **Send code.** For anything larger than a small fix, open an issue or a discussion first so we can agree on the approach before you invest time.
- **Prefer upstream over forks.** If the problem is in llama.cpp, whisper.cpp or another engine, fix it there first; an adapter in this repository comes second and a fork last.

By participating you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

How the project is licensed, what stays free, why upstream contributions are preferred over forks, and what the Registry accepts are in the [Open Source guide](docs/guide/en/open-source.md). In short: contributions are MIT, there is no CLA, and the Registry holds official content only (no community publishing).

## Development setup

Requirements:

- Node.js 22 or newer
- Rust (stable toolchain, with `rustfmt` and `clippy`)
- The [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your operating system (on Linux: WebKitGTK, libdbus, and related packages; see `.github/workflows/ci.yml` for the exact list)

```sh
git clone https://github.com/struklabs/harness-studio.git
cd harness-studio
npm ci
npm run tauri dev
```

The interface depends on Tauri commands, so running the frontend alone in a browser is not supported. Always use `npm run tauri dev`.

This is a single monorepo and its crates have enforced layers: for example the gateway must not depend on the desktop shell, and only the shell uses Tauri. `npm run check:crates` (also run in CI) lists any dependency that breaks the layers; a new crate must be added to `scripts/check-crate-deps.mjs`.

Repository layout:

| Path | Contents |
|---|---|
| `src/` | SvelteKit (Svelte 5) frontend; UI strings in `src/lib/locales/` |
| `src-tauri/` | Tauri shell (desktop integration, commands) |
| `crates/harness-core` | Core services: servers, downloads, runtimes, tools, storage |
| `crates/harness-gateway`, `harness-api`, `harness-mcp`, `harness-cli`, `harness-onnx` | Gateway, Control API, MCP server, CLI, ONNX sidecar |
| `docs/` | User-facing documentation ([features](docs/en/FEATURES.md), [security and privacy](docs/en/SECURITY_MODEL.md), guides, [versioned contracts](docs/en/CONTRACTS.md)) |
| `bench/compare/` | Benchmark suite against other local-AI tools |
| `scripts/` | Frontend checks and UI test scripts |

## Build

```sh
npm run build                    # frontend only
npm run tauri build              # installer for your platform
cargo build --release -p harness-cli   # the `harness` command
```

## Checks before opening a pull request

CI runs the same checks. Please run them locally first:

```sh
npm run check                                   # svelte-check / TypeScript, must report 0 errors
npm run check:locales                           # Turkish and English keys must match
npm run check:crates                            # crates may only depend on each other as the layers allow
npm run build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Some end-to-end tests are marked `#[ignore]` because they start a real `llama-server`:

```sh
HS_LLAMA_SERVER=/path/to/llama-server HS_TEST_MODEL=/path/to/model.gguf \
  cargo test --workspace -- --ignored --test-threads=1
```

Tests use real components rather than mocks wherever possible. Add or update a test for every behavior change. The `scripts/test-*.mjs` files that drive the user interface launch the application; run them one at a time, and do not run them with a glob.

Public documentation covers product usage and information. Internal designs, decisions, specifications, diagnostics, and measurement reports belong in the private `hs-internal` repository.

## Interface and translations

- The UI uses shadcn-svelte and Tailwind v4. Follow existing components for palette, spacing, and interaction patterns.
- Every user-visible string goes through `t('key')`; do not hard-code text. Add each new key to `src/lib/locales/` for **both** Turkish and English. `npm run check:locales` verifies parity.
- Use Title Case for navigation, headings, and buttons in English.

## Commit style

We use [Conventional Commits](https://www.conventionalcommits.org/): `type(scope): short description`, for example `feat(ui): add download queue filters` or `fix(core): handle missing mmproj`. Types: `feat`, `fix`, `refactor`, `perf`, `docs`, `test`, `chore`, `ci`. English or Turkish messages are both fine.

Put proposed user-facing changelog lines in the PR body; do not edit [docs/CHANGELOG.md](docs/CHANGELOG.md) in PRs. Maintainers consolidate entries at release time. Planned work lives in [docs/en/ROADMAP.md](docs/en/ROADMAP.md).

### Sign-off and licensing

Harness Studio is released under the [MIT License](LICENSE). By submitting a contribution you confirm that you have the right to submit it and agree that it is licensed under the same terms. No Contributor License Agreement (CLA) is required. A [Developer Certificate of Origin](https://developercertificate.org/) sign-off (`git commit -s`) is welcome but optional.

### Modules, adapters, and engines

New engines, tool adapters, and modules are contributed upstream as pull requests to this repository, not as long-lived forks, so they are built and tested on every platform and get the security review described in [docs/en/SECURITY_MODEL.md](docs/en/SECURITY_MODEL.md). Open an issue or a discussion first. Official registry content (tools and skills) lives in the `struklabs/harness-registry` repository; send registry changes there. The module manifest is versioned (`sdk/module-manifest.v1.schema.json`). The wider picture, including SDK packages and community listings, is in [Open Source, Contributions, and Community Content](docs/guide/en/14-open-source-and-community.md).

## Pull request process

1. Fork the repository and create a branch from `main` (for example `fix/gateway-timeout`).
2. Make focused changes. Keep unrelated cleanups in separate pull requests.
3. Run the relevant checks above and update English documentation and both UI locales where relevant. Turkish documentation translations are batched at release time.
4. Open a pull request and fill in the template. Link the issue it addresses.
5. A maintainer reviews it. All changes go through pull requests: nobody, including maintainers, pushes directly to `main`. At least one maintainer approval and passing CI are required to merge.
6. Maintainers squash or merge once feedback is addressed. Please be patient; this is a small team.

---

## For maintainers

### Branch protection

Configure `main` with: pull request required, at least one approving review (code owners, see `.github/CODEOWNERS`), required status checks (`CI`), no force pushes, and no direct pushes.

### CI

`.github/workflows/ci.yml` runs only the jobs that match the changed paths: on pull requests on Linux, on pushes to `main` and weekly with the Rust job on Windows, Linux and macOS (type check, locale parity, frontend build and tests, `rustfmt`, `clippy`, tests, SDK, registry fixtures, `cargo audit`). The `ci-ok` job reports the overall result. The **Dogfood** workflow (`.github/workflows/dogfood.yml`) is not part of pull request CI: it runs weekly, on pushes to `release/**` branches, for release tags, and by hand. It starts a headless `harness serve` on a throwaway data directory with a tiny GGUF and checks the API version header, one chat, one tool call, a skill install and listing, and a registry tool install (about 10 minutes, `node sdk/test-server.mjs -- node scripts/dogfood.mjs` locally). It needs **no secrets**, uses read-only permissions, and pull requests from forks run without access to any secret.

### Releases

1. Bump the version in `package.json`, `src-tauri/tauri.conf.json`, and the `Cargo.toml` files, and consolidate changelog lines from merged PR bodies under the new version. Sync Turkish documentation translations in the same release batch.
2. Merge to `main`, then push a tag such as `v0.2.0`.
3. `.github/workflows/release.yml` builds the Windows NSIS installer, macOS DMGs, and Linux AppImage and `.deb`, and uploads them to a **draft** GitHub release, together with the updater signature and `latest.json`. Review the draft and publish it.

Repository secrets used by the release workflow (Settings > Secrets and variables > Actions):

| Secret | Purpose |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | Contents of the updater signing key (generated with `npm run tauri signer generate`). Without it, no updater artifacts or `latest.json` are produced. The public key lives in `plugins.updater.pubkey` in `src-tauri/tauri.conf.json`. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password of that key (empty if generated with `--ci`). |

The workflow also accepts optional Apple signing secrets (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_P8`) to sign and notarize the macOS DMGs; see `.github/workflows/release.yml` for details. Windows code signing is not configured; the installer is unsigned for now. Never commit signing keys (`*.key` is git-ignored).

---

<a id="türkçe"></a>

# Harness Studio'ya Katkıda Bulunma (Türkçe)

Harness Studio, [struklabs](https://github.com/struklabs) tarafından geliştirilen açık kaynaklı bir projedir. Hata bildirimleri, fikirler, belge düzeltmeleri, çeviriler ve kod katkıları memnuniyetle karşılanır.

## Nasıl katkıda bulunabilirsiniz

- **Hata bildirimi veya özellik isteği:** [GitHub Issues](https://github.com/struklabs/harness-studio/issues/new/choose). Önce arama yapın; işletim sistemi, GPU, backend (CUDA, ROCm, Vulkan, Metal, CPU) ve Harness Studio sürümünü yazın.
- **Soru ve fikirler:** [GitHub Discussions](https://github.com/struklabs/harness-studio/discussions).
- **Güvenlik sorunları:** herkese açık issue açmayın, özel olarak bildirin: [SECURITY.md](SECURITY.md).
- **Belge ve çeviri:** Türkçe ve İngilizce eşit önemdedir.
- **Kod:** küçük düzeltmeler dışında önce bir issue veya tartışma açın.

Katılımınızla [Davranış Kuralları](CODE_OF_CONDUCT.md)'nı kabul etmiş olursunuz.

## Geliştirme

Gereksinimler: Node.js 22+, Rust (stable; `rustfmt` ve `clippy` ile), işletim sisteminiz için [Tauri önkoşulları](https://v2.tauri.app/start/prerequisites/).

```sh
git clone https://github.com/struklabs/harness-studio.git
cd harness-studio
npm ci
npm run tauri dev
```

Arayüz Tauri komutlarına bağlıdır; ön yüzü tek başına tarayıcıda çalıştırmak desteklenmez.

## PR öncesi kontroller

```sh
npm run check
npm run check:locales
npm run build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Her davranış değişikliği için test ekleyin veya güncelleyin. Arayüzde kullanıcıya görünen her metin `t('anahtar')` ile alınır; yeni anahtarı `src/lib/locales/` altında **hem Türkçe hem İngilizce** ekleyin. Başlık, menü ve düğmelerde Türkçe için TDK, İngilizce için Chicago başlık yazımı kullanılır.

## Commit stili ve lisans

Conventional Commits: `tür(kapsam): kısa açıklama` (`feat`, `fix`, `refactor`, `perf`, `docs`, `test`, `chore`, `ci`). Türkçe veya İngilizce yazabilirsiniz. Kullanıcıya görünen değişiklikleri [docs/CHANGELOG.md](docs/CHANGELOG.md) içindeki `[Unreleased]` bölümüne ekleyin.

Katkılar [MIT Lisansı](LICENSE) ile lisanslanır. CLA gerekmez; DCO imzası (`git commit -s`) isteğe bağlıdır.

## Pull request süreci

1. Depoyu fork'layın, `main`'den bir dal açın.
2. Değişiklikleri odaklı tutun; ilgisiz temizlikleri ayrı PR'a bırakın.
3. Yukarıdaki kontrolleri çalıştırın; belge ve iki dili güncelleyin.
4. PR şablonunu doldurun ve ilgili issue'yu bağlayın.
5. Bakımcılar inceler. Tüm değişiklikler PR ile girer; bakımcılar dahil kimse `main`'e doğrudan push yapmaz. Birleştirmek için en az bir bakımcı onayı ve geçen CI gerekir.
