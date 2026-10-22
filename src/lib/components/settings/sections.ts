import { Cpu, HardDrive, Network, Palette, ShieldCheck, Download, Lock, Code, Stethoscope, Route, Activity, Wrench, Workflow } from '@lucide/svelte';
import type { TKey } from '#lib/i18n.svelte.js';

export type SectionId = 'runtime' | 'storage' | 'network' | 'security' | 'gateway' | 'tools' | 'developer' | 'otel' | 'automations' | 'appearance' | 'updates' | 'privacy' | 'diagnostics';

/** Settings sections, in sidebar order (the command palette lists them too). */
const ROOT_SECTIONS: { level?: 'advanced'; id: SectionId; label: TKey; desc: TKey; keywords: TKey; icon: typeof Cpu }[] = [
  { id: 'runtime', label: 'settings.tab_runtime', desc: 'settings.desc_runtime', keywords: 'cmd.kw_runtime', icon: Cpu },
  { id: 'storage', label: 'settings.tab_storage', desc: 'settings.desc_storage', keywords: 'cmd.kw_storage', icon: HardDrive },
  { level: 'advanced', id: 'network', label: 'settings.tab_network', desc: 'settings.desc_network', keywords: 'cmd.kw_network', icon: Network },
  { level: 'advanced', id: 'security', label: 'settings.tab_security', desc: 'settings.desc_security', keywords: 'cmd.kw_security', icon: Lock },
  { level: 'advanced', id: 'gateway', label: 'settings.tab_gateway', desc: 'settings.desc_gateway', keywords: 'cmd.kw_gateway', icon: Route },
  { id: 'tools', label: 'settings.tab_tools', desc: 'settings.desc_tools', keywords: 'cmd.kw_tools', icon: Wrench },
  { level: 'advanced', id: 'developer', label: 'settings.tab_developer', desc: 'settings.desc_developer', keywords: 'cmd.kw_developer', icon: Code },
  { level: 'advanced', id: 'otel', label: 'settings.tab_otel', desc: 'settings.desc_otel', keywords: 'cmd.kw_otel', icon: Activity },
  { id: 'automations', label: 'settings.tab_automations', desc: 'settings.desc_automations', keywords: 'cmd.kw_automations', icon: Workflow },
  { id: 'appearance', label: 'settings.tab_appearance', desc: 'settings.desc_appearance', keywords: 'cmd.kw_appearance', icon: Palette },
  { id: 'updates', label: 'settings.tab_updates', desc: 'settings.desc_updates', keywords: 'cmd.kw_updates', icon: Download },
  { id: 'privacy', label: 'settings.tab_privacy', desc: 'settings.desc_privacy', keywords: 'cmd.kw_privacy', icon: ShieldCheck },
  { level: 'advanced', id: 'diagnostics', label: 'settings.tab_diagnostics', desc: 'settings.desc_diagnostics', keywords: 'cmd.kw_diagnostics', icon: Stethoscope },
];

const SUBPAGES: Partial<Record<SectionId, { level?: 'advanced'; id: string; label: TKey; desc?: TKey }[]>> = {
  runtime: [{ id: 'installed', label: 'menus.installed_runtime' }, { level: 'advanced', id: 'registry', label: 'menus.registry', desc: 'settings.desc_registry' }, { level: 'advanced', id: 'ollama', label: 'menus.ollama', desc: 'settings.desc_ollama' }],
  storage: [{ id: 'directories', label: 'menus.directories' }, { id: 'hf', label: 'menus.hf', desc: 'settings.desc_hf' }],
  network: [{ id: 'listen', label: 'menus.listen' }, { id: 'access', label: 'menus.access' }],
  security: [{ id: 'keys', label: 'menus.keys' }, { id: 'permissions', label: 'menus.security_permissions' }],
  gateway: [{ id: 'endpoint', label: 'menus.endpoint' }, { id: 'routing', label: 'menus.routing' }, { id: 'responses', label: 'menus.responses' }],
  tools: [{ level: 'advanced', id: 'builtin', label: 'menus.builtin_tools' }, { id: 'installed', label: 'menus.installed_tools', desc: 'settings.desc_installed_tools' }, { id: 'connectors', label: 'menus.connectors', desc: 'settings.desc_connectors' }],
  developer: [{ id: 'api', label: 'menus.control_api' }, { id: 'cli', label: 'menus.cli' }],
  diagnostics: [{ id: 'health', label: 'menus.health' }, { id: 'reports', label: 'menus.reports' }],
};
export const SETTINGS_SECTIONS = ROOT_SECTIONS.flatMap(root =>
  SUBPAGES[root.id]?.map(page => ({ ...root, level: page.level ?? root.level, id: `${root.id}-${page.id}`, label: page.label, desc: page.desc ?? root.desc, root: root.id })) ?? [{ ...root, level: root.level, root: root.id }]
);
export const SETTINGS_GROUPS = ROOT_SECTIONS.map(root => ({
  label: SUBPAGES[root.id] ? ({ network: 'menus.network', security: 'menus.security', gateway: 'menus.gateway', developer: 'menus.developer' } as Partial<Record<SectionId, TKey>>)[root.id] ?? root.label : undefined,
  level: root.level,
  pages: SETTINGS_SECTIONS.filter(page => page.root === root.id),
}));
/** Legacy palette and cross-screen requests land on the page containing their fields. */
export function settingsPage(id: string): string {
  const aliases: Record<string, string> = {
    runtime: 'runtime-installed', storage: 'storage-directories', network: 'network-listen',
    security: 'security-keys', gateway: 'gateway-endpoint', tools: 'tools-builtin',
    developer: 'developer-api', diagnostics: 'diagnostics-health',
  };
  return aliases[id] ?? id;
}
