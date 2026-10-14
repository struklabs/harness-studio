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

## Şimdi

- **Hugging Face görev kapsamı.** Her görevin en çok indirilen modelleri üzerinde ölçüldü (Ekim 2026): metin üretiminde ilk 300 modelin 295'i, görsel-dil modellerinin %98'i çalışıyor; ama yazıdan sese %29, konuşma tanıma %39, görselden görsele %45'te; sesten sese, konuşmacı ayırma (diarization) ve NLP ile bilgisayarlı görü sınıflandırma görevleri henüz yok. Süren işler:
  - ONNX Runtime üzerinde Python'suz pipeline'lar, `/v1/hf/{model}` arkasında (Hugging Face Inference API biçimi). main'de: metin sınıflandırma, sıfır-atış sınıflandırma, varlık tanıma (NER), maske doldurma, soru yanıtlama, görsel sınıflandırma, nesne algılama, bölütleme, derinlik tahmini, sıfır-atış görsel sınıflandırma (CLIP, SigLIP), görsel öznitelik çıkarma, ses sınıflandırma ve CTC konuşma tanıma (wav2vec2, HuBERT, WavLM). Sürüyor: görselden metne, ardından metinden metne üretim, özetleme ve çeviri (T5, BART, Marian).
  - ggml ses motorları: audio.cpp metinden konuşma, deneysel ve varsayılan olarak kapalı bir motor olarak main'de (kendi kimlik doğrulaması olmadığından rastgele bir loopback portunda, yalnız gateway'in konuşma yolunun arkasında çalışır). Devam ediyor: CrispASR konuşma tanıma, ardından transcribe.cpp ve llama.cpp ses girişi ve çıkışı (LFM2-Audio, Qwen3-TTS).
  - main'de: görsel düzenleme (`/v1/images/edits`, FLUX.2 klein), video üretimi (Wan; OpenAI Videos biçiminde asenkron bir `/v1/videos` iş API'si olarak) ve daha çok sherpa-onnx ailesi (konuşmacı ayırma, Nemotron, Cohere Transcribe, MMS sesleri).
  - Model Hub uyumluluk işaretleri bir depoyu hangi motorun çalıştırdığını söyler; yerelde hiçbir motorun çalıştıramadığı biçimleri (AWQ, FP8, NVFP4 checkpoint'leri) indirmeden önce reddeder.
- **Platform doğrulaması.** 0.1.0 ile Windows kurulum paketi (NSIS), imzalı ve Apple onaylı (notarize) macOS DMG'leri ve önizleme olarak bir Linux `.deb` paketi gelir. GStreamer medya eklentilerini içeren ve otomatik güncellenen Linux AppImage main'de, sonraki sürümle gelir. Linux'u daha çok makinede (AMD, NVIDIA, Intel) ve dağıtımda doğrulayıp önizleme etiketini kaldırmak.
- **İlk çalıştırma cilası.** Her platformda boş veri klasörleriyle uçtan uca ilk çalıştırma akışı (sihirbaz, model bulma, indirme, başlatma, sohbet).
- **Daha fazla donanımda ölçüm.** Karşılaştırma paketini (`bench/compare/`) NVIDIA/CUDA ve Linux üzerinde çalıştırmak, diğer araçları aynı oturumda yeniden ölçmek.
- **Erişilebilirlik denetimleri.** Küçük pencereler, yüksek arayüz ölçeği ve Windows zorunlu renkler kipi.
- **Linux masaüstü ayrıntıları.** WebKitGTK'de yerel `<select>` görünümü, tepsi simgesi tıklama davranışı ve gerçek donanımda GPU bellek raporlama (NVML, fdinfo).

## Sıradaki

- **Python motorları (isteğe bağlı).** Motor başına yönetilen ve kaldırılabilir bir Python ortamı (uv, hash ile sabitlenmiş kilit dosyaları, sistem geneline hiçbir şey kurulmaz); böylece her Hugging Face görevi ve kütüphanesi çalışabilir: tüm transformers pipeline'ları ve diffusers yazıdan görsel için bir `python-hf` işçisi (GGUF ya da ONNX sürümü olmayan modellerde yedek), ardından gateway arkasında OpenAI uyumlu sunucular olarak vLLM ve SGLang. `python-hf` main'de; konuşma ve görsel modelleri OpenAI ses ve görsel yollarını da yanıtlıyor; diffusers modelleri görsel de düzenliyor (görselden görsel ve inpainting), örnekleyici seçimi ve istek başına görsel boyutu sınırıyla. vLLM Windows'ta yerel olarak çalışmadığı için CUDA vLLM, Linux NVIDIA aşamasına kaldı. İşçiler yalnız loopback'te dinler, modelleri çevrimdışı ve varsayılan olarak `trust_remote_code` kapalı yükler; Linux'ta ağ arayüzü olmadan çalışır (`unshare -rn`; D-Bus gibi Unix soketleri erişilebilir kalır), Windows ve macOS'ta yalıtım ortam değişkenleriyle yapılır (çevrimdışı Hub, proxy yok).
  - CPU'da vLLM denendi ve ertelendi (2026-10-14, v0.31.0, Ryzen 7 4700U, AVX2). Resmi CPU paketi ağ ad alanında çevrimdışı çalışıyor; ancak ortam yaklaşık 3 GB, ilk istek triton-cpu çekirdeklerini sistemin C derleyicisiyle derlerken yaklaşık 2,5 dakika bekliyor (çalışma anında gcc gerekiyor) ve 0.5B bir model saniyede yaklaşık 12 token üretiyor; bu, aynı dizüstünde llama.cpp'nin çok gerisinde. vLLM bunun yerine CUDA'yı hedefliyor.
- **Sunucu kipi.** Harness'ı kendi (çoğunlukla ekransız Linux) sunucunuzda çalıştırıp her şeyi tarayıcıdan yönetmek:
  - `harness serve` ile sunulan, parola veya token oturumlu bir web arayüzü.
  - Sunucu Yönetimi alanı: alan adı ve HTTPS, erişim ve portlar, kullanıcılar ve tokenlar, servis durumu, güncellemeler, günlükler, yedekleme ve geri yükleme.
  - Arayüzden otomatik SSL (Let's Encrypt; DNS ve port denetimleriyle).
  - Tek satırlık kurulum betiği ve systemd servisi olan bir Linux sunucu paketi (tar.gz, .deb).
  - CUDA, ROCm, Vulkan ve CPU için Docker imajları.
- **Harness Link.** Uzak bir Harness'ı (adres ve token) ekleyip modellerini kütüphanenizde, Playground'da ve gateway'de kullanmak.
- **Karar modelleri.** Evet/hayır, seçim ve puanlama soruları için kısıtlı çıktı ve olasılıklar veren `/v1/decisions` ve `/v1/systemone` uç noktaları.
- **Control API SDK'ları.** OpenAPI şemasından üretilen TypeScript ve Python istemcileri.
- Apple Silicon için **MLX motoru**: sohbet (mlx-lm), görsel sohbet (mlx-vlm) metinden konuşma (mlx-audio, Qwen3-TTS) ve konuşma tanıma (mlx-audio Whisper ve Parakeet) main'de, sonraki sürümle gelir.
- **Her platformda otomatik güncelleme.** İmzalı uygulama içi güncelleyici 0.1.0'da Windows için, 0.1.1'den itibaren macOS için geliyor; Linux paketleri ardından.
- **İmzalı Windows kurulum paketi.** Kod imzalı Windows kurulum paketi (macOS uygulaması zaten imzalı ve notarize edilmiş).

## Daha sonra

- **Araç ve MCP kayıt defteri.** Araçların ve MCP sunucularının topluluk dizini (`struklabs/harness-registry`): izinleri, yayıncıyı, kaynağı, sürümü ve SHA-256'yı içeren manifestler; CI doğrulaması; hash doğrulamasıyla sandbox'a kurulum; uygulamanın içinden yayımlama; doğrulanmış ve topluluk etiketleri.
- Zaten çalıştırdığınız bir ComfyUI örneğine **ComfyUI köprüsü**.
- Çok sayıda araç kuruluyken her istek için ilgili araçları seçen **araç yönlendirici** (embedding ve rerank altyapısıyla).
- Görsel, kod ve yazı üretimi ile ses model motorları için **daha fazla Oluştur sayfası**.
- Yerel modellerle çalışan **uygulama içi kodlama çalışma alanı**.
- **Kararsız:** OpenVINO ve MLC.

## Planlanmıyor

- Ollama kayıt defterinden çekme (`hf.co/...` başvuruları desteklenir).
- Aynı modelleri yerel bir motor zaten çalıştırırken normal kullanım için Python gerektiren motorlar (text-embeddings-inference, faster-whisper). Python motorları isteğe bağlı eklenti olarak kalır, temel uygulama için hiçbir zaman gerekmez.
