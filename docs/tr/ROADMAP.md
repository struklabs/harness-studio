# Yol Haritası

[English](../en/ROADMAP.md)

Bu, Harness Studio'nun herkese açık yol haritasıdır. Söz değil yön gösterir: öncelikler geri bildirimle değişir. Bir şey önermek veya tartışmak için [GitHub Discussions](https://github.com/struklabs/harness-studio/discussions) üzerinde konu ya da [issue](https://github.com/struklabs/harness-studio/issues) açın. Yayımlanan değişiklikler [değişiklik günlüğünde](../CHANGELOG.md); Harness'ın benzer araçlarla karşılaştırması [FEATURES.md](FEATURES.md) ve [COMPARISON.md](COMPARISON.md) içindedir.

**Ürün hedefi.** Metin, görsel, konuşma, embedding, rerank ve araç iş yüklerini tek yerden yöneten yerel bir yapay zekâ kontrol merkezi. Varsayılan çalışma zamanı llama.cpp'dir; diğerleri ihtiyaç duyduğunuzda kurduğunuz modüllerdir.

**Kapsam hedefi.** Hugging Face model filtresindeki her görev ve kütüphane (çok kipli, bilgisayarlı görü, NLP, ses, tablo, pekiştirmeli öğrenme, çizge) Harness'te çalışır: mümkün olan yerde yerel motorlarla (llama.cpp, ONNX Runtime, stable-diffusion.cpp, whisper.cpp, sherpa-onnx, ggml ses motorları), olmayan yerde isteğe bağlı, yönetilen Python motorlarıyla.

**İlkeler.**
- LM Studio ve Ollama'nın yaptığını, llama.cpp'nin gelişmiş denetimlerini gizlemeden yapmak.
- Gözlemlenebilirlikte ve ajan kullanımında daha iyi olmak, düşük kaynak kullanmak.
- Önce yerel, bulut isteğe bağlı.
- Uygulama, CLI, Control API ve MCP aynı Rust servislerini kullanır.

**0.1.1 sürümü.** Odağı mevcut özelliklerin kararlılığı, hata düzeltmeleri, performans ve kullanıcı deneyimidir. Düşünme seçici (sohbet başına Kapalı, Hafif, Normal, Derin düzeyleri, main'de) ve aşağıdaki yol haritasının geri kalanı bitince çıkar. Sunucu kipi ile araç, MCP ve skill kataloğu 0.1.1'de değil, 0.2.0'da planlanıyor.

## Şimdi

- **Hugging Face görev kapsamı.** Her görevin en çok indirilen modelleri üzerinde ölçüldü (Ekim 2026): metin üretiminde ilk 300 modelin 295'i, görsel-dil modellerinin %98'i çalışıyor; ama yazıdan sese %29, konuşma tanıma %39, görselden görsele %45'te; sesten sese, konuşmacı ayırma (diarization) ve NLP ile bilgisayarlı görü sınıflandırma görevleri henüz yok. Süren işler:
  - ONNX Runtime üzerinde Python'suz pipeline'lar, `/v1/hf/{model}` arkasında (Hugging Face Inference API biçimi). main'de: metin sınıflandırma, sıfır-atış sınıflandırma, varlık tanıma (NER), maske doldurma, soru yanıtlama, görsel sınıflandırma, nesne algılama, bölütleme, derinlik tahmini, sıfır-atış görsel sınıflandırma (CLIP, SigLIP), görsel öznitelik çıkarma, ses sınıflandırma ve CTC konuşma tanıma (wav2vec2, HuBERT, WavLM). Ayrıca main'de: görselden metne (ViT + GPT-2 ile altyazı), sıfır-atış nesne algılama (OWL-ViT, OWLv2) ve ışın aramasıyla (beam search) metinden metne üretim, özetleme ve çeviri (T5, BART, Marian).
  - ggml ses motorları: audio.cpp metinden konuşma, deneysel ve varsayılan olarak kapalı bir motor olarak main'de (kendi kimlik doğrulaması olmadığından rastgele bir loopback portunda, yalnız gateway'in konuşma yolunun arkasında çalışır). CrispASR konuşma tanıma (Canary, wav2vec2 ailesi modeller ve daha fazlası) ve metinden konuşma (Kokoro, Pocket TTS) main'de; audio.cpp ayrıca ses ayırma ve iyileştirme yapıyor (Sesi İşle). llama.cpp ses girişi main'de: projektörü duyan bir sohbet modeli (LFM2.5-Audio, Qwen3-ASR, Voxtral...) `/v1/audio/transcriptions`'ta ve Yazıya Çevir'de de yazıya çeviriyor (en çok 25 sn'lik parçalarla). LFM2.5-Audio'nun ses çıkışı upstream llama.cpp'yi bekliyor (sunucusunda henüz ses çıkışı yok). transcribe.cpp konuşma tanıma (handy-computer GGUF'ları) main'de; CUDA sürümü olan yönetilen bir Python ortamında çalışıyor. Devam ediyor: llama.cpp metinden konuşma (Qwen3-TTS).
  - main'de: görsel düzenleme (`/v1/images/edits`, FLUX.2 klein), video üretimi (Wan; OpenAI Videos biçiminde asenkron bir `/v1/videos` iş API'si olarak) ve daha çok sherpa-onnx ailesi (konuşmacı ayırma, Nemotron, Cohere Transcribe, MMS sesleri).
  - main'de: karar uç noktaları (`/v1/decisions` ve `/v1/systemone`) ve Model Hub'da karar modeli görev filtresi.
  - Windows AMD'de gerçek çıkarım koşuları: `/v1/hf/{model}` arkasında DirectML ve CPU üzerinde ONNX modelleri, 11 görü ve NLP görevinde en çok indirilen 35 model, 49/70 → 70/70 deneme. Sesten sese ve konuşmacı ayırma Model Hub ve `/v1/audio/process` üzerinden çalışıyor: RE-USE, HTDemucs (6 kanallı dahil), Nemotron 3 Diarization ve Sortformer.
  - Apple Silicon'da gerçek çıkarım koşuları, görev başına Mac'e uygun ilk 30 model (yukarıdaki Hub kararı yüzdelerinden farklı bir ölçü): metinden konuşma 2/30 → 13/30, konuşma tanıma 7/30 → 14/30 ve sesten sese 0/30 → 2/30 (Mel-Band-RoFormer ile vokal ayırma).
  - Model Hub uyumluluk işaretleri bir depoyu hangi motorun çalıştırdığını söyler; yerelde hiçbir motorun çalıştıramadığı biçimleri (AWQ, FP8, NVFP4 checkpoint'leri) indirmeden önce reddeder.
- **Platform doğrulaması.** 0.1.0 ile Windows kurulum paketi (NSIS), imzalı ve Apple onaylı (notarize) macOS DMG'leri ve önizleme olarak bir Linux `.deb` paketi gelir. GStreamer medya eklentilerini içeren ve otomatik güncellenen Linux AppImage main'de, sonraki sürümle gelir. Linux'u daha çok makinede (AMD, NVIDIA, Intel) ve dağıtımda doğrulayıp önizleme etiketini kaldırmak. Şimdiye kadar tamamlananlar: Windows NVIDIA turunun tamamı (kurulum ve sihirbaz, birkaç sürüm adayı boyunca güncelleyici ve kurulum paketine devri, sohbet ve araç onayı, bellek yetersizliğinden kurtarma, tarayıcı kaynak (origin) koruması, GPU bellek telemetrisi, gateway'de yükleme, boşta boşaltma ve çıkarma, gateway üzerinden stable-diffusion.cpp ve diffusers görselleri, CUDA'da ses motorları, ONNX CUDA'dan DirectML'e geri dönüş ve yalnız belgelenmiş verileri bırakan temiz kaldırma); NVIDIA'da NVML telemetrisi; macOS'ta tarayıcı kaynak koruması; Windows'ta kendi tırnaklı kullanıcı başına Run girdisiyle oturum açılışında başlatma, güncelleyici devri ve yalnız okuma için uzun yol işleme.
- **İlk çalıştırma cilası.** Her platformda boş veri klasörleriyle uçtan uca ilk çalıştırma akışı (sihirbaz, model bulma, indirme, başlatma, sohbet). NVIDIA ekran kartlı Windows'ta, ilk çalıştırma turu dahil, ve macOS'ta (Apple Silicon) doğrulandı; diğer platformlar kaldı.
- **Daha fazla donanımda ölçüm.** Karşılaştırma paketini (`bench/compare/`) Linux üzerinde çalıştırmak, diğer araçları aynı oturumda yeniden ölçmek. Windows'ta NVIDIA/CUDA tamamlandı, gateway sonuçlara göre ayarlandı: RTX 3050 Ti'de Harness ile doğrudan llama.cpp ([docs/benchmarks/2026-10-15-nvidia](../benchmarks/2026-10-15-nvidia/summary.md)).
- **Erişilebilirlik denetimleri.** Küçük pencereler, yüksek arayüz ölçeği ve Windows zorunlu renkler kipi. Küçük pencereler ve zorunlu renkler (yüksek karşıtlık) Windows'ta tamamlandı; ayrıca ekran okuyucu ve klavye düzeltmeleri (kaydırıcı ve tekrar eden düğme adları, komut paleti, başlık sırası, model seçici odağı, kaydırılabilir kod blokları) ile dar pencereler ve yüksek ekran ölçeği için bölüm menüsü çekmecesi tamamlandı; macOS'ta VoiceOver denetimi ve düzeltmeleri (komut paleti, ilk çalıştırma sihirbazının arkasındaki uygulama, pencere başlığı, palet sonuç listesinin adı) tamamlandı; yüksek arayüz ölçeği ve Linux denetimleri kaldı.
- **Linux masaüstü ayrıntıları (tamamlandı).** Yerel `<select>` görünümü artık temayı izliyor, tepsi simgesi StatusNotifierItem ile çalışıyor, GPU bellek raporlama AMD APU'larda GTT'yi sayıyor (fdinfo), NVIDIA'da NVML telemetrisi doğrulandı ve OpenTelemetry dışa aktarımı anahtar zinciri olmadan çalışıyor. Linux'ta bildirilen gri şerit Harness'a ait değildi (Ubuntu kurulum penceresiydi); bu kayıt kapatıldı.
- **Harness Link (tamamlandı).** Bağlantılar altında uzak bir Harness'ı (adres ve token) ekleyip modellerini Playground'da, yerel gateway'de (`remote:<bağlantı>/<model>` olarak) ve Kütüphanem'deki salt okunur Uzak grubunda kullanmak. Token'lar işletim sistemi anahtar zincirinde durur; `http` yalnızca açık onayla yerel ya da özel ağlarla sınırlıdır, HTTPS sunucu kipi gelene kadar bir ters vekil üzerinden kurulur. Chat, completions, responses, embeddings, rerank ve messages çalışır; görsel, ses ve video çalışmaz.

## Sıradaki

- **Python motorları (isteğe bağlı).** Motor başına yönetilen ve kaldırılabilir bir Python ortamı (uv, hash ile sabitlenmiş kilit dosyaları, sistem geneline hiçbir şey kurulmaz); böylece her Hugging Face görevi ve kütüphanesi çalışabilir: tüm transformers pipeline'ları ve diffusers yazıdan görsel için bir `python-hf` işçisi (GGUF ya da ONNX sürümü olmayan modellerde yedek), ardından gateway arkasında OpenAI uyumlu sunucular olarak vLLM ve SGLang. `python-hf` main'de; konuşma ve görsel modelleri OpenAI ses ve görsel yollarını da yanıtlıyor; diffusers modelleri görsel de düzenliyor (görselden görsel ve inpainting), örnekleyici seçimi ve istek başına görsel boyutu sınırıyla. vLLM Windows'ta yerel olarak çalışmadığı için CUDA vLLM, Linux NVIDIA aşamasına kaldı. İşçiler yalnız loopback'te dinler, modelleri çevrimdışı ve varsayılan olarak `trust_remote_code` kapalı yükler; Linux'ta ağ arayüzü olmadan çalışır (`unshare -rn`; D-Bus gibi Unix soketleri erişilebilir kalır), Windows ve macOS'ta yalıtım ortam değişkenleriyle yapılır (çevrimdışı Hub, proxy yok).
  - CPU'da vLLM denendi ve ertelendi (2026-10-14, v0.31.0, Ryzen 7 4700U, AVX2). Resmi CPU paketi ağ ad alanında çevrimdışı çalışıyor; ancak ortam yaklaşık 3 GB, ilk istek triton-cpu çekirdeklerini sistemin C derleyicisiyle derlerken yaklaşık 2,5 dakika bekliyor (çalışma anında gcc gerekiyor) ve 0.5B bir model saniyede yaklaşık 12 token üretiyor; bu, aynı dizüstünde llama.cpp'nin çok gerisinde. vLLM bunun yerine CUDA'yı hedefliyor.
- **Sunucu kipi (0.2.0 için planlandı).** Harness'ı kendi (çoğunlukla ekransız Linux) sunucunuzda çalıştırıp her şeyi tarayıcıdan yönetmek:
  - `harness serve` ile sunulan, parola veya token oturumlu bir web arayüzü.
  - Sunucu Yönetimi alanı: alan adı ve HTTPS, erişim ve portlar, kullanıcılar ve tokenlar, servis durumu, güncellemeler, günlükler, yedekleme ve geri yükleme.
  - Arayüzden otomatik SSL (Let's Encrypt; DNS ve port denetimleriyle).
  - Tek satırlık kurulum betiği ve systemd servisi olan bir Linux sunucu paketi (tar.gz, .deb).
  - CUDA, ROCm, Vulkan ve CPU için Docker imajları.
- **Control API SDK'ları.** OpenAPI şemasından üretilen TypeScript ve Python istemcileri main'de (`sdk/`) ve CI'da çalışan bir sunucuya karşı sınanıyor. TypeScript SDK, npm'de `@struklabs/harness-studio-sdk` 0.1.0 olarak provenance ile yayımlandı; yayımlama, npm Trusted Publishing (OIDC, token yok) kullanan elle tetiklenen bir iş akışıyla (`workflow_dispatch`) yapılıyor. Python SDK, Trusted Publishing kurulduktan sonra PyPI'a gidecek.
- Apple Silicon için **MLX motoru**: sohbet (mlx-lm), görsel sohbet (mlx-vlm) metinden konuşma (mlx-audio: Qwen3-TTS, Chatterbox Turbo, Soprano, VoxCPM2, Spark-TTS) ve konuşma tanıma (mlx-audio Whisper ve Parakeet) main'de, sonraki sürümle gelir. Chatterbox Turbo, Soprano, VoxCPM2 ve Spark-TTS artık Apple Silicon'da konuşuyor; OuteTTS, Orpheus ve Dia kodeklerini (DAC veya SNAC) modelle birlikte indiriyor. Ayrıca main'de: macOS bellek yetersizliği (jetsam) algılaması, birleşik belleğin GPU payına sığmayan MLX yüklemelerini reddeden bellek ön denetimi ve uzun sohbetlerin belleği tüketmesini önleyen bayt sınırlı istem önbelleği. CSM artık Apple Silicon'da Mimi kodeğiyle konuşuyor.
- **Her platformda otomatik güncelleme.** İmzalı uygulama içi güncelleyici 0.1.0'da Windows için, 0.1.1'den itibaren macOS için geliyor; Linux paketleri ardından. Windows güncelleyicisi, kurulum paketi başlamadan önce sunucuları durdurması dahil, uçtan uca doğrulandı.
- **İmzalı Windows kurulum paketi.** Kod imzalı Windows kurulum paketi (macOS uygulaması zaten imzalı ve notarize edilmiş).

## Daha sonra

- **Araç ve MCP kayıt defteri (0.2.0 için planlandı, bir skill kataloğuyla birlikte).** Araçların ve MCP sunucularının topluluk dizini (`struklabs/harness-registry`): izinleri, yayıncıyı, kaynağı, sürümü ve SHA-256'yı içeren manifestler; CI doğrulaması; hash doğrulamasıyla sandbox'a kurulum; uygulamanın içinden yayımlama; doğrulanmış ve topluluk etiketleri.
- Zaten çalıştırdığınız bir ComfyUI örneğine **ComfyUI köprüsü**.
- Çok sayıda araç kuruluyken her istek için ilgili araçları seçen **araç yönlendirici** (embedding ve rerank altyapısıyla).
- Görsel, kod ve yazı üretimi ile ses model motorları için **daha fazla Oluştur sayfası**.
- Yerel modellerle çalışan **uygulama içi kodlama çalışma alanı**.
- **Değerlendiriliyor (söz verilmedi):**
  - faster-whisper motoru; bir NVIDIA ölçümü yapılana kadar ertelendi. Şimdilik Model Hub, CTranslate2 Whisper çıktılarını whisper.cpp sürümlerine yönlendiriyor.
  - VieNeu metinden konuşma; üçüncü taraf SEA-G2P yerel kütüphanesine ihtiyaç duyduğu için askıda.
- **Kararsız:** OpenVINO ve MLC.

## Planlanmıyor

- Ollama kayıt defterinden çekme (`hf.co/...` başvuruları desteklenir).
- Aynı modelleri yerel bir motor zaten çalıştırırken normal kullanım için Python gerektiren motorlar (text-embeddings-inference; faster-whisper değerlendiriliyor, bkz. Daha sonra). Python motorları isteğe bağlı eklenti olarak kalır, temel uygulama için hiçbir zaman gerekmez.
