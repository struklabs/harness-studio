# Harness Studio Kullanım Kılavuzu

English: [../en/README.md](../en/README.md)

**Bu sayfada ne yapılır:** Doğru bölümü bulursunuz ve yeni bir kurulumdan ilk yerel sohbete götüren beş adımı izlersiniz.

Harness Studio, struklabs tarafından geliştirilen açık kaynaklı bir projedir. Yapay zekâ modellerini kendi bilgisayarınıza indirmenizi, çalıştırmanızı ve zaten kullandığınız araçlara bağlamanızı sağlar. İlk sohbetiniz için terminal kullanmanız gerekmez. Modeli çalıştıran motor llama.cpp, sohbet ekranı ise Playground'dur.

**Kalın** yazılan adlar uygulamada göreceğiniz etiketlerdir. **Model Hub** ve motor indirmeleri internet bağlantısı gerektirir. Bir model ve bir motor bilgisayarınıza indikten sonra sohbet tamamen sizin makinenizde gerçekleşir.

## İçindekiler

1. [Kurulum ve İlk Açılış](01-kurulum.md)
2. [Model Hub ve Kütüphanem](02-model-hub-ve-kutuphane.md)
3. [Dashboard ve Sunucular](03-dashboard-ve-sunucular.md)
4. [Playground](04-playground.md)
5. [Bağlantılar ve Ağ Geçidi](05-baglantilar-ve-ag-gecidi.md)
6. [Araçlar](06-araclar.md)
7. [Performans ve Analitik](07-performans-ve-analitik.md)
8. [Ayarlar](08-ayarlar.md)
9. [CLI, Control API ve MCP](09-cli-api-mcp.md)
10. [Sorun Giderme ve SSS](10-sorun-giderme.md)

## Beş Adımda Başlayın

Beş dakika, adımları tanımak için bir hedeftir. Motor ve model indirmeleri, boyutlarının ve bağlantınızın gerektirdiği kadar sürer.

1. **İndirin ve kurun.** [GitHub Releases](https://github.com/struklabs/harness-studio/releases) sayfasından Windows `.exe` paketini, Apple Silicon veya Intel için imzalı ve Apple noter onaylı DMG paketini (macOS 13.3+) ya da Linux `.deb` / AppImage önizlemesini edinin. Windows SmartScreen uyarısının adımları [Kurulum](01-kurulum.md) sayfasındadır.
2. **Kurulum sihirbazını tamamlayın.** **Kurulum sihirbazı** içinde **Donanım**, **Depolama**, **Motor**, **Amaç** ve **Gizlilik** adımlarını izleyin. Yönetilen motor için **En Son Sürümü İndir ve Kur** seçeneğini kullanın. **Amaç** adımında arayüz düzeyinizi de seçersiniz (ilk kullanımda **Basit** önerilir). Sonunda **Kurulumu Bitir** düğmesine basın.
3. **Bir model indirin.** **Model Bul** (Gelişmiş düzeyde **Model Hub**) içinde bir sohbet modeli arayın ve **Dosyalar ve kuantlar** bölümünü açın. **Donanım uyumu** sütununu okuyup bir dosyanın **İndir** düğmesine basın. **İndirme Kuyruğu** içinde **Tamamlandı** durumunu bekleyin.
4. **Sunucuyu başlatın.** **Dashboard** içinde indirdiğiniz modeli (varsa bir **Model tanımı** ile birlikte) seçin. Bellek tahminini kontrol edip **Sunucuyu Başlat** düğmesine basın. **Sunucu aktif** durumunu bekleyin.
5. **Sohbet edin.** **Sohbet ve Medya** (Gelişmiş düzeyde **Playground**) içinde **Sohbet** bölümünü açın, modeli kontrol edin, **Mesajınız** alanına "Bana bugün için üç maddelik bir plan yaz." yazıp **Gönder** düğmesine basın.

Model yüklenmezse [bellek kurtarma adımlarına](03-dashboard-ve-sunucular.md#bellek-yetmediğinde-oom) ve [sorun giderme bölümüne](10-sorun-giderme.md) geçin.

## Temel Kavramlar

| Terim | Anlamı |
| --- | --- |
| Model | Yapay zekânın öğrendiklerini taşıyan dosya. Buradaki modellerin çoğu GGUF dosyasıdır, ancak Harness Studio isteğe bağlı motorlarla başka biçimleri de (ONNX, Whisper, difüzyon modelleri vb.) çalıştırır. |
| Kuant | Model sayısal değerlerinin daha derli toplu saklanarak dosyanın daha az yer kaplaması. Daha küçük bir kuant (Q4) kaliteden küçük bir ödünle daha az bellek kullanır. |
| Model motoru (runtime) | Modeli gerçekten çalıştıran program. Sohbet için llama.cpp'dir. Harness Studio onu sizin için indirir ve günceller. İsteğe bağlı motorlar konuşma, görsel üretimi, ONNX ve daha fazlasını ekler. |
| Sunucu | Bir modelin yüklü olduğu ve istek beklediği çalışan süreç. Dashboard'dan başlatılır. |
| Ağ geçidi (gateway) | Birkaç modeli tek adresten sunan ve gerektiğinde yükleyen isteğe bağlı katman. Varsayılan olarak kapalıdır. |
| Bağlam | Modelin aynı anda aklında tutabildiği metin miktarı (token cinsinden). Daha büyük bağlam daha çok bellek ister. |
| Takma ad (alias) | Başka programların dosya adı yerine kullandığı model adı. Model tanımında belirlenir. |
| Arayüz düzeyi | **Basit** temel iş akışını, **Gelişmiş** her teknik ayarı gösterir. Düzey değiştirmek kayıtlı hiçbir değeri değiştirmez. |

## Basit ve Gelişmiş Arayüz Düzeyleri

Harness Studio'nun tek bir arayüzü ve iki ayrıntı düzeyi vardır. **Basit**, bir model bulmak, indirmek, sohbet etmek ve bir uygulamayı bağlamak için gereken her şeyi gösterir. **Gelişmiş** teknik sayfaları ekler: **Sunucular** sayfası, **Oluştur**, **Performans**, **Analitik**, çoğu Dashboard ve Playground ayar sayfası ve çoğu Ayarlar bölümü.

İstediğiniz an ana gezinmenin altındaki **Basit / Gelişmiş** anahtarıyla, **Ayarlar > Görünüm ve Başlangıç** içindeki **Arayüz Düzeyi** ile ya da komut paletinden (**Gelişmiş Düzeye Geç**) geçiş yapabilirsiniz. Geçiş yalnızca nelerin görüneceğini değiştirir; hiçbir değeri değiştirmez veya sıfırlamaz. **Basit** düzeydeyken bir bağlantı, bir hata mesajı ya da komut paleti sizi Gelişmiş bir sayfaya götürürse sayfa normal şekilde açılır ve üstte kısa bir uyarı görünür ("Bu sayfa Gelişmiş düzeyde.").

Bu kılavuz, gerektiğinde Gelişmiş düzeydeki sayfaları işaretler. Burada anlatılan bir sayfayı bulamıyorsanız **Gelişmiş** düzeye geçin.

Basit gezinmede **Model Bul** (Gelişmiş: **Model Hub**), **Sohbet ve Medya** (**Playground**) ve **Uygulama Bağla** (**Bağlantılar**) adları kullanılır. Dashboard’da **Özet**, **Model Seç** ve **Sohbet Uzunluğu** bulunur; son sayfada yalnız bağlam boyutu gösterilir. Teknik kontroller için **Gelişmiş Ayarlar** kullanılır. Dar pencerede yüzen bölüm menüsü seçimden sonra açık kalır; **Daralt** veya Escape ile kapatın. Ana gezinme çekmecesi ise gezinmeden sonra kapanır.

## Bu Kılavuz Yolları Nasıl Yazar?

Kalın yazılan ad, uygulamadaki bir düğme, alan veya sayfa adıdır. Ayarlar yolları kenar çubuğunun sırasını izler. Örneğin **Ayarlar > Depolama > Hugging Face Hesabı**, şu demektir: **Ayarlar**'ı açın, sonra soldaki listede **Depolama** başlığı altındaki **Hugging Face Hesabı** sayfasını seçin.

Kılavuz Türkçe arayüzü esas alır. Uygulama İngilizce de çalışır; dili **Ayarlar > Görünüm ve Başlangıç** içinden değiştirebilirsiniz.

## Daha Ayrıntılı Başvurular

[Özellikler](../../tr/FEATURES.md), [CLI](../../tr/CLI.md), [MCP](../../tr/MCP.md), [Entegrasyonlar](../../tr/INTEGRATIONS.md), [Güvenlik modeli](../../tr/SECURITY_MODEL.md), [Araçlar](../../tr/TOOLS.md), [Features](../../tr/FEATURES.md) ve [Features](../../tr/FEATURES.md).
