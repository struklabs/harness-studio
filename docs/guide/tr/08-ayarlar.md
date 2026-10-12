# Ayarlar

English: [../en/08-settings.md](../en/08-settings.md)

**Bu sayfada ne yapılır:** Ayar sayfalarının görevlerini anlar, görünüm ve başlangıç davranışını seçer, pencere tepsiye kapatıldığında olacakları yönetirsiniz.

## Ayarlar Nasıl Çalışır?

Sol menüdeki **Ayarlar** düğmesi sayfayı açar. Soldaki liste bölümlere ve alt sayfalara ayrılmıştır; bu kılavuz yolları **Ayarlar > Bölüm > Sayfa** biçiminde yazar. İlk sayfa **Model Motoru > Kurulu Model Motoru**'dır. **Basit** arayüz düzeyinde yalnız **Kurulu Model Motoru**, **Dizinler**, **Hugging Face Hesabı**, **Görünüm ve Başlangıç**, **Güncellemeler** ve **Gizlilik** listelenir; kalanlar Gelişmiş sayfalardır (bkz. [Basit ve Gelişmiş düzeyler](README.md#basit-ve-gelişmiş-arayüz-düzeyleri)).

- Birçok sayfada yaptığınız değişiklik hemen kaydedilmez. Altta "{sayı} kaydedilmemiş değişiklik" çubuğu çıkar; **Kaydet** ile kaydedin veya **Değişiklikleri At** ile vazgeçin.
- Bazı alanların kendi **Kaydet** veya **Uygula** düğmesi vardır (Hugging Face tokenı, ağ geçidi gibi). Dil, tema ve arayüz boyutu gibi tercihler hemen uygulanır.
- Kaydedilmemiş değişiklik varken pencereyi tepsiye küçültürseniz o girdi kaybolabilir; bkz. [Tepside Sayfa Bırakmak](#tepside-sayfa-bırakmak).
- Komut paletinden (arama) bir ayar adını yazarak doğrudan ilgili sayfaya gidebilirsiniz.

### Ayar Haritası

| Ayar | Yol |
| --- | --- |
| Motor (llama.cpp) kaynağı, sürümü, güncellemesi, kurulum sihirbazı | **Model Motoru > Kurulu Model Motoru** |
| Ek llama.cpp derlemeleri ve diğer motorlar (konuşma, görsel, ONNX, Lemonade, mistral.rs, GGUF dönüştürme) | **Model Motoru > Registry** |
| Ollama uyumlu API | **Model Motoru > Ollama** |
| Model klasörleri | **Depolama > Dizinler** |
| Hugging Face hesabı ve tokenı | **Depolama > Hugging Face Hesabı** |
| Yeni sunucuların varsayılanları, yükleme zaman aşımı, çökünce yeniden başlatma, dinlenen adres | **Ağ > Dinleme** |
| Tarayıcıdan erişim (CORS) | **Ağ > Erişim** |
| Sunucu API anahtarları | **Güvenlik > API Anahtarları** |
| Anahtarsız ağa açma onayları | **Güvenlik > İzinler** |
| Ağ geçidini açma, port, token | **Ağ Geçidi > Uç Nokta** |
| Model yükleme/boşaltma kuralları | **Ağ Geçidi > Yönlendirme** |
| Responses API'nin yanıt saklaması | **Ağ Geçidi > Responses** |
| Araç izin politikası, çalışma klasörü, web araması | **Araçlar > Yerleşik Araçlar** |
| Kurulu araç paketleri | **Araçlar > Kurulu Araçlar** |
| Control API ve isimli tokenlar | **Geliştirici > Control API** |
| Komut satırı aracı (CLI) | **Geliştirici > CLI** |
| Metrik ve iz dışa aktarma | **OpenTelemetry** |
| Tema, dil, arayüz düzeyi, başlangıç, tepsi | **Görünüm ve Başlangıç** |
| Uygulama sürümü ve güncelleme denetimi | **Güncellemeler** |
| Yerel tanılama raporlarının içeriği | **Gizlilik** |
| Kurulum denetimi, raporlar | **Tanılama > Sağlık**, **Tanılama > Raporlar** |

## Model Motoru

**Ayarlar > Model Motoru > Kurulu Model Motoru** motor kaynağını düzenler.

- **Yönetilen (önerilen)** seçiliyken **Kurulu sürüm**, **Güncellemeleri Denetle** ve **Otomatik güncelle** görünür. Otomatik güncelleme yeni sürümü arka planda indirip doğrular ve sunucular durduğunda etkinleştirir. Yeni sürüm elle de indirilip test edilir ve siz **Yeni Sürümü Etkinleştir** diyene kadar eski sürüm çalışır. Bir güncelleme sorun çıkarırsa yedek varsa **Önceki sürüme dön** kullanılabilir.
- **Özel ikili** seçiliyken **llama-server yolu** yazılır veya **Otomatik Bul** ile aranır, **Doğrula** ile denenir. Özel ikili otomatik güncellenmez ve geri alınmaz.
- **Backend** satırı hangi derlemenin (CUDA, Vulkan, ROCm, Metal, CPU) kurulu ve donanımınız için önerildiğini gösterir; kurulu derleme ekran kartını göremiyorsa başka derleme kurabilirsiniz.
- **Kurulum sihirbazı > Sihirbazı Aç** ilk açılış sihirbazını yeniden çalıştırır.

**Registry** (Gelişmiş bir sayfa) çatallar ve kendi derlemeleriniz gibi ek llama.cpp model motorlarını (her model tanımı veya sunucu profili kendi motorunu seçebilir) ve diğer model türleri için isteğe bağlı motorları yönetir:

| Kart | Ne işe yarar? |
| --- | --- |
| Konuşma Tanıma (whisper.cpp) | Ses dosyalarını metne çeviren `whisper-server`. |
| Seslendirme (sherpa-onnx) | Kokoro ve Piper ses modelleri; sherpa-onnx ayrıca Parakeet, SenseVoice ve Omnilingual tanıma modellerini de çalıştırır. |
| Görsel Üretimi (stable-diffusion.cpp) | Difüzyon modellerinden (SD1.x, SDXL, SD3.5, FLUX, Qwen-Image) görsel üreten `sd-server`. |
| ONNX Runtime (gömme, yeniden sıralama, sohbet) | `harness-onnx` yardımcısının, Hugging Face ONNX gömme (embedding), yeniden sıralama (rerank) ve sohbet modellerini Python olmadan çalıştırmak için kullandığı kütüphaneler. **DirectML** (Windows: AMD ve NVIDIA), **CUDA** veya **CPU** varyantını kurabilirsiniz; indirmeler SHA-256 ile doğrulanır. ONNX sohbet modelleri (onnxruntime-genai) şimdilik yalnız Windows'ta çalışır ve araç çağrısı desteklemez. |
| Lemonade (Ryzen AI NPU) | İsteğe bağlı. Modelleri AMD Ryzen AI NPU'larında (FastFlowLM) ve Lemonade'in kendi ROCm ve Vulkan llama.cpp derlemeleriyle çalıştırır; ağ geçidi onun sohbet, gömme, yeniden sıralama ve konuşmayı metne çevirme modellerini sunar. |
| Safetensors Modelleri (mistral.rs) | İsteğe bağlı. Hugging Face safetensors sohbet ve görüntü modellerini GGUF'a dönüştürmeden çalıştırır. Kendi mistral.rs derlemenizi eklersiniz. |
| GGUF Dönüştürme Ortamı | Safetensors modellerini GGUF'a dönüştürürken kullanılan Python ortamı; buradan kurulur veya silinir. |

**Ollama** sayfası, Ollama istemcilerinin ağ geçidine Ollama'ymış gibi bağlanmasını düzenler ([Bağlantılar](05-baglantilar-ve-ag-gecidi.md)).

## Depolama

**Dizinler** birincil model klasörünü (yeni indirmelerin hedefi) ve ek klasörleri yönetir; ayrıntı [Kurulum](01-kurulum.md#model-klasörü-ve-veri-dizini) sayfasındadır. **Hugging Face Hesabı** erişim tokenı veya tarayıcı girişi içindir; adımlar [Model Hub sayfasındadır](02-model-hub-ve-kutuphane.md#hugging-face-hesabı-i̇ki-yol).

## Ağ

**Ağ > Dinleme** sayfasında üç grup bulunur:

| Grup | Alanlar | Anlamı |
| --- | --- | --- |
| **Sunucu Varsayılanları** | **Port**, **GPU katmanları (-ngl)**, **Bağlam boyutu (-c)** | Yeni oluşturulan sunucuların başlangıç değerleri. Port `0` (Otomatik) 11440–11499 aralığından boş port seçer. Daha önce başlatılmış sunucu kendi kayıtlı değerlerini kullanır. |
| **Sunucu Başlatma** | **Yükleme zaman aşımı (sn)**, **Çökünce otomatik yeniden başlat**, **En fazla deneme** | Sunucu bu sürede hazır olmazsa başlatma hatayla biter (büyük modelde artırın). Çalışırken beklenmedik duran sunucu 2, 4, 8... sn beklemelerle yeniden başlatılır; yükleme hataları (bellek, bozuk dosya, port) yeniden denenmez. |
| **Varsayılan Bağlanma Adresi** | **Yalnız bu bilgisayar**, **Yerel ağ**, **Özel adres** | `127.0.0.1` yalnız bu bilgisayara açıktır (önerilen). **Yerel ağ** (`0.0.0.0`) aynı ağdaki cihazların bağlanmasını sağlar ve API anahtarı gerektirir; anahtarsız açmak istenirse uygulama bir **Yine de Aç** onayı ister (bu onaylar **Güvenlik > İzinler** sayfasında listelenir). |

**Ağ > Erişim** sayfası, tarayıcıda çalışan web uygulamalarının sunuculara erişimini (**İzinli Tarayıcı Origin'leri (CORS)**) belirler. Masaüstü istemciler ve komut satırı araçları bundan etkilenmez. Harness Studio güvenlik duvarınızı kendiliğinden değiştirmez; Windows Güvenlik Duvarı ilk başlatmada izin isteyebilir, yalnız "Özel ağlar" için izin vermeniz önerilir.

## Güvenlik

**API Anahtarları** sayfasında her sunucu için anahtar durumu görünür (**Anahtar var** / **Anahtar yok**); **Güçlü Anahtar Üret**, **Yeni Anahtar Üret** ve **Anahtarı Kopyala** düğmeleri vardır. Anahtarlar işletim sistemi anahtarlığında tutulur, dosyalara yazılmaz. Yeni anahtar sunucu yeniden başlatılınca geçerli olur ve eski anahtarı kullanan istemcilerin ayarını da güncellemeniz gerekir. **İzinler** sayfası, sunucuları anahtarsız ağa açmak için verdiğiniz **Yine de Aç** onaylarını listeler ve geri almanıza izin verir. Ayrıntılı koşullar [güvenlik modelinde](../../tr/SECURITY_MODEL.md) anlatılır.

## Ağ Geçidi

**Uç Nokta** ağ geçidini açar ve adresini düzenler, **Yönlendirme** model yükleme/boşaltma ve kuyruk kurallarını, **Responses** OpenAI Responses API'sinin saklama seçeneklerini içerir. Ağ geçidi varsayılan kapalıdır ve uygulama kapanınca kapanır. Ayrıntı ve istemci bağlama için [Bağlantılar bölümüne](05-baglantilar-ve-ag-gecidi.md) bakın.

## Araçlar

**Yerleşik Araçlar** izin politikasını, güvenilir araçları, varsayılan çalışma klasörünü, web araması sağlayıcısını ve **Araçları MCP istemcilerine sun** seçeneğini içerir. **Kurulu Araçlar** hazır veya üretilmiş paketleri yönetir. Ayrıntı [Araçlar bölümündedir](06-araclar.md).

## Geliştirici

**Control API** programların Harness Studio'yu yönetmesine izin veren API'yi ve isimli tokenları içerir; günlük sohbet için açmanız gerekmez. **CLI** sayfasındaki **Terminalde Kullan** alanı komut satırı aracını gösterir; **PATH'e Ekle** terminalden erişimi sağlar. Zaten açık olan terminaller değişikliği görmez; yeni bir terminal açın. Ayrıntı için [CLI, Control API ve MCP](09-cli-api-mcp.md) bölümüne bakın.

## OpenTelemetry

**OpenTelemetry** metrik ve izleri seçtiğiniz toplayıcıya aktarmak içindir; yalnız gözlem altyapısı işleten kişilere yöneliktir ve varsayılan olarak kapalıdır. **Dışa aktarmayı aç** seçilmedikçe uygulama kendiliğinden toplayıcıya bağlanmaz ve veri göndermez. Tek istisna **Deneme Gönder** düğmesidir: dışa aktarma kapalı olsa bile formdaki adrese gerçek bir deneme span'ı gönderir. Dışa aktarma açıldığında veriler yazdığınız toplayıcı adresine gider. Derlemede gRPC desteği yoksa **HTTP/protobuf (port 4318)** protokolünü seçin.

## Görünüm ve Başlangıç

| Ayar | Ne yapar? |
| --- | --- |
| Tema | **Koyu**, **Açık**, **Sistem**, **OLED Siyah**, **Gece**, **Yüksek Kontrast Koyu**, **Yüksek Kontrast Açık**. |
| Dil | Türkçe veya İngilizce; değişiklik hemen uygulanır. |
| **Arayüz düzeyi** | **Basit** veya **Gelişmiş**; bkz. [Basit ve Gelişmiş düzeyler](README.md#basit-ve-gelişmiş-arayüz-düzeyleri). Yalnız nelerin görüneceğini değiştirir, kayıtlı hiçbir değeri değiştirmez. |
| **Arayüz boyutu** | Yazı, simge ve boşlukları birlikte ölçekler. Kısayollar: Ctrl+= büyüt, Ctrl+- küçült, Ctrl+0 sıfırla. |
| **Renk körlüğü dostu renkler** | Durum ve grafik renklerini kırmızı/yeşil ayrımına dayanmayan renklerle değiştirir. |
| **Oturum açılınca başlat** | Bilgisayar açılınca Harness Studio başlar. Hemen uygulanır. |
| **Küçültülmüş başlat** | Otomatik açılışta pencere açılmaz, uygulama tepside bekler. Yalnız **Oturum açılınca başlat** açıkken kullanılabilir. |
| **Son oturumu geri yükle** | Açılışta en son başlattığınız sunucu yeniden başlar. |
| **Pencereyi kapatınca** | **Sor**, **Tepsiye Küçült** veya **Uygulamadan Çık**. macOS'ta tepsinin karşılığı **Menü çubuğunda tut**'tur. |
| **Tepsideyken pencereyi bellekte tut** | Tepsideki sayfanın bellekte kalıp kalmayacağı; aşağıda anlatılır. |

## Güncellemeler

**Güncellemeler** sayfası uygulama sürümünü gösterir ve **Güncellemeleri Denetle** düğmesi olan **Uygulama** bölümünü içerir. Daha yeni bir sürüm yayımlandığında **Güncellemeyi Kur**'u kullanabilirsiniz; uygulama güncellemeyi indirip kurar, ardından yeniden başlar ve çalışan tüm sunucular durdurulur. Denetim güncelleme kaynağının bulunmadığını bildirirse, en yeni kurulum paketini projenin [GitHub Releases](https://github.com/struklabs/harness-studio/releases) sayfasından indirip çalıştırın. llama.cpp motoru ayrıca **Model Motoru > Kurulu Model Motoru** sayfasında güncellenir.

## Gizlilik

**Gizlilik** yerel tanılama raporlarının içeriğini seçer: **Çökme raporları**, **Uygulama hataları**, **Sunucu hataları (llama.cpp)**, **Donanım bilgisi**, **Performans metrikleri**. Raporlar yalnız bu cihazda saklanır ve hiçbir yere otomatik gönderilmez. Bu raporlara istemler, model yanıtları, dosya içerikleri ve API istek gövdeleri girmez. Sohbet geçmişi ve isteğe bağlı analitik içerik kaydı ayrı işlevlerdir; [analitik bölümündeki](07-performans-ve-analitik.md#analitik-ve-i̇stek-günlüğü) düz metin içerik kaydı açıklamasını da okuyun.

## Tanılama

**Tanılama > Sağlık** sayfasında **Denetimi Çalıştır** kurulumu, model motoru uyumunu, GPU kütüphanelerini ve portları denetler; sonuçlar **Tamam**, **Uyarı** veya **Hata** olarak işaretlenir ve **JSON'u Kopyala** ile rapora eklenir. **Tanılama > Raporlar** yerel tanılama kayıtlarını inceleme (**İncele**), kopyalama ve silme (**Raporları Sil**) alanıdır. Bir sorun için önce başarısız denetimin açıklamasını okuyun; ardından [sorun giderme bölümüyle](10-sorun-giderme.md) devam edin.

## Tepside Sayfa Bırakmak

**Pencereyi kapatınca** seçimi **Tepsiye Küçült** ise pencereyi kapatmak uygulamayı kapatmaz: uygulama sistem tepsisinde (macOS'ta menü çubuğunda) çalışmaya devam eder, sunucular ve indirmeler sürer. Tepsi simgesinden pencere yeniden açılır; tamamen kapatmak için simgenin menüsündeki **Çıkış** kullanılır. **Uygulamadan Çık** seçiliyse çalışan sunucular durdurulur. Masaüstünüzde tepsi veya menü çubuğu simgesi yoksa pencereyi kapatmak uygulamadan çıkar.

Tepsideyken bellek tasarrufu için pencerenin web sayfası kapatılabilir:

1. **Ayarlar > Görünüm ve Başlangıç** içinde **Pencereyi kapatınca** seçimini **Tepsiye Küçült** yapın.
2. Pencereyi kapatın. Sunucular ve indirmeler uygulama sürecinde çalışmaya devam eder.
3. **Tepsideyken pencereyi bellekte tut** kapalıysa (varsayılan), pencere tepsideyken yaklaşık 30 saniye sonra arayüz sayfası kapatılır ve yalnız uygulama süreci kalır; bu, tepside yaklaşık 60 MiB tasarruf sağlar. Tepsi simgesinden açtığınızda sayfa yeniden kurulur (yaklaşık yarım saniye) ve açık sekmeniz geri gelir.
4. Sayfa şu durumlarda tepsideyken bile kapatılmaz: Playground'da yanıt akıyorsa veya araç çağrısı sürüyorsa, bir indirme çalışıyorsa, bir araç onayı bekleniyorsa, yazılmış ama gönderilmemiş mesaj varsa, Dashboard'da kaydedilmemiş sunucu ayarı veya Ayarlar'da kaydedilmemiş değişiklik varsa, araç oluşturucu veya ses kaydı çalışıyorsa.
5. Anında hazır olmasını istiyorsanız **Tepsideyken pencereyi bellekte tut** seçimini açın. Daha fazla bellek kullanır ama açılış anlıktır.

Yukarıdaki korumalara girmeyen girdiler (örneğin bir arama kutusuna yazılmış ama aranmamış metin) kaybolabilir; pencereyi kapatmadan önce işinizi kaydedin. macOS'ta ilgili tercih **Menü çubuğundayken pencereyi bellekte tut** olarak görünür.

Sonraki: [CLI, Control API ve MCP](09-cli-api-mcp.md).
