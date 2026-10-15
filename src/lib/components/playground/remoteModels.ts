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
