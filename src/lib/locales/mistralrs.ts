// mistral.rs runtime: the local-folder card in Settings > Runtime, the Hub notes
// for safetensors chat models, the parameter catalog labels (params_mistralrs.toml) and the
// backend error keys.
const tr = {
  // ---- Settings > Runtime: local folder ----
  'rt.mistralrs_title': 'Safetensors Modelleri (mistral.rs)',
  'rt.mistralrs_desc':
    'mistral.rs, Hugging Face safetensors modellerini (sohbet ve görsel anlayan modeller) GGUF\'a çevirmeden OpenAI uyumlu bir sohbet sunucusunda çalıştırır. Windows\'taki resmi sürüm yalnızca CPU ile çalıştığı için hazır indirme sunulmaz: CUDA desteğiyle kendiniz derleyin (cargo build --release --features cuda -p mistralrs-cli), sonra mistralrs.exe dosyasının bulunduğu klasörü (genellikle target\\release) aşağıdan ekleyin. Ekleme sırasında mistralrs --version çalıştırılarak klasör doğrulanır.',
  'rt.mistralrs_fill': 'Aileyi mistral.rs Olarak Seç',
  // ---- Hub ----
  'hub.compat_needs_mistralrs': 'Gerekli: mistral.rs motoru',
  'hub.compat_needs_mistralrs_title':
    'Bu safetensors modeli mistral.rs ile çalışır; motor henüz eklenmemiş. Kendi derlediğiniz mistral.rs klasörünü Ayarlar > Model Motoru\'ndan ekleyebilirsiniz.',
  'hub.compat_needs_mistralrs_drawer':
    'Gerekli: mistral.rs motoru. Modeli indirebilirsiniz ama çalıştırmak için kendi derlediğiniz mistral.rs klasörünü Ayarlar > Model Motoru\'ndan eklemeniz gerekir.',
  'hub.compat_open_runtime_settings': 'Model Motoru Ayarlarını Aç',
  // ---- Parameter catalog (params_mistralrs.toml) ----
  'mrparam.max_seq_len_help':
    'Modelin bir istekte işleyeceği en uzun bağlam (token). mistral.rs belleği bu uzunluğa göre ayırır; PagedAttention önbelleği de bu boyuta göre ayarlanır.',
  'mrparam.max_seqs': 'Eşzamanlı İstek',
  'mrparam.max_seqs_help': 'Sunucunun aynı anda işleyeceği en çok istek sayısı (--max-seqs). Boş bırakılırsa gateway en fazla 8 isteği aynı anda gönderir.',
  'mrparam.isq': 'Yükleme Sırasında Nicemleme (ISQ)',
  'mrparam.isq_help':
    'Ağırlıkları yüklerken nicemler, böylece model daha az bellek kullanır (örnek: 4, 8, q4k, q8_0). Boş = ağırlıklar olduğu gibi yüklenir.',
  'mrparam.dtype': 'Veri Türü',
  'mrparam.dtype_help': 'Ağırlıkların ve hesaplamanın veri türü. Otomatik: model ve donanım için uygun olan seçilir.',
  'mrparam.arch': 'Mimari',
  'mrparam.arch_help': 'Model mimarisini elle belirtir. Boş = config.json dosyasından okunur.',
  'mrparam.max_model_len': 'En Büyük Model Bağlamı',
  'mrparam.max_model_len_help': 'Modelin kabul edeceği en uzun bağlam. Boş = modelin kendi değeri.',
  'mrparam.paged_attn': 'PagedAttention',
  'mrparam.paged_attn_help': 'KV önbelleğini sayfalara bölerek yönetir (CUDA). Otomatik: desteklenen donanımda açılır.',
  'mrparam.pa_context_len': 'PagedAttention Bağlamı',
  'mrparam.pa_context_len_help': 'PagedAttention önbelleğinin token cinsinden boyutu. Boş = bağlam uzunluğu kadar.',
  'mrparam.pa_memory_mb': 'PagedAttention Belleği (MB)',
  'mrparam.pa_memory_mb_help': 'PagedAttention önbelleği için ayrılacak GPU belleği, MB cinsinden.',
  'mrparam.pa_memory_fraction': 'PagedAttention Bellek Oranı',
  'mrparam.pa_memory_fraction_help': 'PagedAttention önbelleği için boş GPU belleğinin ne kadarının ayrılacağı (0,05–1).',
  'mrparam.pa_cache_type': 'PagedAttention Önbellek Türü',
  'mrparam.pa_cache_type_help': 'KV önbelleğinin veri türü (örnek: auto, f8e4m3). Boş = otomatik.',
  // ---- Backend errors ----
  'process.mistralrs_loopback_only':
    'mistral.rs sunucusunun kendi kimlik doğrulaması olmadığı için yalnızca 127.0.0.1 / localhost\'ta dinler ({host}). Dışarıya gateway üzerinden açın.',
  'mistralrs.model_not_found': 'Safetensors model klasörü bulunamadı: {model}',
};

const en: Record<keyof typeof tr, string> = {
  'rt.mistralrs_title': 'Safetensors Models (mistral.rs)',
  'rt.mistralrs_desc':
    'mistral.rs runs Hugging Face safetensors models (chat and vision models) without converting them to GGUF, behind an OpenAI-compatible chat server. The official Windows release runs on the CPU only, so there is no managed download: build it yourself with CUDA (cargo build --release --features cuda -p mistralrs-cli), then add the folder that holds mistralrs.exe (usually target\\release) below. Adding it runs mistralrs --version to check the folder.',
  'rt.mistralrs_fill': 'Choose the mistral.rs Family',
  'hub.compat_needs_mistralrs': 'Required: mistral.rs runtime',
  'hub.compat_needs_mistralrs_title':
    'This safetensors model runs on mistral.rs, which hasn’t been added yet. Add the folder of your own mistral.rs build in Settings > Runtime.',
  'hub.compat_needs_mistralrs_drawer':
    'Required: mistral.rs runtime. You can download the model, but to run it you need to add the folder of your own mistral.rs build in Settings > Runtime.',
  'hub.compat_open_runtime_settings': 'Open Runtime Settings',
  'mrparam.max_seq_len_help':
    'The longest context (tokens) the model handles in one request. mistral.rs sizes its memory for it; the PagedAttention cache is sized to it too.',
  'mrparam.max_seqs': 'Concurrent Requests',
  'mrparam.max_seqs_help': 'How many requests the server runs at once (--max-seqs). Left empty, the gateway sends up to 8 at a time.',
  'mrparam.isq': 'In-Situ Quantization (ISQ)',
  'mrparam.isq_help':
    'Quantizes the weights while loading so the model takes less memory (for example 4, 8, q4k, q8_0). Empty = the weights load as they are.',
  'mrparam.dtype': 'Data Type',
  'mrparam.dtype_help': 'Data type of the weights and the computation. Auto picks what suits the model and the hardware.',
  'mrparam.arch': 'Architecture',
  'mrparam.arch_help': 'Sets the model architecture by hand. Empty = read from config.json.',
  'mrparam.max_model_len': 'Maximum Model Context',
  'mrparam.max_model_len_help': 'The longest context the model accepts. Empty = the model’s own value.',
  'mrparam.paged_attn': 'PagedAttention',
  'mrparam.paged_attn_help': 'Manages the KV cache in pages (CUDA). Auto turns it on where it is supported.',
  'mrparam.pa_context_len': 'PagedAttention Context',
  'mrparam.pa_context_len_help': 'Size of the PagedAttention cache in tokens. Empty = the context length.',
  'mrparam.pa_memory_mb': 'PagedAttention Memory (MB)',
  'mrparam.pa_memory_mb_help': 'GPU memory to set aside for the PagedAttention cache, in MB.',
  'mrparam.pa_memory_fraction': 'PagedAttention Memory Fraction',
  'mrparam.pa_memory_fraction_help': 'Share of the free GPU memory to set aside for the PagedAttention cache (0.05–1).',
  'mrparam.pa_cache_type': 'PagedAttention Cache Type',
  'mrparam.pa_cache_type_help': 'Data type of the KV cache (for example auto, f8e4m3). Empty = automatic.',
  'process.mistralrs_loopback_only':
    'The mistral.rs server has no authentication of its own, so it only listens on 127.0.0.1 / localhost ({host}). Expose it through the gateway.',
  'mistralrs.model_not_found': 'Safetensors model folder not found: {model}',
};

export default { tr, en };
