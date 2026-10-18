# Harness Studio 0.1.2

[English](../en/RELEASE_NOTES_0.1.2.md)

Bu sürüm Playground'u zenginleştiriyor, donanımınıza sığan modelleri bulmayı kolaylaştırıyor ve günlük kullanımdaki denetimleri sadeleştiriyor.

### Öne Çıkanlar

- **Playground'da üretken arayüz:** araç sonuçları, tablolar, grafikler ve basit arayüz blokları zengin bileşenlerle gösteriliyor.
- **Model keşfi:** Donanımıma Uygun, modelleri bilgisayarınıza ne kadar sığdıklarına göre sıralıyor ve sığma etiketi gösteriyor. Model Hub sıralaması artık Hugging Face ile aynı.
- **Basit ve Gelişmiş modlar:** Basit mod temel işleri sade etiketlerle gösteriyor; Gelişmiş mod tüm seçenekleri daha anlaşılır gruplarda sunuyor.
- **Günlük kullanım:** Dashboard, Sunucular, Model Hub, Kütüphanem, Ayarlar, Bağlantılar, Oluştur, Performans ve Analitik düzenleri iyileştirildi. Bölüm menüsü seçimden sonra açık kalıyor; Daralt ile kapatılıyor.
- **Araç denetimleri:** araçlara Tüm İnternet erişimi verin veya erişimi izin verilen alan adlarıyla sınırlayın; yerel ve özel ağ adresleri engelli kalıyor. Günlüğü Temizle, Otomatik Araç Oluşturucu'nun işlem günlüğünü temizliyor.
- **Daha güvenli indirmeler:** arşiv açılırken sembolik bağlantılar ve hedef klasör dışındaki yollar reddediliyor, ayrıcalıklı izin bitleri kaldırılıyor.

### Platformlar ve Bilinen Sınırlamalar

Windows 10/11 x64, macOS 13.3 veya üstü (Apple Silicon ve Intel), Linux x86_64 (önizleme). Linux indirmeleri AppImage ve `.deb` içeriyor; daha kapsamlı Linux doğrulaması ve güncelleyici çalışmaları [yol haritasında](ROADMAP.md). Windows kurulum paketi imzasızdır. ONNX Runtime modelleri Intel Mac'lerde kullanılamaz. LFM2.5-Audio ses çıkışı upstream llama.cpp desteğini bekliyor. llama.cpp üzerinde Qwen3-TTS ses klonlamaz; geçici GPU bellek kullanımı henüz ağ geçidinin model zamanlama bütçesine dahil değildir.

[GitHub Releases](https://github.com/struklabs/harness-studio/releases) üzerinden indirin. [Kurulum kılavuzuna](../guide/tr/01-kurulum.md), [veri ve gizlilik bilgilerine](RELEASE_NOTES_0.1.0.md#veri-ve-gizlilik) ve [tam değişiklik günlüğüne](../CHANGELOG.md) bakın.
