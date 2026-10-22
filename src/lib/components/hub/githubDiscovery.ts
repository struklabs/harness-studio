import type { RegistryEntry } from '../tools/registryCatalog.ts';

export interface GithubRepo {
  full_name: string; description: string | null; stargazers_count: number; updated_at: string;
  topics: string[]; license: { spdx_id: string } | null;
}
export type RiskCategory = 'network' | 'shell' | 'eval' | 'credentials' | 'binary';
export interface RiskFinding { file: string; category: RiskCategory; pattern: string; line: number }
export type RiskLevel = 'none' | 'low' | 'medium' | 'high';
export interface SecuritySummary { level: RiskLevel; scripts_scanned: number; network: number; shell: number; eval: number; credentials: number; binary: number }
export interface SkillReview {
  path: string; name: string; errors: string[]; scripts: string[]; network: string[]; bytes: number;
  references: string[]; warnings: string[]; risks: RiskFinding[]; security: SecuritySummary;
  commit: string; scope: string; install_dir: string;
}
export type CheckStatus = 'pass' | 'warn' | 'fail';
export interface McpCheck { id: string; status: CheckStatus; detail: string }
export type McpKind = 'local_stdio' | 'remote_http' | 'unknown';
export interface McpEntryEvaluation { kind: McpKind; label: string | null; checks: McpCheck[] }
export interface McpEvaluation { checks: McpCheck[]; entries: McpEntryEvaluation[] }
export interface McpEvaluationView { source: 'manifest' | 'registry'; evaluation: McpEvaluation }
/** Registry and repository metadata checks the core computed (nothing here contacts a server). */
export function mcpEvaluations(metadata: unknown): McpEvaluationView[] {
  const out: McpEvaluationView[] = [];
  const data = metadata as { evaluation?: McpEvaluation; registry?: { evaluation?: McpEvaluation }[] } | null;
  if (data?.evaluation) out.push({ source: 'manifest', evaluation: data.evaluation });
  for (const item of data?.registry ?? []) if (item.evaluation) out.push({ source: 'registry', evaluation: item.evaluation });
  return out;
}
/** Flagged lines grouped by category, in a fixed order. */
export function riskGroups(risks: RiskFinding[]): { category: RiskCategory; findings: RiskFinding[] }[] {
  return (['credentials', 'eval', 'shell', 'network', 'binary'] as RiskCategory[])
    .map(category => ({ category, findings: risks.filter(r => r.category === category) }))
    .filter(g => g.findings.length);
}
/** A high-risk skill needs a second, explicit confirmation before it is copied. */
export const needsRiskAck = (security: SecuritySummary): boolean => security.level === 'high';
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
