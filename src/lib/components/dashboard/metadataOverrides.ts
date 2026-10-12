export const METADATA_TYPES = ['int', 'float', 'bool', 'str'] as const;
export type MetadataType = (typeof METADATA_TYPES)[number];
export interface MetadataOverrideRow {
  key: string;
  type: MetadataType;
  value: string;
}

/** Catalog lists and older comma/newline text settings share the same editor. */
export function parseMetadataOverrides(input: unknown): MetadataOverrideRow[] {
  const items = Array.isArray(input) ? input : typeof input === 'string' ? [input] : [];
  return items.flatMap((item) => typeof item === 'string' ? item.split(/[,\n]/) : []).flatMap((item) => {
    const match = item.trim().match(/^([^=]+)=(int|float|bool|str):([\s\S]*)$/);
    return match ? [{ key: match[1].trim(), type: match[2] as MetadataType, value: match[3] }] : [];
  });
}

export function validMetadataOverride(row: MetadataOverrideRow): boolean {
  if (!row.key.trim() || /[=,\s]/.test(row.key.trim()) || /[,\r\n]/.test(row.value)) return false;
  if (row.type === 'str') return true;
  const value = row.value.trim();
  if (row.type === 'bool') return value === 'true' || value === 'false';
  if (!value || !Number.isFinite(Number(value))) return false;
  return row.type === 'float' || /^[-+]?\d+$/.test(value);
}

export function serializeMetadataOverrides(rows: MetadataOverrideRow[]): string[] {
  return rows.filter(validMetadataOverride).map((row) =>
    `${row.key.trim()}=${row.type}:${row.type === 'str' ? row.value : row.value.trim()}`
  );
}

/** Applying a recommendation replaces conflicting rows and preserves unrelated keys. */
export function applyMetadataOverride(input: unknown, fix: string): string[] {
  const recommended = parseMetadataOverrides(fix)[0];
  if (!recommended) return serializeMetadataOverrides(parseMetadataOverrides(input));
  return serializeMetadataOverrides([
    ...parseMetadataOverrides(input).filter((row) => row.key !== recommended.key),
    recommended,
  ]);
}

export function metadataOverrideApplied(input: unknown, fix: string): boolean {
  const recommended = parseMetadataOverrides(fix)[0];
  if (!recommended) return false;
  const sameKey = parseMetadataOverrides(input).filter((row) => row.key === recommended.key);
  return sameKey.length === 1 && sameKey[0].type === recommended.type && sameKey[0].value.trim() === recommended.value;
}
