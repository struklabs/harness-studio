import type { RegistryEntry } from '../tools/registryCatalog.ts';

export interface GithubRepo {
  full_name: string; description: string | null; stargazers_count: number; updated_at: string;
  topics: string[]; license: { spdx_id: string } | null;
}
export interface SkillReview { path: string; name: string; errors: string[]; scripts: string[]; network: string[]; bytes: number }
export interface GithubReview {
  repo: string; sha: string; readme: string; release: { tag_name: string; name: string } | null;
  platforms: string[]; skills: SkillReview[]; mcp: unknown; manifests: Record<string, string>;
}
/** Copy configuration templates; keep provenance and requirements in the review. */
export function mcpConfigTemplates(metadata: unknown): { mcpServers: Record<string, unknown> } {
  const servers: Record<string, unknown> = {};
  function collect(value: unknown) {
    if (!value || typeof value !== 'object') return;
    const data = value as { config?: { configTemplate?: unknown }[]; registry?: unknown[] };
    for (const entry of data.config ?? []) {
      if (entry.configTemplate) servers[`github-server-${Object.keys(servers).length + 1}`] = entry.configTemplate;
    }
    for (const item of data.registry ?? []) collect(item);
  }
  collect(metadata);
  return { mcpServers: servers };
}
export function officialMatches(repo: string, entries: RegistryEntry[]): RegistryEntry[] {
  const normalize = (s: string) => {
    try {
      const url = new URL(s);
      if (url.protocol !== 'https:' || url.hostname !== 'github.com' || url.username || url.password || url.port) return '';
      const parts = url.pathname.split('/').filter(Boolean);
      return parts.length >= 2 ? `https://github.com/${parts[0]}/${parts[1].replace(/\.git$/, '')}`.toLowerCase() : '';
    } catch { return ''; }
  };
  const url = normalize(`https://github.com/${repo}`);
  return entries.filter(e => normalize(e.source) === url || normalize(e.homepage) === url);
}
