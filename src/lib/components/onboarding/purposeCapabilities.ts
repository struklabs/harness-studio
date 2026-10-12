import type { AgentModel } from '../connections/agentModels';

export type CodeToolEvidence = 'native' | 'generic' | 'verified' | 'limited' | 'unknown';

/** Reuse the backend verdict; a template alone is never proof of tool calling. */
export function codeToolEvidence(model: AgentModel | undefined): CodeToolEvidence {
  if (model?.tool_level) return model.tool_level;
  if (model?.tools === 'verified') return 'verified';
  if (model?.tools === 'failed' || model?.tools === 'no_template') return 'limited';
  return 'unknown';
}

export function codeToolRank(evidence: CodeToolEvidence): number {
  return { native: 0, generic: 1, verified: 1, unknown: 2, limited: 3 }[evidence];
}

/** Only built-ins registered in harness_core::tools::builtin::register. No registry downloads. */
export const PURPOSE_TOOLS = {
  chat: ['web_search'],
  code: ['read_file', 'file_glob_search', 'grep_search', 'write_file', 'edit_file', 'exec_shell_command'],
  rag: ['read_file', 'file_glob_search', 'grep_search', 'fetch_url'],
  asr: [],
};
