import type { HfModelVariant, LocalModel } from '#lib/types.js';

/** A variant is local only when every shard is present, or its whole repo folder is local. */
export function localVariantFor(
  variant: HfModelVariant,
  localByFile: ReadonlyMap<string, LocalModel>,
  localFolder?: LocalModel,
): LocalModel | undefined {
  if (localFolder) return localFolder;
  const files = variant.shards.map((s) => s.filename.replace(/\\/g, '/').toLowerCase());
  return files.length > 0 && files.every((file) => localByFile.has(file))
    ? localByFile.get(files[0])
    : undefined;
}
