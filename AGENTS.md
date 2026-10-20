# Agent Rules

- Harness Studio is a Tauri + Svelte 5 desktop app: `src/` is the frontend,
  `crates/` contains Rust services, and `src-tauri/` is the desktop shell.
- English docs under `docs/en/` and root `README.md` are the single source of truth.
  Read only English documentation, except `docs/tr/ROADMAP.md`;
  the English user guide is in `docs/guide/en/`.
- Never load `docs/tr/`, `docs/guide/tr/`, `README.tr.md`, or other Turkish
  translations (including Turkish sections in bilingual files) unless the task
  is translation or the task updates `docs/tr/ROADMAP.md`. Compare filenames and
  Git metadata for structural audits.
- Keep current EN/TR documentation pairs for public GitHub readers. Sync Turkish
  translations in one batch at release time, never per PR, except
  `docs/tr/ROADMAP.md`: it is the one Turkish doc kept in sync per change.
- Never commit test reports, regression notes, one-off scripts, screenshots,
  build output, or AI attribution. Preserve tests and their fixtures, reusable
  tooling, release notes, licenses/notices, SDKs, and workflows during cleanup.
- PRs must not edit `docs/CHANGELOG.md`; put proposed changelog lines in the PR
  body. Maintainers consolidate them at release time.
- UI strings need both EN and TR locale keys. Use Title Case: TR TDK, EN Chicago.
- Run `npm run check` and `npm run check:locales`. Run only specific Node test
  files added or touched; never use `node --test scripts/test-*.mjs`.
- For docs-only changes, run available package locale/doc-link checks; skip Cargo.
- For Rust changes, run `cargo fmt`, targeted crate Clippy with
  `--all-targets -- -D warnings`, relevant unit tests, `cargo test -p harness-cli`,
  and `cargo test -p harness-api --test api`. If the Control API schema changes,
  regenerate `sdk/openapi.json` with
  `cargo run -q -p harness-api --example dump_openapi`.
- Use short English commit messages without AI attribution or `Co-Authored-By`.
  Follow the requested commit count; do not push unless explicitly asked.
