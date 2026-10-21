// Dev-only: synthetic module catalogs for checking the Modules page at scale.
// The page loads it only when `import.meta.env.DEV` is true and `?modules_fixture=500`
// (or `localStorage.hs_modules_fixture`) asks for it; production builds never import it.
import type { ModuleView, ModuleStatus } from '../modules.ts';

/** Small deterministic PRNG so a fixture is the same on every run. */
export function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export const FIXTURE_CATEGORIES = ['speech', 'vision', 'agents', 'training', 'automation', 'data', 'integrations', 'developer'];
const CAPABILITIES = ['transcribe', 'agent', 'train', 'automate', 'ocr', 'embed', 'speak', 'search'];
const PLATFORMS = ['windows-x86_64', 'linux-x86_64', 'macos-aarch64'];
const AUTHORS = ['Acme Labs', 'Open Voice Collective', 'tinkerer42', 'Northwind AI', 'Lumen Tools', 'Kestrel Systems'];
const LICENSES = ['MIT', 'Apache-2.0', 'GPL-3.0-only', 'MPL-2.0', 'BSD-3-Clause'];
const WORDS = ['Vox', 'Pixel', 'Atlas', 'Relay', 'Forge', 'Pulse', 'Prism', 'Echo', 'Beacon', 'Quill', 'Nimbus', 'Harbor', 'Cobalt', 'Sprout', 'Lattice', 'Ember'];
const NOUNS = ['Bridge', 'Studio', 'Toolkit', 'Pipeline', 'Scribe', 'Router', 'Vault', 'Lens', 'Runner', 'Sync'];
const STATUSES: ModuleStatus[] = ['not_installed', 'not_installed', 'not_installed', 'installed', 'installed', 'disabled', 'failed'];

export function generateModules(count: number, seed = 1): ModuleView[] {
  const rand = mulberry32(seed);
  const pick = <T,>(list: readonly T[]) => list[Math.floor(rand() * list.length)];
  return Array.from({ length: count }, (_, i) => {
    const community = rand() < 0.6;
    const category = pick(FIXTURE_CATEGORIES);
    const capabilities = [...new Set([pick(CAPABILITIES), pick(CAPABILITIES)])];
    const name = `${pick(WORDS)} ${pick(NOUNS)} ${i + 1}`;
    const status = pick(STATUSES);
    const installed = status === 'installed' || status === 'disabled';
    const version = `${1 + Math.floor(rand() * 3)}.${Math.floor(rand() * 10)}.${Math.floor(rand() * 10)}`;
    const day = String(1 + Math.floor(rand() * 28)).padStart(2, '0');
    const month = String(1 + Math.floor(rand() * 10)).padStart(2, '0');
    const platforms = rand() < 0.8 ? PLATFORMS : [pick(PLATFORMS)];
    const mb = 20 + Math.floor(rand() * 900);
    const id = `fixture-${String(i + 1).padStart(4, '0')}`;
    return {
      manifest: {
        id, version, manifest_version: 1,
        name: { en: name, tr: name },
        description: { en: `${name} adds ${capabilities.join(' and ')} support to Harness Studio.`, tr: `${name}, Harness Studio'ya ${capabilities.join(' ve ')} desteği ekler.` },
        functions: capabilities.map(c => ({ en: `Provides ${c}`, tr: `${c} sağlar` })),
        platforms, hardware: { en: 'CPU only', tr: 'Yalnızca işlemci' },
        components: [{ id: `${id}-core`, version, sources: [`https://example.com/${id}.zip`], download_bytes: mb * 1024 * 1024, disk_bytes: mb * 2 * 1024 * 1024 }],
        permissions: [{ en: 'Network access', tr: 'Ağ erişimi' }, { en: 'Reads files you choose', tr: 'Seçtiğiniz dosyaları okur' }].slice(0, 1 + Math.floor(rand() * 2)),
        shared_component_ids: [], min_app_version: '0.1.0', planned: false,
      },
      state: { status, installed_version: installed ? version : null, error: status === 'failed' ? 'modules.interrupted' : null, components: [] },
      disk_bytes: installed ? mb * 1024 * 1024 : 0, shared_disk_bytes: 0,
      update_available: installed && rand() < 0.2,
      data_choices: [],
      incompatibility: platforms.length === 1 && rand() < 0.5 ? 'modules.platform_incompatible' : null,
      catalog: {
          category, tags: [pick(WORDS).toLowerCase(), category, pick(CAPABILITIES)],
          author: community ? pick(AUTHORS) : undefined, license: community ? pick(LICENSES) : 'MIT',
          repository: community ? `https://github.com/example/${id}` : undefined, community,
          capabilities, requires: rand() < 0.15 ? ['speech'] : [],
          changelog: [{ version, notes: { en: 'Performance and stability fixes.', tr: 'Başarım ve kararlılık düzeltmeleri.' } }],
          updated_at: `2026-${month}-${day}`, featured: rand() < 0.05, popularity: Math.floor(rand() * 10000)
      },
      progress: null, components: {},
    } satisfies ModuleView;
  });
}

/** Count requested through `?modules_fixture=N` or localStorage, capped at 5000; 0 when off. */
export function fixtureCount(search: string, stored: string | null): number {
  const raw = new URLSearchParams(search).get('modules_fixture') ?? stored;
  const n = raw ? Number.parseInt(raw, 10) : 0;
  return Number.isFinite(n) && n > 0 ? Math.min(n, 5000) : 0;
}
