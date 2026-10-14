import type { ServerSummary } from '#lib/types.js';

export interface RunningServer {
  id: string;
  /** A gateway server's `gw-<alias>` name (as in the sidebar), else the model file name, else the instance id. */
  label: string;
  /** Model file name, when known. */
  file: string | null;
  port: number | null;
}

/** Running servers other than the selected one (the gateway's `gw-*` servers included), in list order. */
export function otherRunningServers(servers: Pick<ServerSummary, 'id' | 'status' | 'active'>[], selectedId: string): RunningServer[] {
  return servers
    .filter((s) => s.id !== selectedId && s.status === 'Running')
    .map((s) => {
      const file = s.active?.model_path?.split(/[\\/]/).filter(Boolean).pop() || null;
      const label = s.id.startsWith('gw-') ? s.id : file || s.id;
      return { id: s.id, label, file, port: s.active?.port ?? null };
    });
}
