# Ayarlar

**Bu sayfada ne yapılır:** Ayar sayfalarının görevleri anlaşılır, görünüm ve başlangıç davranışı seçilir, tepside pencerenin tutulması yönetilir.

## Ayarlar nasıl çalışır?

Sol menüdeki **Ayarlar** düğmesi sayfayı açar. Soldaki liste bölümlere ve alt sayfalara ayrılmıştır; bu kılavuz yolları **Ayarlar > Bölüm > Sayfa** biçiminde yazar. İlk sayfa **Model Motoru > Kurulu Model Motoru**'dır.

- Birçok sayfada yaptığınız değişiklik hemen kaydedilmez. Altta **{sayı} kaydedilmemiş değişiklik** çubuğu çıkar; **Kaydet** ile kaydedin veya **Değişiklikleri At** ile vazgeçin.
- Bazı alanların kendi **Kaydet** veya **Uygula** düğmesi vardır (Hugging Face tokenı, ağ geçidi gibi). Dil, tema ve arayüz boyutu gibi tercihler hemen uygulanır.
- Kaydedilmemiş değişiklik varken pencereyi tepsiye küçültürseniz o girdi kaybolabilir; bkz. [Tepside sayfa bırakmak](#tepside-sayfa-bırakmak).
- Komut paletinden (arama) bir ayar adını yazarak doğrudan ilgili sayfaya gidebilirsiniz.

### Ayar haritası

| Ayar | Yol |
| --- | --- |
| Motor (llama.cpp) kaynağı, sürümü, güncellemesi, kurulum sihirbazı | **Model Motoru > Kurulu Model Motoru** |
| Whisper, görsel üretimi, seslendirme, ONNX ve Lemonade motorlarını tek tıkla kurma | **Model Motoru > Kurulu Model Motoru** (Diğer Model Motorları) |
| Ek llama.cpp derlemeleri, GitHub'dan ya da yerel klasörden motor ekleme | **Model Motoru > Registry** |
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
| Tema, dil, başlangıç, tepsi | **Görünüm ve Başlangıç** |
| Uygulama sürümü ve güncelleme denetimi | **Güncellemeler** |
| Yerel tanılama raporlarının içeriği | **Gizlilik** |
| Kurulum denetimi, raporlar | **Tanılama > Sağlık**, **Tanılama > Raporlar** |

## Model Motoru

**Ayarlar > Model Motoru > Kurulu Model Motoru** motor kaynağını düzenler.

- **Yönetilen (önerilen)** seçiliyken **Kurulu sürüm**, **Güncellemeleri Denetle** ve **Otomatik güncelle** görünür. Otomatik güncelleme yeni sürümü arka planda indirip doğrular ve sunucular durduğunda etkinleştirir. Yeni sürüm elle de indirilip test edilir (**{sürüm} İndir ve Test Et**) ve siz **Yeni Sürümü Etkinleştir** diyene kadar eski sürüm çalışır. Bir güncelleme sorun çıkarırsa yedek varsa **Önceki sürüme dön** kullanılabilir.
- **Özel ikili** seçiliyken **llama-server yolu** yazılır veya **Otomatik Bul** ile aranır, **Doğrula** ile denenir. Özel ikili otomatik güncellenmez ve geri alınmaz.
- **Backend** satırı hangi derlemenin (CUDA, Vulkan, ROCm, Metal, CPU) kurulu ve donanımınız için önerildiğini gösterir; kurulu derleme ekran kartını göremiyorsa başka derleme kurabilirsiniz.
- **Kurulum sihirbazı > Sihirbazı Aç** ilk açılış sihirbazını yeniden çalıştırır.

**Registry** çatallar ve kendi derlemeleriniz gibi ek llama.cpp model motorlarını yönetir. **Ollama** sayfası, Ollama istemcilerinin ağ geçidine Ollama'ymış gibi bağlanmasını düzenler ([Bağlantılar](05-baglantilar-ve-ag-gecidi.md)).

## Depolama

**Dizinler** birincil model klasörünü (yeni indirmelerin hedefi) ve ek klasörleri yönetir; ayrıntı [Kurulum](01-kurulum.md#model-klasörü-ve-veri-dizini) sayfasındadır. **Hugging Face Hesabı** erişim tokenı veya tarayıcı girişi içindir; adımlar [Model Hub sayfasındadır](02-model-hub-ve-kutuphane.md#hugging-face-hesabı-iki-yol).

## Ağ

**Ağ > Dinleme** sayfasında üç grup bulunur:

| Grup | Alanlar | Anlamı |
| --- | --- | --- |
| **Sunucu Varsayılanları** | **Port**, **GPU katmanları (-ngl)**, **Bağlam boyutu (-c)** | Yeni oluşturulan sunucuların başlangıç değerleri. Port `0` (Otomatik) 11440–11499 aralığından boş port seçer. Daha önce başlatılmış sunucu kendi kayıtlı değerlerini kullanır. |
| **Sunucu Başlatma** | **Yükleme zaman aşımı (sn)**, **Çökünce otomatik yeniden başlat**, **En fazla deneme** | Sunucu bu sürede hazır olmazsa başlatma hatayla biter (büyük modelde artırın). Çalışırken beklenmedik duran sunucu 2, 4, 8... sn beklemelerle yeniden başlatılır; yükleme hataları (bellek, bozuk dosya, port) yeniden denenmez. |
| **Varsayılan Bağlanma Adresi** | **Yalnız bu bilgisayar**, **Yerel ağ**, **Özel adres** | `127.0.0.1` yalnız bu bilgisayara açıktır (önerilen). **Yerel ağ** (`0.0.0.0`) aynı ağdaki cihazların bağlanmasını sağlar ve API anahtarı gerektirir; anahtarsız açmak istenirse uygulama bir **Yine de Aç** onayı ister (bu onaylar **Güvenlik > İzinler** sayfasında listelenir). |

**Ağ > Erişim** sayfası, tarayıcıda çalışan web uygulamalarının sunuculara erişimini (**İzinli Tarayıcı Origin'leri (CORS)**) belirler. Masaüstü istemciler ve komut satırı araçları bundan etkilenmez. Harness güvenlik duvarını kendiliğinden değiştirmez; Windows Güvenlik Duvarı ilk başlatmada izin isteyebilir, yalnız "Özel ağlar" için izin vermeniz önerilir.

## Güvenlik

**API Anahtarları** sayfasında her sunucu için anahtar durumu görünür (**Anahtar var** / **Anahtar yok**); **Güçlü Anahtar Üret**, **Yeni Anahtar Üret** ve **Anahtarı Kopyala** düğmeleri vardır. Anahtarlar işletim sistemi anahtarlığında tutulur, dosyalara yazılmaz. Yeni anahtar sunucu yeniden başlatılınca geçerli olur ve eski anahtarı kullanan istemcilerin ayarını da güncellemeniz gerekir. **İzinler** sayfası, sunucuları anahtarsız ağa açmak için verdiğiniz **Yine de Aç** onaylarını listeler ve geri almanıza izin verir. Ayrıntılı koşullar [SECURITY.md](../SECURITY.md) belgesindedir.

## Ağ geçidi

**Uç Nokta** ağ geçidini açar ve adresini düzenler, **Yönlendirme** model yükleme/boşaltma ve kuyruk kurallarını, **Responses** OpenAI Responses API'sinin saklama seçeneklerini içerir. Ağ geçidi varsayılan kapalıdır ve uygulama kapanınca kapanır. Ayrıntı ve istemci bağlama için [Bağlantılar sayfasına](05-baglantilar-ve-ag-gecidi.md) bakın.

## Araçlar

**Yerleşik Araçlar** izin politikasını, güvenilir araçları, varsayılan çalışma klasörünü, web araması sağlayıcısını ve **Araçları MCP istemcilerine sun** seçeneğini içerir. **Kurulu Araçlar** hazır veya üretilmiş paketleri yönetir. Ayrıntı [Araçlar sayfasındadır](06-araclar.md).

## Geliştirici

**Control API** programların Harness'ı yönetmesine izin veren API'yi ve isimli tokenları içerir; günlük sohbet için açmanız gerekmez. **CLI** sayfasındaki **Terminalde Kullan** alanı komut satırı aracını gösterir; **PATH'e Ekle** terminalden erişimi sağlar. Açık terminaller değişikliği görmez; yeni terminal gerekir. Ayrıntı [CLI, API ve MCP sayfasındadır](09-cli-api-mcp.md).

## OpenTelemetry

**OpenTelemetry** metrik ve izleri seçtiğiniz toplayıcıya aktarmak içindir; yalnız gözlem altyapısı kuran kullanıcılar içindir ve varsayılan olarak kapalıdır. **Dışa aktarmayı aç** seçilmedikçe uygulama kendiliğinden toplayıcıya bağlanmaz ve veri göndermez. Tek istisna **Deneme Gönder** düğmesidir: dışa aktarma kapalı olsa bile formdaki adrese gerçek bir deneme span'ı gönderir. Dışa aktarma açıldığında veriler yazdığınız toplayıcı adresine gider. Derlemede gRPC desteği yoksa **HTTP/protobuf (port 4318)** protokolünü seçin.

## Görünüm ve başlangıç

| Ayar | Ne yapar? |
| --- | --- |
| Tema | **Koyu**, **Açık**, **Sistem**, **OLED Siyah**, **Gece**, **Yüksek Kontrast Koyu**, **Yüksek Kontrast Açık**. |
| Dil | Türkçe/İngilizce; değişiklik hemen uygulanır. |
| **Arayüz boyutu** | Yazı, simge ve boşlukları birlikte ölçekler. Kısayollar: Ctrl+= büyüt, Ctrl+- küçült, Ctrl+0 sıfırla. |
| **Renk körlüğü dostu renkler** | Durum ve grafik renklerini kırmızı/yeşil ayrımına dayanmayan renklerle değiştirir. |
| **Oturum açılınca başlat** | Bilgisayar açılınca Harness Studio başlar. Hemen uygulanır. |
| **Küçültülmüş başlat** | Otomatik açılışta pencere açılmaz, uygulama tepside bekler. Yalnız **Oturum açılınca başlat** açıkken kullanılabilir. |
| **Son oturumu geri yükle** | Açılışta en son başlattığınız sunucu yeniden başlar. |
| **Pencereyi kapatınca** | **Sor**, **Tepsiye Küçült** veya **Uygulamadan Çık**. macOS'ta **Menü çubuğunda tut** karşılığı vardır. |
| **Tepsideyken pencereyi bellekte tut** | Tepsideki sayfanın bellekte kalıp kalmayacağı; aşağıda anlatılır. |

## Güncellemeler

**Güncellemeler** sayfası uygulama sürümünü ve güncelleme denetimini içerir. Otomatik uygulama güncellemesi şu an kapalıdır: yeni sürümleri paket olarak elle indirip kurun. Güncelleme denetimi `Güncelleme sunucusu yapılandırılmamış` benzeri bir hata verir veya sonuç vermezse bunun nedeni, güncelleme adresinin bu ön sürümde henüz yapılandırılmamış olmasıdır. Motor (llama.cpp) güncellemesi ayrıdır ve **Model Motoru > Kurulu Model Motoru** sayfasında yönetilir. macOS'ta yeni sürümden sonra mikrofon ve Otomasyon izinlerini yeniden vermeniz gerekebilir.

## Gizlilik

**Gizlilik** yerel tanılama raporlarının içeriğini seçer: **Çökme raporları**, **Uygulama hataları**, **Sunucu hataları (llama.cpp)**, **Donanım bilgisi**, **Performans metrikleri**. Raporlar yalnız bu cihazda saklanır ve hiçbir yere otomatik gönderilmez. Bu raporlara istemler, model yanıtları, dosya içerikleri ve API istek gövdeleri girmez. Sohbet geçmişi ve isteğe bağlı analitik içerik kaydı ayrı işlevlerdir; [analitik sayfasındaki](07-performans-ve-analitik.md#analitik-ve-istek-günlüğü) düz metin içerik kaydı açıklamasını da okuyun.

## Tanılama

**Tanılama > Sağlık** sayfasında **Denetimi Çalıştır** kurulumu, model motoru uyumunu, GPU kütüphanelerini ve portları denetler; sonuçlar **Tamam**, **Uyarı** veya **Hata** olarak işaretlenir ve **JSON'u Kopyala** ile rapora eklenir. **Tanılama > Raporlar** yerel tanılama kayıtlarını inceleme (**İncele**), kopyalama ve silme (**Raporları Sil**) alanıdır. Bir hata için önce başarısız denetimin açıklamasını okuyun; [sorun giderme sayfasıyla](10-sorun-giderme.md) devam edin.

## Tepside sayfa bırakmak

**Pencereyi kapatınca** seçimi **Tepsiye Küçült** ise pencereyi kapatmak uygulamayı kapatmaz: uygulama sistem tepsisinde (macOS'ta menü çubuğunda) çalışmaya devam eder, sunucular ve indirmeler sürer. Tepsi simgesinden pencere yeniden açılır; tamamen kapatmak için simgenin menüsündeki **Çıkış** kullanılır. **Uygulamadan Çık** seçiliyse çalışan sunucular durdurulur. Bu masaüstünde tepsi veya menü çubuğu simgesi yoksa pencereyi kapatmak uygulamadan çıkar.

Tepsideyken bellek tasarrufu için pencerenin web sayfası kapatılabilir:

1. **Ayarlar > Görünüm ve Başlangıç** içinde **Pencereyi kapatınca** seçimini **Tepsiye Küçült** yapın.
2. Pencereyi kapatın. Sunucular ve indirmeler uygulama sürecinde çalışmaya devam eder.
3. **Tepsideyken pencereyi bellekte tut** kapalıysa (varsayılan davranış), pencere tepsideyken yaklaşık 30 saniye sonra arayüz sayfası kapatılır ve yalnız uygulama süreci kalır; bu, tepside yaklaşık 60 MiB daha az bellek demektir. Tepsi simgesinden açtığınızda sayfa yeniden kurulur (yaklaşık yarım saniye) ve açık sekme geri gelir.
4. Sayfa şu durumlarda kapatılmaz, tepsideyken bile bekletilir: Playground'da yanıt akıyorsa veya araç çağrısı sürüyorsa, bir indirme çalışıyorsa, bir araç onayı bekleniyorsa, yazılmış ama gönderilmemiş mesaj varsa, Dashboard'da kaydedilmemiş sunucu ayarı veya Ayarlar'da kaydedilmemiş değişiklik varsa, araç oluşturucu veya ses kaydı çalışıyorsa.
5. Hazır beklemesini istiyorsanız **Tepsideyken pencereyi bellekte tut** seçimini açın. Daha fazla bellek kullanır ama açılış anlıktır.

Yukarıdaki korumalara girmeyen girdiler (örneğin bir arama kutusuna yazılmış ama aranmamış metin) kaybolabilir; pencereyi kapatmadan önce işinizi kaydedin. macOS'ta ilgili tercih **Menü çubuğundayken pencereyi bellekte tut** olarak görünür.

<!-- Kaynak denetimi: src/lib/components/Settings.svelte, settings/sections.ts, settings/AppearanceSettings.svelte, settings/RuntimeSettings.svelte, settings/NetworkSettings.svelte, settings/SecuritySettings.svelte, settings/PrivacySettings.svelte, settings/DiagnosticsSettings.svelte, settings/DeveloperApiSettings.svelte, settings/OtelSettings.svelte; src/lib/windowActivity.ts (holdReleaseWhile ve 30 sn bırakma); src-tauri/src/desktop.rs (release_window); src/lib/locales/settings.ts, menus.ts, tools.ts, developer.ts, security.ts, gateway.ts, recovery.ts, shell.ts; docs/RELEASE_NOTES_0.1.0.md. -->
