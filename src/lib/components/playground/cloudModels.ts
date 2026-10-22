import { isRemoteModel } from './remoteModels.ts';

export interface CloudModel {
  id: string;
  model: string;
  provider: string;
  kind: 'openai' | 'gemini' | 'anthropic' | 'openrouter' | 'custom' | 'custom_anthropic';
  label: string;
  state: 'online' | 'offline' | 'unauthorized' | 'key_missing';
  display_name: string | null;
  context_length: number | null;
  input_price: number | null;
  output_price: number | null;
  tools: boolean | null;
  vision: boolean | null;
  reasoning?: boolean | null;
  price_custom?: boolean;
}

export function isCloudModel(id: string): boolean {
  return /^cloud:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\/.+/.test(id);
}

/** Served by another machine (Harness Link) or a cloud provider: always "available", never loaded here. */
export function isExternalModel(id: string): boolean {
  return isRemoteModel(id) || isCloudModel(id);
}

export function filterCloudModels(models: CloudModel[], query: string): CloudModel[] {
  const words = query.trim().toLowerCase().split(/\s+/);
  return models.filter((m) => {
    const text = `${m.id} ${m.model} ${m.display_name ?? ''} ${m.provider} ${m.label}`.toLowerCase();
    return words.every((word) => text.includes(word));
  });
}

/** "Cloud: <this>": the provider's brand, or the user's name for a custom endpoint. */
export function cloudLabel(m: Pick<CloudModel, 'kind' | 'label' | 'provider'>): string {
  return m.kind === 'custom' || m.kind === 'custom_anthropic' ? m.provider : m.label;
}

/** Picker groups, one per provider, in the order the providers were listed. */
export function groupCloudModels(models: CloudModel[]): { provider: string; label: string; models: CloudModel[] }[] {
  const groups = new Map<string, { provider: string; label: string; models: CloudModel[] }>();
  for (const m of models) {
    const group = groups.get(m.provider) ?? { provider: m.provider, label: cloudLabel(m), models: [] };
    group.models.push(m);
    groups.set(m.provider, group);
  }
  return [...groups.values()];
}

/** USD per million tokens, short: 0.15, 2.5, 15. */
export function formatPrice(value: number): string {
  return value === 0 ? '0' : value < 0.01 ? value.toPrecision(2) : String(Math.round(value * 100) / 100);
}

/** A price field's value as sent to `cloud_set_prices`: a number input bound with `bind:value`
 *  holds a number once typed in (null when cleared), a string while untouched. */
export function priceValue(v: string | number | null | undefined): number | null {
  return v == null || String(v).trim() === '' ? null : Number(v);
}
