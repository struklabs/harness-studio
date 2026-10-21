import { invoke } from '#lib/ipc.js';
import { BackendError } from '#lib/errors.js';

export type DeviceKind = 'microphone' | 'camera';
export type Verdict = 'allowed' | 'denied' | 'ask' | 'os_blocked';

/** The client name the desktop app uses for its own device access. */
export const UI_CLIENT = 'ui';

/** Why a device may not be used: the operating system or the user's own answer. */
export class DeviceRefused extends Error {
  constructor(readonly reason: 'os_blocked' | 'denied') {
    super(reason);
  }
}

interface Prompt {
  kind: DeviceKind;
  resolve: (choice: 'remember' | 'once' | 'deny') => void;
}

/** State of the device consent dialog (mounted once in SecurityDialogs.svelte). */
class DeviceConsentState {
  prompt = $state<Prompt | null>(null);

  ask(kind: DeviceKind): Promise<'remember' | 'once' | 'deny'> {
    this.prompt?.resolve('deny');
    return new Promise((resolve) => {
      this.prompt = { kind, resolve };
    });
  }

  settle(choice: 'remember' | 'once' | 'deny') {
    const p = this.prompt;
    this.prompt = null;
    p?.resolve(choice);
  }
}

export const deviceConsent = new DeviceConsentState();

/**
 * Two checks before the app opens the microphone or camera: the operating system's own switch
 * (read by the core) and Harness's consent. Resolves when the device may be used; throws
 * `DeviceRefused` otherwise. The first time the user is asked; "Allow and Remember" (or a
 * denial) is stored in the core and can be revoked on the Permissions page.
 */
export async function requireDevice(kind: DeviceKind): Promise<void> {
  let verdict: Verdict;
  try {
    verdict = await invoke<Verdict>('workloads_check', { client: UI_CLIENT, kind });
  } catch (e) {
    // Outside the desktop shell there is no core to ask; the browser shows its own prompt.
    if (e instanceof BackendError) throw e;
    return;
  }
  if (verdict === 'allowed') return;
  if (verdict === 'os_blocked') throw new DeviceRefused('os_blocked');
  if (verdict === 'denied') throw new DeviceRefused('denied');
  const choice = await deviceConsent.ask(kind);
  if (choice === 'deny') {
    await invoke('workloads_remember', { client: UI_CLIENT, kind, choice: 'deny' }).catch(() => {});
    throw new DeviceRefused('denied');
  }
  if (choice === 'remember') await invoke('workloads_remember', { client: UI_CLIENT, kind, choice: 'allow' });
}
