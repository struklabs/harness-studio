# Yol Haritası

[English](../en/ROADMAP.md)

Harness Studio, yerel metin, görsel, konuşma ve araç iş yüklerini tek yerden yönetir. Öncelik yerel kullanımdadır; ek motorlar ve bulut bağlantıları isteğe bağlıdır. Bu yol haritası yön gösterir; öncelikler geri bildirimle değişebilir. Önerilerinizi [Discussions](https://github.com/struklabs/harness-studio/discussions) veya [issue](https://github.com/struklabs/harness-studio/issues) üzerinden paylaşın. Ayrıntılar için [değişiklik günlüğüne](../CHANGELOG.md) bakın.

## 0.1.2 ile Gelenler

- **Playground:** araç sonuçları, tablolar, grafikler ve basit arayüz blokları için üretken arayüz.
- **Model keşfi:** Donanımıma Uygun sıralaması ve sığma etiketleri; Hugging Face ile aynı sıralama.
- **Günlük denetimler:** Basit ve Gelişmiş modlar, iyileştirilmiş sayfa düzenleri, açık kalan bölüm menüsü, araçlar için Tüm İnternet izni ve Otomatik Araç Oluşturucu'da Günlüğü Temizle.
- **Güvenlik:** arşiv açılırken sembolik bağlantılar ve hedef klasör dışındaki yollar reddediliyor, ayrıcalıklı izin bitleri kaldırılıyor.

## 0.1.1 ile Gelenler

- **Motorlar:** transcribe.cpp, llama.cpp Qwen3-TTS, Apple Silicon'da mflux ile görseller, CSM dahil genişleyen MLX konuşma desteği.
- **Model kapsamı:** daha fazla ONNX görü, NLP ve ses görevi; görsel düzenleme, video üretme ve konuşmacı ayrımıyla ses işleme.
- **Bağlantılar:** Harness Link ile uzak Harness modellerini Playground'da ve yerel ağ geçidinde kullanma.
- **Masaüstü deneyimi:** erişilebilirlik ve yüksek arayüz ölçeği düzeltmeleri, ilk çalıştırma iyileştirmeleri, daha hızlı uzun sohbetler ve model kütüphaneleri.
- **Platformlar ve kararlılık:** macOS güncellemeleri, Linux AppImage desteği; bellek baskısı, indirmeler ve model başlatma düzeltmeleri.
- **SDK'lar:** TypeScript SDK npm'de yayımlandı; TypeScript ve Python istemcileri CI'da Control API'ye karşı denetleniyor.

## Sıradaki Adımlar

- Linux'u daha fazla AMD ve NVIDIA makinede ve dağıtımda doğrulayıp önizleme etiketini kaldırmak.
- Linux ilk çalıştırma ve erişilebilirlik kontrollerini tamamlamak.
- Linux'ta benchmark karşılaştırmasını çalıştırmak.
- Linux güncelleyici paketlerini ve doğrulamasını tamamlamak.
- Linux NVIDIA doğrulamasından sonra CUDA vLLM eklemek.
- Python SDK'yı Trusted Publishing ile PyPI'da yayımlamak.
- Web sitesi hazır olduktan sonra Windows kurulum paketini imzalamak.
- Upstream llama.cpp desteklediğinde LFM2.5-Audio ses çıkışını etkinleştirmek.

## 0.2.0 için Planlananlar

- **Sunucu kipi:** `harness serve` ile tarayıcıdan yönetim, oturumlar, HTTPS, kullanıcılar ve token'lar, güncellemeler, günlükler, yedekleme ve geri yükleme; Linux hizmet paketleri ve Docker imajları.
- **Araç, MCP ve Skill Kataloğu:** izin bildirimleri, yayıncı ve sürüm bilgileri, sağlama toplamı doğrulaması, CI denetimi ve uygulama içinden yayımlama sunan topluluk kataloğu.

## Daha Sonra

- ComfyUI köprüsü ve her istek için ilgili araçları seçen araç yönlendiricisi.
- Daha fazla Oluştur sayfası ve yerel modellerle çalışan uygulama içi kodlama alanı.
- Değerlendirilenler: NVIDIA benchmark'ından sonra faster-whisper; SEA-G2P bağımlılığı uygun hâle geldiğinde VieNeu.
- Karar verilmedi: OpenVINO ve MLC.

## Planlanmayanlar

- Ollama kayıt deposundan indirme (`hf.co/...` başvuruları desteklenir).
- Temel uygulama için Python zorunluluğu veya yerel motorların model kapsamını yineleyen Python motorları; ek motorlar isteğe bağlı kalır.
