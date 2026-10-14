import type { CoreTool } from '#lib/tools-client.js';

export interface WorkbenchCall {
  id: string;
  type: 'function';
  function: { name: string; arguments: string };
}

/** Keep the model's original call (including raw argument text) in the next round. */
export function normalizeWorkbenchCall(call: WorkbenchCall): WorkbenchCall {
  if (!call?.function?.name || typeof call.function.arguments !== 'string') throw new Error('Invalid tool call');
  return { id: call.id || 'workbench_call', type: 'function', function: { ...call.function } };
}

export function toolRoundMessages(prompt: string, assistant: { content?: string | null; reasoning_content?: string; tool_calls: WorkbenchCall[] }, result: string, source: 'sample' | 'real' = 'sample') {
  if (assistant.tool_calls.length !== 1) throw new Error('Expected one tool call');
  return [
    { role: 'system', content: source === 'sample'
      ? 'Answer the original question using the provided tool result. This is sample test data; clearly label the answer as an example, not live information.'
      : 'Answer the original question using the provided tool result. The result was obtained by executing a real Harness tool, not by simulating sample data. Report what the result supports; do not invent facts.' },
    { role: 'user', content: prompt },
    { role: 'assistant', ...assistant },
    { role: 'tool', tool_call_id: assistant.tool_calls[0].id, content: result },
  ];
}

export function sampleToolResult(preset: string, args: unknown): string {
  const a = args && typeof args === 'object' ? args as Record<string, unknown> : {};
  if (preset === 'weather') return JSON.stringify({ location: a.location ?? 'Istanbul', temperature_c: 18, condition: 'partly cloudy' }, null, 2);
  if (preset === 'calc') return JSON.stringify({ expression: a.expression ?? '(144 * 12) + 256', result: 1984 }, null, 2);
  if (preset === 'read_file') return JSON.stringify({ filepath: a.filepath ?? 'C:/projects/config.json', content: '{"app":"demo","debug":false}' }, null, 2);
  return '';
}

/** Dangerous tools are deliberately excluded, even under allow_all. */
export function runnableWorkbenchTool(name: string, tools: CoreTool[]): CoreTool | undefined {
  return tools.find(t => t.name === name && t.class !== 'dangerous');
}

/** Adapt the templates without changing the model's original formatting-test call. */
export function realWorkbenchCall(name: string, args: unknown): { name: string; args: Record<string, unknown> } {
  if (!args || typeof args !== 'object' || Array.isArray(args)) throw new Error('Expected JSON object arguments');
  const a = args as Record<string, unknown>;
  if (name === 'read_file' && 'filepath' in a && !('path' in a)) return { name, args: { path: a.filepath } };
  return name === 'get_current_weather' ? { name: 'weather', args: { location: a.location } } : { name, args: a };
}
