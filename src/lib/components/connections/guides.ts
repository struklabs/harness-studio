// Step-by-step setup for tools whose config Harness does not write (the tool keeps it in its
// own UI or database). Steps come from each tool's official docs; see docs/en/INTEGRATIONS.md.
import { t } from '#lib/i18n.svelte.js';
import { hostOf, PLACEHOLDER_KEY, type SnippetContext } from './snippets';

export interface GuideField {
  label: string;
  value: string;
}

export interface Guide {
  steps: string[];
  fields: GuideField[];
  /** Shown as a warning above the steps (e.g. "localhost is unreachable"). */
  warn: string | null;
}

const GUIDES: Record<string, { steps: number; host?: boolean; warn?: boolean }> = {
  'claude-desktop': { steps: 5, host: true },
  cursor: { steps: 4, warn: true },
  cline: { steps: 4 },
  'roo-code': { steps: 3, warn: true },
  jetbrains: { steps: 4 },
  jan: { steps: 4 },
  'open-webui': { steps: 4, warn: true },
  'obsidian-copilot': { steps: 4, warn: true },
};

/** Guide for `id`, or null when the tool has none. Reads t() so call it from a $derived. */
export function buildGuide(id: string, c: SnippetContext): Guide | null {
  const g = GUIDES[id];
  if (!g) return null;
  const key = id.replace(/-/g, '_');
  const steps = Array.from({ length: g.steps }, (_, i) => t(`intg.guide_${key}_${i + 1}` as never));
  const fields: GuideField[] = [
    g.host
      ? { label: t('intg.field_gateway_url'), value: hostOf(c.baseUrl) }
      : { label: t('intg.field_base_url'), value: c.baseUrl },
    { label: t('intg.field_api_key'), value: c.apiKey ?? PLACEHOLDER_KEY },
    { label: t('intg.field_model'), value: c.modelId },
  ];
  if (id === 'cline' || id === 'roo-code' || id === 'jetbrains') {
    fields.push({ label: t('intg.field_context'), value: String(c.contextSize || 32768) });
  }
  return { steps, fields, warn: g.warn ? t(`intg.guide_${key}_warn` as never) : null };
}
