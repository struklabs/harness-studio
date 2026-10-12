# Yol Haritası

[English](../en/ROADMAP.md)

Bu, Harness Studio'nun herkese açık yol haritasıdır. Söz değil yön gösterir: öncelikler geri bildirimle değişir. Bir şey önermek veya tartışmak için [GitHub Discussions](https://github.com/struklabs/harness-studio/discussions) üzerinde konu ya da [issue](https://github.com/struklabs/harness-studio/issues) açın. Yayımlanan değişiklikler [değişiklik günlüğünde](../CHANGELOG.md); Harness'ın benzer araçlarla karşılaştırması [FEATURES.md](FEATURES.md) ve [COMPARISON.md](COMPARISON.md) içindedir.

**Ürün hedefi.** Metin, görsel, konuşma, embedding, rerank ve araç iş yüklerini tek yerden yöneten yerel bir yapay zekâ kontrol merkezi. Varsayılan çalışma zamanı llama.cpp'dir; diğerleri ihtiyaç duyduğunuzda kurduğunuz modüllerdir.

**İlkeler.**
- LM Studio ve Ollama'nın yaptığını, llama.cpp'nin gelişmiş denetimlerini gizlemeden yapmak.
- Gözlemlenebilirlikte ve ajan kullanımında daha iyi olmak, düşük kaynak kullanmak.
- Önce yerel, bulut isteğe bağlı.
- Uygulama, CLI, Control API ve MCP aynı Rust servislerini kullanır.

## Şimdi

- **Platform doğrulaması.** Windows kurulum paketi (NSIS) 0.1.0 ile gelir. Linux ve macOS derlemelerini AMD, NVIDIA, Intel ve Apple Silicon donanımlı gerçek makinelerde doğrulayıp yayımlamak.
- **İlk çalıştırma cilası.** Her platformda boş veri klasörleriyle uçtan uca ilk çalıştırma akışı (sihirbaz, model bulma, indirme, başlatma, sohbet).
- **Daha fazla donanımda ölçüm.** Karşılaştırma paketini (`bench/compare/`) NVIDIA/CUDA ve Linux üzerinde çalıştırmak, diğer araçları aynı oturumda yeniden ölçmek.
- **Erişilebilirlik denetimleri.** Küçük pencereler, yüksek arayüz ölçeği ve Windows zorunlu renkler kipi.
- **Linux masaüstü ayrıntıları.** WebKitGTK'de yerel `<select>` görünümü, tepsi simgesi tıklama davranışı ve gerçek donanımda GPU bellek raporlama (NVML, fdinfo).

## Sıradaki

- **Sunucu kipi.** Harness'ı kendi (çoğunlukla ekransız Linux) sunucunuzda çalıştırıp her şeyi tarayıcıdan yönetmek:
  - `harness serve` ile sunulan, parola veya token oturumlu bir web arayüzü.
  - Sunucu Yönetimi alanı: alan adı ve HTTPS, erişim ve portlar, kullanıcılar ve tokenlar, servis durumu, güncellemeler, günlükler, yedekleme ve geri yükleme.
  - Arayüzden otomatik SSL (Let's Encrypt; DNS ve port denetimleriyle).
  - Tek satırlık kurulum betiği ve systemd servisi olan bir Linux sunucu paketi (tar.gz, .deb).
  - CUDA, ROCm, Vulkan ve CPU için Docker imajları.
- **Harness Link.** Uzak bir Harness'ı (adres ve token) ekleyip modellerini kütüphanenizde, Playground'da ve gateway'de kullanmak.
- **Karar modelleri.** Evet/hayır, seçim ve puanlama soruları için kısıtlı çıktı ve olasılıklar veren `/v1/decisions` ve `/v1/systemone` uç noktaları.
- **Control API SDK'ları.** OpenAPI şemasından üretilen TypeScript ve Python istemcileri.
- Apple Silicon için **MLX motoru**.
- **Otomatik güncelleme.** Masaüstü uygulaması için GitHub Releases üzerinden imzalı güncelleme kanalı.
- **İmzalı kurulum paketleri.** Kod imzalı Windows kurulum paketi ve Apple tarafından doğrulanmış (notarize edilmiş) macOS uygulaması.

## Daha sonra

- **Araç ve MCP kayıt defteri.** Araçların ve MCP sunucularının topluluk dizini (`struklabs/harness-registry`): izinleri, yayıncıyı, kaynağı, sürümü ve SHA-256'yı içeren manifestler; CI doğrulaması; hash doğrulamasıyla sandbox'a kurulum; uygulamanın içinden yayımlama; doğrulanmış ve topluluk etiketleri.
- Zaten çalıştırdığınız bir ComfyUI örneğine **ComfyUI köprüsü**.
- Çok sayıda araç kuruluyken her istek için ilgili araçları seçen **araç yönlendirici** (embedding ve rerank altyapısıyla).
- Görsel, kod ve yazı üretimi ile ses çalışma zamanları için **daha fazla Oluştur sayfası**.
- Yerel modellerle çalışan **uygulama içi kodlama çalışma alanı**.
- **Kararsız:** araştırma modelleri (3D, müzik) için bir PyTorch işçisi, OpenVINO ve MLC. Python alt süreçleri için ağ yalıtımı önce tasarlanır.

## Planlanmıyor

- Ollama kayıt defterinden çekme (`hf.co/...` başvuruları desteklenir).
- Windows'ta çalışmayan ya da normal kullanım için çalışma zamanında Python gerektiren motorlar (vLLM, SGLang, text-embeddings-inference, faster-whisper).
