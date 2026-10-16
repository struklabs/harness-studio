# Harness Studio 0.1.1

[English](../en/RELEASE_NOTES_0.1.1.md)

Bu sürüm yerel görsel ve konuşma desteğini genişletiyor; günlük kullanımda güvenilirliği ve erişilebilirliği iyileştiriyor.

### Öne Çıkanlar

- **Yeni motorlar:** transcribe.cpp ile konuşma tanıma; llama.cpp Qwen3-TTS ile modelin kendi sesinden konuşma; Apple Silicon'da mflux FLUX.2 [klein] ile görsel üretme ve düzenleme; MLX CSM ile iki sesli konuşma.
- **Harness Link:** başka bir Harness örneğine bağlanıp modellerini Playground'da, Kütüphanem'de ve yerel ağ geçidinde kullanın.
- **Ses ve model kapsamı:** Ses İşleme, konuşmacı ayrımını zaman çizelgesi ve tablo olarak gösteriyor, JSON dışa aktarımı sunuyor. Daha fazla ONNX görü ve NLP modeli yerelde çalışıyor; konuşma ve ses işleme desteği genişliyor.
- **Masaüstü iyileştirmeleri:** macOS uygulama içi güncellemeleri; daha erişilebilir klavye ve ekran okuyucu denetimleri; yüksek arayüz ölçeğinde ve küçük pencerelerde okunabilir düzenler ve ulaşılabilir düğmeler.
- **İlk çalıştırma ve kararlılık:** karşılama turu kurulumdan sonra açılıyor, bildirimler sihirbaz düğmelerini kapatmıyor, uzun sohbetler daha hızlı açılıyor; model başlatma, indirme ve bellek yönetimi daha güvenilir çalışıyor.
- **Geliştirici erişimi:** TypeScript SDK npm'de `@struklabs/harness-studio-sdk` adıyla sunuluyor; SDK paketlerinin sürümleri ayrı tutuluyor.

### Platformlar ve Bilinen Sınırlamalar

Windows 10/11 x64, macOS 13.3 veya üstü (Apple Silicon ve Intel), Linux x86_64 (önizleme). Linux indirmeleri AppImage ve `.deb` içeriyor; daha kapsamlı Linux doğrulaması ve güncelleyici çalışmaları [yol haritasında](ROADMAP.md). Windows kurulum paketi imzasızdır. ONNX Runtime modelleri Intel Mac'lerde kullanılamaz. LFM2.5-Audio ses çıkışı upstream llama.cpp desteğini bekliyor. llama.cpp üzerinde Qwen3-TTS ses klonlamaz; geçici GPU bellek kullanımı henüz ağ geçidinin model zamanlama bütçesine dahil değildir.

[GitHub Releases](https://github.com/struklabs/harness-studio/releases) üzerinden indirin. [Kurulum kılavuzuna](../guide/tr/01-kurulum.md), [veri ve gizlilik bilgilerine](RELEASE_NOTES_0.1.0.md#veri-ve-gizlilik) ve [tam değişiklik günlüğüne](../CHANGELOG.md) bakın.
