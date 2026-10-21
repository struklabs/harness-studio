// Workspace dependency check: the crates of this monorepo may only depend on each other the
// way the layers below allow. It reads `cargo metadata` and fails on any other edge, so a
// module boundary (the gateway does not know the desktop shell, the core knows no crate that
// sits on top of it) is enforced, not just drawn in a diagram.
//
//   node scripts/check-crate-deps.mjs
//
// Layers, lowest first. A crate may depend on the workspace crates listed for it (normal,
// build and dev dependencies alike) and on no other workspace crate:
//
//   harness-onnx      leaf: ONNX Runtime sidecar
//   harness-core      services, storage, engines; only depends on harness-onnx
//   harness-gateway   OpenAI-compatible gateway; core only
//   harness-mcp       MCP server; core only
//   harness-api       Control API; core and mcp
//   harness-cli       `harness` command; api, core, gateway, mcp
//   harness-studio    desktop shell (src-tauri); api, core, gateway
//
// Only the shell may use the desktop UI toolkit (tauri, wry, tao and their plugins).
// A new workspace crate has to be added to RULES, which makes its place in the layers an
// explicit decision in review.
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

/** Workspace crates each crate may depend on. */
export const RULES = {
  'harness-onnx': [],
  'harness-core': ['harness-onnx'],
  'harness-gateway': ['harness-core'],
  'harness-mcp': ['harness-core'],
  'harness-api': ['harness-core', 'harness-mcp'],
  'harness-cli': ['harness-api', 'harness-core', 'harness-gateway', 'harness-mcp'],
  'harness-studio': ['harness-api', 'harness-core', 'harness-gateway'],
};

/** The only crate that may depend on the desktop UI toolkit. */
export const UI_CRATE = 'harness-studio';
const UI_TOOLKIT = /^(tauri|wry|tao|muda|tray-icon)(-|$)/;

/**
 * Violations in a `cargo metadata --no-deps` document, as readable lines (empty = fine).
 * @param {{ packages: { name: string, dependencies: { name: string, kind: string | null }[] }[] }} metadata
 * @param {Record<string, string[]>} rules
 */
export function check(metadata, rules = RULES) {
  const problems = [];
  const members = new Map(metadata.packages.map((p) => [p.name, p]));
  for (const name of members.keys()) {
    if (!(name in rules)) problems.push(`${name}: workspace crate has no layer; add it to RULES in scripts/check-crate-deps.mjs`);
  }
  for (const name of Object.keys(rules)) {
    if (!members.has(name)) problems.push(`${name}: listed in RULES but not a workspace member`);
  }
  for (const [name, pkg] of members) {
    const allowed = new Set(rules[name] ?? []);
    for (const dep of pkg.dependencies) {
      const kind = dep.kind ?? 'normal';
      if (members.has(dep.name) && !allowed.has(dep.name)) {
        problems.push(`${name} -> ${dep.name} (${kind}): not allowed; ${name} may depend on ${allowed.size ? [...allowed].join(', ') : 'no workspace crate'}`);
      }
      if (name !== UI_CRATE && UI_TOOLKIT.test(dep.name)) {
        problems.push(`${name} -> ${dep.name} (${kind}): only ${UI_CRATE} may use the desktop UI toolkit`);
      }
    }
  }
  return problems;
}

function main() {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
  const out = execFileSync('cargo', ['metadata', '--format-version', '1', '--no-deps'], {
    cwd: root,
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  });
  const problems = check(JSON.parse(out));
  if (problems.length > 0) {
    console.error('Crate dependency check failed:');
    for (const p of problems) console.error(`  ${p}`);
    process.exit(1);
  }
  console.log(`Crate dependency check passed: ${Object.keys(RULES).length} crates.`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
