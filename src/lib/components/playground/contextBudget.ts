/** Cheap preflight estimate; tokenizer counts replace this when available. */
export function estimatePromptBudget(history: unknown[], tools: unknown[], draft: string, ragTokens = 0) {
  const count = (value: string) => Math.ceil(Array.from(value).length / 3);
  const toolTokens = tools.length ? count(JSON.stringify(tools)) : 0;
  const inputTokens = count(JSON.stringify(history)) + count(draft) + history.length * 4 + toolTokens + Math.max(0, ragTokens);
  return { toolTokens, inputTokens };
}

/** The tool-definition warning applies: tools are offered (`toolTokens` > 0) and the input reaches 75 % of the context. */
export function hasContextPressure(inputTokens: number, context: number | null, toolTokens = 1): boolean {
  return toolTokens > 0 && !!context && context > 0 && inputTokens >= context * 0.75;
}

export function lengthStop(
  finish: string | undefined,
  stats: { source: string; promptN?: number; predictedN?: number } | undefined,
  context: number | null | undefined,
  reasoning: string | undefined,
  content: string,
) {
  const used = (stats?.promptN ?? 0) + (stats?.predictedN ?? 0);
  const contextFull = finish === 'length' && !!context && context > 0
    && !!stats && (stats.source === 'timings' || stats.source === 'usage')
    && used >= context - Math.max(16, Math.ceil(context * 0.01));
  const visible = content.replace(/<think>[\s\S]*?(?:<\/think>|$)/gi, '').trim();
  return { contextFull, thinkingOnly: finish === 'length' && (!!reasoning?.trim() || /<think>/i.test(content)) && !visible };
}
