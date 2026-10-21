// Onboarding flow: coach marks tour, privacy consent modal, app init/loading screens,
// and the remaining (non-nav) header tagline.
const tr = {

  // CoachMarks
  'onboarding.coach_step1_title': '1. Adım: Yerel Modeller',
  'onboarding.coach_step1_desc':
    'Bilgisayarınızda taranan tüm GGUF modeller burada listelenir. Tek bir model seçebilir ya da "Router Modu" ile tüm modelleri aynı anda sunabilirsiniz. Yeni model aramak veya indirmek için sol menüdeki Model Hub\'ı kullanın.',
  'onboarding.coach_step1_hint': 'İpucu: Henüz modeliniz yoksa Model Hub sekmesinden tek tıkla indirebilirsiniz.',
  'onboarding.coach_step2_title': '2. Adım: Çalıştırma Parametreleri',
  'onboarding.coach_step2_desc':
    'Modelin donanımınızda nasıl çalışacağını belirleyin. GPU Katmanları (-ngl) ekran kartınızın gücünü kullanır; Bağlam Penceresi (-c) ise modelin tek seferde hafızasında tutabileceği konuşma uzunluğudur.',
  'onboarding.coach_step2_hint': 'İpucu: Otomatik bağlam GGUF eğitim bağlamını en fazla 32.768 token olarak kullanır (bilinmiyorsa 32.768); belleğe sığdırma bunu azaltabilir. Otomatik iş parçacığını kullanın.',
  'onboarding.coach_step3_title': '3. Adım: Sunucuyu Başlat',
  'onboarding.coach_step3_desc':
    'Modeli ve parametreleri seçtikten sonra büyük "Sunucuyu Başlat" butonuna tıklayarak yerel llama.cpp motorunu çalıştırın. Sunucu açıldığında Cursor, Continue veya Web Arayüzü ile hemen sohbet edebilirsiniz.',
  'onboarding.coach_step3_hint': 'İpucu: Başlat butonunun yanında doğrudan llama.cpp dahili sohbet arayüzünü açan bir kısayol belirir.',
  'onboarding.coach_step4_title': '4. Adım: Canlı Telemetri ve Kaynaklar',
  'onboarding.coach_step4_desc':
    'Sunucu çalışırken modelin saniyede kaç token (kelime) ürettiğini, yanıt gecikmesini, GPU VRAM ve Sistem RAM doluluğunu gerçek zamanlı olarak izleyin.',
  'onboarding.coach_step4_hint': 'İpucu: Modelin katmanları GPU\'ya tam sığdığında üretim hızı (Tokens/s) en üst seviyeye ulaşır.',
  'onboarding.coach_step5_title': '5. Adım: Canlı Sunucu Logları',
  'onboarding.coach_step5_desc':
    'Arka planda çalışan llama-server motorunun başlama adımlarını, katmanların belleğe yüklenişini ve olası uyarıları buradan canlı takip edebilirsiniz.',
  'onboarding.coach_step5_hint': 'İpucu: Dilediğinizde "Oto Kaydır"ı kapatabilir veya ekranı temizleyebilirsiniz.',
  'onboarding.coach_badge': 'Rehber · {current} / {total}',
  'onboarding.coach_prev': 'Geri',
  'onboarding.coach_skip': 'Turu Bitir',
  'onboarding.coach_finish': 'Anladım, Başla',
  'onboarding.coach_next': 'İleri',

  // +page.svelte (init / loading screens)
  'onboarding.init_error_title': 'Harness Studio başlatılamadı',
  'onboarding.retry': 'Tekrar Dene',
  'onboarding.loading_subtitle': 'Sistem ve donanım yapılandırması yükleniyor...',

  // Header.svelte (remaining hard-coded tagline)

  // Setup Wizard
  'onboarding.sg_reason_vram': '{mem} VRAM’e sığar',
  'onboarding.sg_reason_shared': '{mem} ortak bellek bütçesine sığar',
  'onboarding.sg_reason_tight': '{mem} VRAM’e zor sığar; bağlam uzunluğunu düşük tutun',
  'onboarding.sg_reason_tight_shared': '{mem} ortak bellek bütçesine zor sığar; bağlam uzunluğunu düşük tutun',
  'onboarding.sg_reason_hybrid': 'Bir kısmı GPU’da, kalanı CPU’da çalışır',
  'onboarding.sg_reason_cpu': 'CPU’da çalışır; yavaş olabilir',
  'onboarding.sg_reason_whisper': '{ram} boş RAM’e sığar',
  'onboarding.sg_reason_runtime': 'Ses yazıya çevirme için gerekli (GitHub sürümü, sağlama doğrulanır)',
  'onboarding.sg_reason_mlx_engine': 'Apple Silicon’da ses yazıya çevirmeyi MLX ile çalıştırır (mlx-audio, kendi Python ortamı)',
  'onboarding.sg_reason_mlx_whisper': 'Apple Silicon için MLX biçimli Whisper; yazıya döker ve İngilizceye çevirir',
};

const en: Record<keyof typeof tr, string> = {
  // CoachMarks
  'onboarding.coach_step1_title': 'Step 1: Local Models',
  'onboarding.coach_step1_desc':
    'All GGUF models scanned on your computer are listed here. You can pick a single model or serve all of them at once with "Router Mode". Use Model Hub in the left menu to find or download new models.',
  'onboarding.coach_step1_hint': "Tip: Don't have a model yet? Download one with a single click from the Model Hub tab.",
  'onboarding.coach_step2_title': 'Step 2: Launch Parameters',
  'onboarding.coach_step2_desc':
    'Decide how the model runs on your hardware. GPU Layers (-ngl) uses your graphics card\'s power; Context Window (-c) is how much conversation the model can hold in memory at once.',
  'onboarding.coach_step2_hint': 'Tip: Auto context uses the GGUF training context, capped at 32,768 tokens (32,768 if unknown); memory fitting may reduce it. Use Automatic threads.',
  'onboarding.coach_step3_title': 'Step 3: Start the Server',
  'onboarding.coach_step3_desc':
    'Once you\'ve picked a model and parameters, click the big "Start Server" button to run the local llama.cpp engine. When the server is up, you can chat right away with Cursor, Continue, or the Web UI.',
  'onboarding.coach_step3_hint': 'Tip: A shortcut appears next to the Start button to open the built-in llama.cpp chat UI directly.',
  'onboarding.coach_step4_title': 'Step 4: Live Telemetry and Resources',
  'onboarding.coach_step4_desc':
    "While the server is running, watch how many tokens (words) per second the model produces, response latency, and GPU VRAM and system RAM usage in real time.",
  'onboarding.coach_step4_hint': 'Tip: When the model\'s layers fully fit on the GPU, generation speed (Tokens/s) reaches its peak.',
  'onboarding.coach_step5_title': 'Step 5: Live Server Logs',
  'onboarding.coach_step5_desc':
    'Follow the background llama-server engine\'s startup steps, layers loading into memory, and any warnings live from here.',
  'onboarding.coach_step5_hint': 'Tip: You can turn off "Auto-Scroll" or clear the screen at any time.',
  'onboarding.coach_badge': 'Guide · {current} / {total}',
  'onboarding.coach_prev': 'Back',
  'onboarding.coach_skip': 'Skip Tour',
  'onboarding.coach_finish': 'Got It, Start',
  'onboarding.coach_next': 'Next',

  // +page.svelte (init / loading screens)
  'onboarding.init_error_title': 'Harness Studio failed to start',
  'onboarding.retry': 'Retry',
  'onboarding.loading_subtitle': 'Loading system and hardware configuration...',

  // Header.svelte (remaining hard-coded tagline)

  // Setup Wizard
  'onboarding.sg_reason_vram': 'Fits in {mem} of VRAM',
  'onboarding.sg_reason_shared': 'Fits the {mem} shared memory budget',
  'onboarding.sg_reason_tight': 'Barely fits in {mem} of VRAM; keep the context short',
  'onboarding.sg_reason_tight_shared': 'Barely fits in the {mem} shared memory budget; keep the context short',
  'onboarding.sg_reason_hybrid': 'Runs partly on the GPU, the rest on the CPU',
  'onboarding.sg_reason_cpu': 'Runs on the CPU; may be slow',
  'onboarding.sg_reason_whisper': 'Fits in {ram} of free RAM',
  'onboarding.sg_reason_runtime': 'Needed for speech to text (GitHub release, checksum verified)',
  'onboarding.sg_reason_mlx_engine': 'Runs speech to text with MLX on Apple Silicon (mlx-audio, in its own Python environment)',
  'onboarding.sg_reason_mlx_whisper': 'Whisper in MLX format for Apple Silicon; transcribes and translates to English',
};

export default { tr, en };
