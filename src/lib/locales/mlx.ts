// The MLX engine (mlx): parameter catalog labels (params_mlx.toml) and the backend error keys of
// its launches.
const tr = {
  'mlxparam.max_tokens': 'En çok yanıt uzunluğu',
  'mlxparam.max_tokens_help': 'İstek belirtmediğinde bir yanıtın en çok kaç belirteç olacağı (boş = 8192).',
  'mlxparam.kv_bits': 'KV önbelleği bitleri',
  'mlxparam.kv_bits_help': 'KV önbelleğini 4 ya da 8 bite sıkıştırır: uzun bağlamda bellek azalır, ama istekler sırayla işlenir.',
  'mlxparam.prompt_cache_size': 'İstem önbelleği',
  'mlxparam.prompt_cache_size_help': 'Bellekte tutulan ayrı KV önbelleği sayısı (sohbetler arası yeniden kullanım).',
  'mlx.unsupported_macos': 'MLX modelleri macOS 14 veya üstü gerektirir; bu Mac’te çalışmaz. Aynı modelin GGUF sürümünü kullanın.',
  'hub.compat_mlx': 'MLX ile çalışır',
  'hub.compat_mlx_title': 'Bu MLX biçimli model Apple Silicon’da MLX motoruyla çalışır.',
  'hub.compat_needs_mlx': 'Gerekli: MLX motoru',
  'hub.compat_needs_mlx_title': 'Bu model MLX motoruyla çalışır; motor henüz kurulu değil. Depoyu açıp kurabilirsiniz (macOS 14+, yaklaşık 350 MB).',
};

const en: Record<keyof typeof tr, string> = {
  'mlxparam.max_tokens': 'Longest answer',
  'mlxparam.max_tokens_help': 'How many tokens an answer may have when the request doesn’t say (empty = 8192).',
  'mlxparam.kv_bits': 'KV cache bits',
  'mlxparam.kv_bits_help': 'Compresses the KV cache to 4 or 8 bits: less memory on long contexts, but requests are served one at a time.',
  'mlxparam.prompt_cache_size': 'Prompt cache',
  'mlxparam.prompt_cache_size_help': 'How many separate KV caches stay in memory (reused across conversations).',
  'mlx.unsupported_macos': 'MLX models need macOS 14 or later; they don’t run on this Mac. Use the GGUF version of the same model.',
  'hub.compat_mlx': 'Runs with MLX',
  'hub.compat_mlx_title': 'This MLX-format model runs on the MLX runtime on Apple Silicon.',
  'hub.compat_needs_mlx': 'Required: MLX runtime',
  'hub.compat_needs_mlx_title': 'This model runs on the MLX runtime, which isn’t installed yet. Open the repository to install it (macOS 14+, about 350 MB).',
};

export default { tr, en };
