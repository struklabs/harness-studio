/** Shared chat capability filter (unknown capability remains usable). */
export function chatModelsOnly<T extends { chat?: boolean | null }>(models: T[]): T[] {
  return models.filter(model => model.chat !== false);
}
