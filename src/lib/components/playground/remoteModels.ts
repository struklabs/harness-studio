export interface RemoteModel {
  id: string;
  model: string;
  link: string;
  state: 'online' | 'offline' | 'unauthorized';
}

export function isRemoteModel(id: string): boolean {
  return /^remote:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\/.+/.test(id);
}

export function filterRemoteModels(models: RemoteModel[], query: string): RemoteModel[] {
  const words = query.trim().toLowerCase().split(/\s+/);
  return models.filter((m) => {
    const text = `${m.id} ${m.model} ${m.link}`.toLowerCase();
    return words.every((word) => text.includes(word));
  });
}

/** Library "Remote" group: hidden without links, a hint while the local gateway is off, else the rows. */
export function remoteGroupState(linkCount: number, gatewayRunning: boolean): 'hidden' | 'gateway_off' | 'list' {
  if (linkCount <= 0) return 'hidden';
  return gatewayRunning ? 'list' : 'gateway_off';
}
