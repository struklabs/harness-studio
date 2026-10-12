// Verifies every locale namespace has identical key sets in `tr` and `en`.
// Loads the .ts files directly via Node's built-in TypeScript type stripping (Node >= 22.18 / 23.6).
import { readdirSync } from 'node:fs';
import { pathToFileURL, fileURLToPath } from 'node:url';
import path from 'node:path';

const dir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', 'src', 'lib', 'locales');
const files = readdirSync(dir).filter((f) => f.endsWith('.ts') && f !== 'index.ts').sort();

let problems = 0;
const seen = new Map();

for (const file of files) {
  const mod = await import(pathToFileURL(path.join(dir, file)).href);
  const { tr, en } = mod.default ?? {};
  if (!tr || !en) {
    console.error(`${file}: default export must be { tr, en }`);
    problems++;
    continue;
  }
  for (const k of Object.keys(tr)) {
    if (!(k in en)) { console.error(`${file}: "${k}" is in tr but missing in en`); problems++; }
    if (seen.has(k)) { console.error(`${file}: "${k}" is also defined in ${seen.get(k)}`); problems++; }
    else seen.set(k, file);
  }
  for (const k of Object.keys(en)) {
    if (!(k in tr)) { console.error(`${file}: "${k}" is in en but missing in tr`); problems++; }
  }
}

if (problems > 0) {
  console.error(`\nLocale check failed with ${problems} problem(s).`);
  process.exit(1);
}
console.log(`Locale check passed: ${seen.size} keys across ${files.length} files.`);
