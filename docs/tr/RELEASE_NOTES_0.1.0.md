# Harness Studio 0.1.0

[English](../en/RELEASE_NOTES_0.1.0.md)

Harness Studio, yerel yapay zekâ modellerini bulmak, çalıştırmak ve kullandığınız uygulamalara bağlamak için bir kontrol merkezidir.

### Öne çıkanlar

- Yerel model sunucularını başlatın, durdurun; birden fazla sunucuyu ve kayıtlı çalıştırma ayarlarını yönetin.
- **Model Hub** ile Hugging Face modellerini keşfedin, indirmeleri duraklatıp sürdürün; **Kütüphanem** ile modellerinizi düzenleyin ve Ollama/LM Studio'dan içe aktarın.
- **Playground**'da sohbet edin, iki modeli karşılaştırın, embedding ve rerank deneyin; whisper.cpp ile konuşmayı yazıya dökün.
- PDF, DOCX, metin ve kod belgeleriyle yerel arama ve kaynak gösteren yanıtlar alın; uzun sohbetleri geri alınabilir biçimde özetleyin.
- İsteğe bağlı **gateway** üzerinden tek adreste birden fazla modele erişin; OpenAI, Anthropic ve Ollama uyumlu istemciler bağlayın.
- Dosya, arama ve diğer yerleşik araçları izin denetimiyle kullanın; MCP sunucularına bağlanın ve kendi araçlarınızı oluşturun.
- **Claude Code, Codex, OpenCode** ve diğer istemcileri bağlayın; kalıcı ayar değişikliklerini uygulamadan önce önizleyin.
- Performans ölçümleri, karşılaştırmalar, **analitik ve istek günlüğü** ile hız, token kullanımı ve hataları inceleyin.
- **CLI ve Control API** ile modelleri ve sunucuları yönetin; arayüz olmadan çalıştırın ve Harness'ı MCP üzerinden kullanın.
- Bellek ihtiyacı tahminlerini ve bellek yetersizliği durumunda kurtarma önerilerini görün; gateway gerektiğinde boşta kalan modelleri bellekten çıkarır.
- **Türkçe/İngilizce arayüz**, açık/koyu/sistem temaları, yüksek kontrast ve arayüz boyutlandırması kullanın.

### Sistem gereksinimleri

| Platform | Donanım ve paketler |
| --- | --- |
| Windows 10/11 x64 | AMD GPU'larda Vulkan; uyumlu donanımda gerekli HIP kitaplıklarıyla ROCm/HIP. NVIDIA GPU'larda CUDA. GPU olmadan CPU ile kullanım. NSIS kurulum paketi (GitHub Releases). |
| macOS 13.3 veya üstü | Apple Silicon (Metal) ve Intel Mac'ler. İmzalı ve Apple onaylı (notarize) DMG (GitHub Releases): Apple Silicon için `aarch64`, Intel için `x64`. ONNX Runtime modelleri Intel Mac'lerde kullanılamaz. |
| Linux x86_64 (önizleme) | GitHub Releases'ta `.deb` ve AppImage. AMD ve Intel GPU'larda Vulkan, GPU olmadan CPU. Şimdilik tek bir Ubuntu 24.04 makinede test edildi; sorunları lütfen bildirin. |

GPU hızlandırması donanım, sürücüler ve seçilen model çalıştırıcısına bağlıdır. Kullanılabilir RAM/ekran kartı belleğine sığan bir model seçin; model dosyaları için yeterli disk alanı ayırın.

### Bilinen sınırlamalar

- **Windows kurulumu imzasızdır.** SmartScreen uyarısı çıkarsa **Ek bilgi > Yine de çalıştır** yolunu kullanın.
- **Otomatik uygulama güncellemesi:** güncelleme denetimi GitHub Releases üzerindeki imzalı `latest.json` dosyasını kullanır. İlk sürümde güncelleme bulunmaz; yeni sürümleri [Releases](https://github.com/struklabs/harness-studio/releases) sayfasından da indirebilirsiniz.
- **Windows kaldırma işlemi:** `HKCU\Software\harnessstudio` altında kalan kayıt yalnız kurulum dizinini hatırlar; zararsızdır, istenirse silinebilir. `%APPDATA%\com.harnessstudio.app` içindeki kullanıcı verileri kaldırmada silinmez.
- **Eski sürüme dönüş:** Harness Studio'yu kapatın ve eski sürümü kurmadan önce veri klasöründeki `config/settings.json.bak-v2` dosyasını `config/settings.json` üzerine kopyalayın. Bu, yeni ayar biçimine geçişten sonra değiştirdiğiniz ayarları kaybettirir.

### Veri ve gizlilik

Ayarlar, sohbetler, belgelerin arama kayıtları ve günlükler yerelde saklanır. Varsayılan veri klasörleri:

| Platform | Konum |
| --- | --- |
| Windows | `%APPDATA%\com.harnessstudio.app` |
| macOS | `~/Library/Application Support/com.harnessstudio.app` |
| Linux | `$XDG_DATA_HOME/com.harnessstudio.app`; tanımlı değilse `~/.local/share/com.harnessstudio.app` |

- Sunucular varsayılan olarak yalnız `127.0.0.1` üzerinden erişilir. Masaüstünde gateway ve Control API başlangıçta kapalıdır; ağdan erişim ayrıca açılır.
- Otomatik telemetri veya çökme raporu gönderilmez. Model keşfi/indirme Hugging Face'e; model çalıştırıcısı indirme/güncellemeleri GitHub'a bağlanır. Web, hava durumu ve uzak MCP araçları kullanıldığında ilgili hizmetlere erişilir. **OpenTelemetry dışa aktarımı isteğe bağlıdır.**
- Hugging Face tokenları, model sunucusu API anahtarları ve arama sağlayıcısı anahtarları işletim sistemi anahtarlığında tutulur. Üçüncü taraf istemci ayarları ve yedekleri düz metin anahtar içerebilir; önizlemeyi kontrol edin.
- Yerel veritabanları şifreli değildir. İstek analitiği varsayılan olarak içerik yerine kullanım bilgilerini kaydeder; sohbetler ve kayıtlı yanıtlar yerelde içerik barındırır.
