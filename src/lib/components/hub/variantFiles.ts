import type { HfModelVariant, LocalModel } from '#lib/types.js';

/**
 * A variant is local when every shard of it is present; the matched file wins. Otherwise the
 * repo downloaded as a whole folder (MLX, Python engine) stands for it.
 */
export function localVariantFor(
  variant: HfModelVariant,
  localByFile: ReadonlyMap<string, LocalModel>,
  localFolder?: LocalModel,
): LocalModel | undefined {
  const files = variant.shards.map((s) => s.filename.replace(/\\/g, '/').toLowerCase());
  const byFile = files.length > 0 && files.every((file) => localByFile.has(file)) ? localByFile.get(files[0]) : undefined;
  return byFile ?? localFolder;
}
