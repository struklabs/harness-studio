import policy from '../../../../shared/tool-loop-policy.json' with { type: 'json' };

/** Shared retention rationale and constants: shared/tool-loop-policy.json. */
export const MAX_TOOL_CALLS = policy.max_calls;
export function toolCallLimit(offered: number): number {
  return offered >= policy.menu_min_tools ? Math.min(MAX_TOOL_CALLS, Math.max(policy.menu_min_calls, Math.ceil(offered * policy.menu_ratio_numerator / policy.menu_ratio_denominator))) : MAX_TOOL_CALLS;
}
function canonical(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value)
      .sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0)
      .map(([k, v]) => [k, canonical(v)]));
  }
  return value;
}
export function guardToolCalls<T extends { name: string; arguments: string }>(calls: T[], offered: number, finishing = false): { calls: T[]; stopped: boolean } {
  const keys = new Set<string>();
  const limit = toolCallLimit(offered);
  for (let i = 0; i < calls.length; i++) {
    if (i >= limit) return { calls: calls.slice(0, 1), stopped: true };
    if (!finishing && !calls[i].name) continue;
    let args: string;
    try { args = JSON.stringify(canonical(JSON.parse(calls[i].arguments))); }
    catch {
      if (!finishing) continue; // Never compare partial streaming prefixes.
      args = calls[i].arguments.trim();
    }
    const key = JSON.stringify([calls[i].name, args]);
    if (keys.has(key)) return { calls: calls.slice(0, i <= policy.short_repeat_prefix ? i : 1), stopped: true };
    keys.add(key);
  }
  return { calls, stopped: false };
}
