# Dashboard ve Sunucular

English: [../en/03-dashboard-and-servers.md](../en/03-dashboard-and-servers.md)

**Bu sayfada ne yapılır:** Model seçilir, çalıştırma ayarları düzenlenir, sunucu başlatılır, ayarlar profil olarak saklanır ve gerekirse bellek yetersizliği hatasından kurtarılır.

## Ekranın Yapısı

**Dashboard** iki parçadan oluşur. Solda ayarlar vardır. **Basit** arayüz düzeyinde **Özet**, **Model ve Model Motoru** ve **Bağlam ve Bellek** sayfalarını görürsünüz. **Gelişmiş** düzeyde liste şöyledir:

| Grup | Sayfa | İçeriği |
| --- | --- | --- |
| Sunucu | **Özet** | Seçili sunucunun adı, amacı ve durumu. |
| Sunucu | **Model ve Model Motoru** | Model, **Model tanımı** ve motor seçimi; **Router Modu** anahtarı. |
| Sunucu | **Ağ** (Gelişmiş) | Port, dinlenen adres ve sunucu API anahtarı. |
| Çıkarım | **Bağlam ve Bellek** | Bağlam boyutu, eşzamanlı istek, konuşma hafızası hassasiyeti, bağlam kaydırma. |
| Çıkarım | **GPU** (Gelişmiş) | GPU katmanları, hedef ekran kartı, CPU iş parçacığı, flash attention, yükleme zaman aşımı. |
| Çıkarım | **Örnekleme** (Gelişmiş) | Örnekleme ayarlarının nerede düzenleneceğini gösterir (Playground ve hazır ayarlar). |
| Gelişmiş | **Şablon ve Reasoning**, **Araçlar**, **Diğer Parametreler** | Sohbet şablonu, taslak model (spekülatif çözme), LoRA adaptörleri, ham komut satırı parametreleri. |

Sağ tarafta canlı telemetri (üretim hızı, VRAM ve RAM kullanımı) ve altında **Sunucu Logları** bulunur. En altta ayarlarınızı kaydeden bir çubuk, onun üstünde ise **Sunucuyu Başlat** / **Sunucuyu Durdur** düğmesi yer alır.

Üstteki sunucu seçici (**Kontrol edilen sunucu**) Dashboard, Playground, Bağlantılar ve Performans sayfalarının hangi sunucuyla çalışacağını belirler. Birden fazla sunucunuz varsa önce bunu kontrol edin.

## İlk Sunucunuzu Başlatın

1. **Dashboard > Model ve Model Motoru** sayfasında yerel bir model seçin ve varsa bir **Model tanımı** seçin. Liste boşsa **Model Hub'a git ve model indir** düğmesini kullanın.
2. Model motoru uyarısı varsa önce [motoru kurun veya doğrulayın](01-kurulum.md#model-motoru-seçimi).
3. **Bağlam ve Bellek** ile **GPU** sayfalarındaki önerileri okuyun. **Otomatik**, GGUF modelinin eğitim bağlamını en fazla 32.768 token olarak kullanır (bilinmiyorsa 32.768); belleğe sığdırma bunu azaltabilir. Modelin sınırını ve boş belleğinizi kontrol edin. Modelin yanındaki donanım uyumu rozeti (**VRAM'e sığıyor**, **VRAM sınırda**, **Kısmi GPU yükleme**, **Çoğunlukla CPU**, **Bellek Yetmeyebilir**) tahmini gösterir.
4. **Ağ** sayfasında portu kontrol edin. **Otomatik** seçildiğinde Harness Studio her başlatmada 11440 ile 11499 arasından boş bir port seçer. Kendiniz bir port yazarsanız o port kaydedilir.
5. **Sunucuyu Başlat** düğmesine basın. **Başlatılıyor...** görünürken model belleğe yüklenir (büyük modellerde dakikalar sürebilir; **İptal Et** ile vazgeçilir). **Sunucu aktif** durumunu bekleyin.
6. **Playground**'a geçip sohbet edin. İşiniz bitince **Sunucuyu Durdur**, modelin kullandığı belleği boşaltır.

**Bağlam boyutu** ve **GPU katmanları** **Otomatik** bırakıldığında Harness Studio bunları sizin yerinize seçer. Küçük bir ekran kartında tüm katmanları GPU'da tutup bağlamı küçültmeyi tercih eder (sırasıyla 32k, 24k, 16k, 12k, sonra 8k dener; her birinde önce otomatik slot, sonra tek slot kullanır); olmazsa modelin bir kısmını CPU'da tutan 16k'lık bir hibrit düzene geçer. Dashboard formu, kullanmak üzere olduğu bağlam, katman ve slot değerlerini gösterir. Kendiniz belirlediğiniz değerlere (KV önbelleği, slotlar, bağlam, sığdırma) dokunulmaz. Aynı kural bir modeli CLI'dan, Control API'den, Kütüphanem'deki **Yükle ve Sohbet Et** ile veya Playground'daki **Yükle** ile başlattığınızda da geçerlidir.

**Router Modu** anahtarı, tek bir model yerine model klasörünüzdeki tüm hazır modelleri tek bir sunucuda sunar; istemci model adını belirterek istediğini seçer ve model ilk istekte ya da **Router Modelleri** listesinden yüklenir. Günlük kullanım için tek bir model veya [ağ geçidi](05-baglantilar-ve-ag-gecidi.md) yeterlidir.

**Sunucu modu** ayarı normalde model türünü izler: embedding modeli embedding sunucusu, reranker ise rerank sunucusu olarak başlar. Gerekirse değiştirilebilir.

## Ayarlar Ne Anlama Gelir?

Aşağıdaki alanlar uygulamadaki adlarıyla verilmiştir. Emin değilseniz önerilen değerleri ve **Otomatik** seçimleri bırakın.

| Alan (sayfa) | Sizin için anlamı |
| --- | --- |
| **Bağlam boyutu** (Bağlam ve Bellek) | Modelin bir istekte işleyebileceği toplam girdi ve çıktı miktarı (token). Sohbet geçmişi, sistem istemi, belgeler ve araç bilgileri bu alanı paylaşır; dolunca model en eski kısmı unutur. Büyük değer daha fazla bellek ister. Kodlama ajanları günlük sohbetten çok daha büyük bağlam ister (OpenCode için en az 48k). |
| **Eşzamanlı istek** (Bağlam ve Bellek) | Modelin aynı anda kaç isteğe yanıt vereceği (sunucunun "slot" sayısı). Fazlası sırada bekler. Slotlar bağlamı paylaşır: aynı anda çalışan isteklerin toplamı bağlama sığmalıdır. **Otomatik** seçildiğinde Harness Studio belleğe göre bir değer önerir; tek kullanıcı için çoğunlukla 1, ajanlar için 2 olur. |
| **Konuşma hafızası hassasiyeti** (Bağlam ve Bellek) | Sohbetin çalışma hafızasının (KV önbelleği) kaç bitle saklanacağı. **Otomatik (önerilen)**, bellek yettiği sürece tam hassasiyeti (**Tam (16 bit)**) korur, yetmezse 8 bite iner. **Dengeli (8 bit)** belleğin yarısını kullanır, kalite farkı çok küçüktür. **Tasarruf** seçenekleri daha da az bellek kullanır; çok uzun bağlamlarda doğruluk düşebilir ve bazı karışık anahtar/değer çiftleri yavaş çalışır (uygulama sizi uyarır). |
| **K cache kuantı** ve **V cache kuantı** (Bağlam ve Bellek) | Yukarıdaki ayarın teknik karşılığı. **Özel (anahtar ve değer ayrı)** seçilince görünür. Anahtarı (K) 4 bite indirmek kaliteyi belirgin biçimde düşürür. |
| **Bağlam kaydırma yönetimi** (Bağlam ve Bellek) | Bağlam dolunca eski içeriğin otomatik kaydırılıp kaydırılmayacağı. Kaydırma üretimin sürmesini sağlar ama kaydırılan içerik modelin hafızasından çıkar. |
| **KV önbelleğini GPU'ya taşı** | Kapalıysa önbellek sistem RAM'inde kalır: ekran kartı belleğinden tasarruf edersiniz ama hız düşebilir. |
| **GPU katmanları** (GPU) | Modelin kaç katmanının ekran kartına yükleneceği. Hepsi kartta olduğunda hız en yüksektir. VRAM yetmezse azaltın; kalan katmanlar CPU'da çalışır, bu daha yavaştır ve daha fazla RAM ister. |
| **Hedef ekran kartı (GPU)** (GPU) | Birden fazla kartınız varsa hangisinin kullanılacağı. **Otomatik (önerilen)**, en çok belleğe sahip harici kartı seçer; harici kart varken tümleşik GPU seçilemez. |
| **CPU iş parçacığı** (GPU) | CPU hesabı için kullanılan çekirdek sayısı. Boş bırakılırsa (**Oto (önerilen)**) fiziksel çekirdek sayısı kullanılır. |
| **Flash Attention** (GPU) | Dikkat hesabını hızlandırır ve uzun bağlamların bellek yükünü azaltır. Destek, motora bağlıdır. Değer önbelleği (V) kuantlandığında Harness Studio bunu kendisi açar. |
| **Mantıksal batch** ve **Mikro batch** (GPU) | Tek seferde işlenen token sayısı. Hesaplama belleği hatasında mikro batch'i küçültmek yardımcı olabilir. |
| **Yükleme zaman aşımı (sn)** (GPU) | Sunucunun hazır olması için beklenecek süre. Büyük bir modeli yavaş diskten yüklerken artırın. Boşsa **Ayarlar > Ağ > Dinleme** sayfasındaki genel değer kullanılır. |
| **Taslak model** (Diğer Parametreler) | Spekülatif çözme: aynı aileden küçük bir model üretimi hızlandırır. Kazanç modele ve donanıma göre değişir. Harness Studio uyumlu bir taslak model önerebilir. |
| **LoRA adaptörleri** (Diğer Parametreler) | Modelin üzerine uygulanan, her biri bir ölçekle kullanılan adaptör dosyaları. Çalışan bir sunucuda **Canlı LoRA Ölçekleri**, yeniden başlatmadan ölçeği değiştirir; 0 adaptörü kapatır. |
| **Ek ham CLI parametreleri** (Diğer Parametreler) | Arayüzde karşılığı olmayan llama-server bayrakları. Motorun desteklemediği bayrakları zorlamayın. |

**Örnekleme** (sıcaklık, Top P vb.) istek bazındadır; Playground'un oturum ayarlarından veya hazır ayar yöneticisinden düzenlenir. Kodlama ajanlarına hazırlık için [Bağlantılar'daki akışa](05-baglantilar-ve-ag-gecidi.md#kodlama-ajanı-bağla) bakın.

### Model Tanımı ve Kaydetme Çubuğu

Seçili **Model tanımı** (bkz. [Kütüphanem](02-model-hub-ve-kutuphane.md#kütüphanem-ve-model-tanımları)) bu ayarların başlangıç değerlerini taşır. Formda değişiklik yaptığınızda ekranın altındaki çubuk kaydedilmemiş değişiklik olduğunu söyler. Sunucuyu başlatmak değişiklikleri kaydetmez. Çubuktaki seçenekler:

| Düğme | Etkisi |
| --- | --- |
| **Varsayılan Olarak Kaydet** | Değişiklikleri seçili tanıma yazar. Tanımı kullanan profiller de etkilenir. |
| **Yeni Tanım Olarak Kaydet** | Değişikliklerinizle ayrı bir tanım (yeni bir takma ad) oluşturur. |
| **Geri Al** | Formu tanımın kayıtlı değerlerine döndürür. |

## Bellek Yetmediğinde (OOM)

Bir model belleğe sığmazsa sunucu durur ve **Model belleğe sığmadı** penceresi açılır (hata metni: "GPU belleği yetmedi; sunucu durduruldu.").

1. Pencerede **Boş VRAM** ve tahmini ihtiyaç satırını, ayrıca neyin sığmadığını okuyun: KV önbelleği, model ağırlıkları veya hesaplama tamponları.
2. **Önerilen** olarak işaretlenmiş seçeneğe bakın. Çıkabilecek öneriler: GPU katmanlarını llama-server'a seçtirmek, bağlamı düşürmek, KV önbelleğini kuantlamak, daha az GPU katmanı kullanmak, MoE uzman ağırlıklarını CPU'da tutmak, daha küçük bir fiziksel batch kullanmak veya GPU'yu kullanan başka bir sunucuyu durdurmak.
3. Seçtiğiniz seçeneğin yanındaki **Bununla Dene** düğmesine basın.
4. Deneme başarılı olursa yeni bir sohbetle kontrol edin. Değerlerin kalıcı olmasını istiyorsanız **Bu değerleri model tanımına kaydet** kutusunu işaretleyin; kapalıysa değerler yalnızca bu başlatmaya uygulanır.
5. Hiç seçenek yoksa ("Bu hata için otomatik seçenek yok. Logları inceleyin."), hatayı incelemek için **Logları aç** düğmesini kullanın ve daha küçük bir model ya da daha küçük bir kuant (örneğin Q4_K_M) seçin.

GPU katmanlarını azaltmak RAM ihtiyacını artırır. Uygulama sistem belleğinin yetmeyebileceği konusunda uyarırsa bunu ciddiye alın. Kurtarma seçenekleri, yükleme sırasındaki bellek hataları içindir. Çalışırken beklenmedik biçimde duran sunucular için **Ayarlar > Ağ > Dinleme** sayfasında **Çökünce otomatik yeniden başlat** seçeneği vardır (yükleme hataları yeniden denenmez).

## Sunucular ve Profiller

**Sunucular** sayfası (Gelişmiş bir sayfa) her sunucunun durumunu (**Çalışıyor**, **Başlatılıyor**, **Durduruluyor**, **Kapalı**, **Hata**), modelini, adresini, portunu ve API anahtarı olup olmadığını gösterir. Her satırda **Başlat**, **Durdur**, **Yeniden Başlat**, **Dashboard'da Aç** ve **Playground'da Aç** işlemleri bulunur; durmuş bir örnek **Kaldır** ile listeden çıkarılabilir.

Profil, bir model ile bir çalıştırma ayarları takımının (port dahil) adlandırılmış kaydıdır. Bir profil kendi sunucu örneğinde tek tıkla başlar; böylece aynı anda farklı modeller, örneğin biri sohbet biri kod için, çalışabilir.

1. Dashboard'da istediğiniz modeli ve ayarları hazırlayın.
2. **Sunucu Olarak Kaydet** düğmesine basın, bir **Profil adı** girin ve kaydedin.
3. Bir sonraki kullanımda profili **Sunucular** sayfasından başlatın.
4. Bir profilin ayarlarını değiştirdiğinizde Dashboard'daki **Değişiklikleri Kaydet** düğmesini kullanın.
5. İsterseniz **Uygulama açılışında otomatik başlat** seçeneğini açın. Bu, Harness Studio'nun bir sonraki açılışında devreye girer.

**Yeni Sunucu**, aynı ayarlarla bir sonraki boş portta yeni bir sunucu taslağı oluşturur; başlatmadan önce modeli ve ayarları düzenleyin. Profilleri **Yeniden Adlandır**, **Çoğalt** ve **Sil** ile yönetirsiniz; çalışan bir profil silinemez, önce durdurun. Aynı anda çalışan sunucuların bellek ihtiyaçları toplanır ve hepsi ekran kartına sığmayabilir. İki sunucuya aynı portu verirseniz şu uyarıyı alırsınız: "Bu port şu anda “{label}” sunucusu tarafından kullanılıyor. Başlatmadan önce başka bir port seçin."

## Logları Kullanma

Dashboard'ın sağ altındaki **Sunucu Logları** alanı yükleme adımlarını ve hata satırlarını canlı gösterir. Bir sorunu bulmak için arama ve süzme seçeneklerini kullanın; satırları kendi hızınızda okumak için **Oto kaydır**'ı kapatın. **Dışa Aktar** düğmesi **Logu dışa aktar** penceresini açar; çıktıyı destek için kaydetmek üzere **Kaydet…** düğmesini kullanın. Paylaşmadan önce içeriği gözden geçirin. Çalışan bir sunucunun gerçekte hangi portu kullandığını görmek için uygulamada gösterilen adrese bakın.

Gelişmiş ayarlar ve sınırlar için [Özellikler başvurusuna](../../tr/FEATURES.md) ve [güvenlik modeline](../../tr/SECURITY_MODEL.md) bakın.

Sıradaki: [Playground](04-playground.md).
