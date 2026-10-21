// The simulator end-to-end run (debug builds only, see `e2e_script` in src-tauri): pair with a
// Harness, stream a chat from its first model, then open the job list and the manage screen.
// It drives the same code the screens use; the test reads the screens from screenshots.
import { invoke } from '@tauri-apps/api/core';
import { chat } from './chat.svelte.ts';
import { connections } from './connections.svelte.ts';
import { gatewayModels } from './harness.ts';

interface Script {
  gateway: string;
  api?: string;
  token: string;
  prompt: string;
  /** Also a BYOK provider (e.g. a custom OpenAI-compatible endpoint) and one chat through it. */
  provider?: { kind: 'openai' | 'gemini' | 'anthropic' | 'openrouter' | 'custom'; name: string; base: string; key: string };
}

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

export async function runE2e(show: (tab: 'chat' | 'jobs' | 'manage' | 'connections') => void): Promise<void> {
  const raw = await invoke<string | null>('e2e_script').catch(() => null);
  if (!raw) return;
  const script = JSON.parse(raw) as Script;
  show('connections');
  const c = connections.harness.find((h) => h.gatewayUrl === script.gateway.replace(/\/+$/, '')) ?? (await connections.addHarness('Mac (e2e)', script.gateway, script.api ?? '', script.token));
  await wait(3000);
  const [model] = await gatewayModels(c);
  chat.target = { type: 'harness', connection: c.id, model: model.id };
  show('chat');
  await chat.send(script.prompt);
  while (chat.busy) await wait(200);
  if (script.provider) {
    const { kind, name, base, key } = script.provider;
    show('connections');
    const p = connections.providers.find((x) => x.baseUrl === base) ?? (await connections.addProvider(kind, name, base, key));
    await wait(3000);
    const listed = connections.status[p.id]?.models ?? [];
    chat.clear();
    chat.target = { type: 'provider', connection: p.id, model: listed[0]?.id ?? '' };
    show('chat');
    await chat.send(script.prompt);
    while (chat.busy) await wait(200);
    await wait(6000);
  }
  await wait(4000);
  show('jobs');
  await wait(15000);
  show('manage');
}
