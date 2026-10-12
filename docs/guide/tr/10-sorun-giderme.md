# Sorun Giderme ve SSS

English: [../en/10-troubleshooting.md](../en/10-troubleshooting.md)

**Bu sayfada ne yapılır:** Ekrandaki hata mesajını tanır, nedenini anlar, uygun düzeltmeyi uygular ve sık sorulan sorulara yanıt bulursunuz.

## Önce Üç Kontrol

1. **Sunucular** sayfasında doğru sunucunun durumunu (**Çalışıyor**, **Başlatılıyor**, **Kapalı**, **Hata**) ve adresini kontrol edin. (**Sunucular** sayfasını görmüyorsanız arayüzü **Gelişmiş** düzeye geçirin; Dashboard'ın **Özet** sayfası da seçili sunucunun durumunu gösterir.) Üstteki sunucu seçicisinde (**Kontrol edilen sunucu**) çalışmak istediğiniz sunucunun seçili olduğundan emin olun.
2. Dashboard'daki **Sunucu Logları** alanında ilk hata satırını okuyun. Yükleniyor durumunu bağlantı hatasıyla karıştırmayın; büyük bir modelin yüklenmesi dakikalar sürebilir.
3. **Ayarlar > Tanılama > Sağlık** sayfasındaki **Denetimi Çalıştır** düğmesini kullanın. Model motoru, GPU kütüphanesi veya port denetiminde **Hata** ya da **Uyarı** varsa önce onu giderin.

Aşağıdaki mesajlar uygulamanın Türkçe arayüzündeki hata metinlerinden alınmıştır. `{url}`, `{path}`, `{port}` ve benzeri alanlar ekranda gerçek değerlerle doldurulur. İşletim sistemi hata ayrıntısı farklı dilde olabilir.

## Hata Mesajları ve Çözümleri

### Sunucuya Bağlanılamıyor

| Gerçek mesaj | Neden ve ne yapılır? |
| --- | --- |
| `Sunucuya bağlanılamadı ({url}). Sunucu çalışıyor mu? Sunucular sayfasından kontrol edin.` | Yerel adrese (127.0.0.1) bağlanılamadı: sunucu kapalı, çökmüş veya başka portta olabilir. **Sunucular** sayfasında durumu kontrol edin, kapalıysa **Başlat** ile açın. Çalışıyorsa istemcideki adresi uygulamada görünen adresle karşılaştırın; port Otomatik ise her başlatmada değişebilir, bu yüzden sabit adres için ağ geçidini kullanın. |
| `Sunucuya bağlanılamadı ({url}). Ağ bağlantısını, vekil sunucu (proxy) ayarını ve adresin doğruluğunu kontrol edin.` | Yerel olmayan (uzak) bir adrese bağlanılamadı. Adresi, ağ bağlantısını ve proxy ayarını kontrol edin. Model indirirken çıkıyorsa Hugging Face'e erişimi de inceleyin. |
| `İstek zaman aşımına uğradı ({url}). Yerel sunucu meşgul olabilir veya yanıt vermiyor olabilir. Sunucular sayfasında durumunu ve günlüğünü kontrol edin.` | Yerel sunucu yanıt vermedi. Çalışan isteklerin bitmesini bekleyin; günlükte hata veya kapanma varsa sunucuyu yeniden başlatın. |
| `İstek zaman aşımına uğradı ({url}). Ağ bağlantısını, vekil sunucu (proxy) ayarını ve uzak sunucunun yanıt verdiğini kontrol edip tekrar deneyin.` | Uzak sunucu yanıt vermedi. Ağı ve uzak sunucuyu kontrol edin. |
| `Sunucunun adı çözümlenemedi ({url}). Adresi ve ağ/DNS ayarlarını kontrol edin.` | Alan adı çözümlenemiyor: adreste yazım hatası veya DNS sorunu. |
| `Güvenli bağlantı kurulamadı ({url}). Sunucunun TLS sertifikasını ve bilgisayarın tarihini kontrol edin.` | Sertifika geçersiz ya da bilgisayarın saati yanlış. |

### Model Belleğe Sığmıyor veya Sunucu Yüklenirken Duruyor

| Gerçek mesaj | Neden ve ne yapılır? |
| --- | --- |
| `GPU belleği yetmedi; sunucu durduruldu. [{code}]` ve **Model belleğe sığmadı** penceresi | Model, bağlam veya hesaplama alanı ekran kartı belleğine sığmadı. [OOM kurtarma adımlarını](03-dashboard-ve-sunucular.md#bellek-yetmediğinde-oom) izleyin: **Bununla Dene** düğmesi bağlamı küçültür, KV önbelleğini sıkıştırır veya GPU katmanını azaltır. Kalıcı çözüm için daha küçük bir model veya kuant seçin. |
| `Bellek yetersiz: llama-server model için yer ayıramadı (VRAM ya da RAM).` | Aynı durumun sınıflandırılmış açıklaması. RAM ve VRAM ihtiyacını birlikte kontrol edin; başka bir sunucu ekran kartını tutuyorsa onu durdurun. |
| `Model GPU belleğine sığmıyor: ~{need_mib} MiB gerekiyor, birleşik bellekte GPU payı {budget_mib} MiB. Bağlamı ya da GPU katmanlarını azaltın.` | Başlatma öncesi bellek kontrolü. Önerilen kadar bağlamı veya GPU katmanını azaltın. |
| `Ekran kartı işi durdurdu (sürücü sıfırlandı ya da GPU hatası). Sunucuyu yeniden başlatın; tekrarlarsa daha az GPU katmanı ya da başka bir sürücü sürümü deneyin.` | GPU sürücüsü işlemi yarıda kesti. Sürücüyü güncelleyin veya GPU katmanını azaltın. |
| `Model dosyası bozuk ya da eksik (yarım kalmış indirme?). Dosyayı yeniden indirin.` | Dosya tam inmemiş ya da bozuk. Modeli yeniden indirin; parçalı modelde tüm parçaların mevcut olduğunu kontrol edin (**Kütüphanem > Onar**). |
| `Bu llama.cpp runtime'ı "{arch}" model mimarisini desteklemiyor. Runtime'ı güncelleyin.` | Model yeni bir mimari; motor eski. **Ayarlar > Model Motoru > Kurulu Model Motoru** sayfasında güncelleme denetleyin. |
| `Sunucu {seconds} sn içinde hazır olmadı. Büyük modeller için yükleme zaman aşımını artırın.` | Model zamanında yüklenemedi. **Yükleme zaman aşımı (sn)** değerini Dashboard'ın **GPU** sayfasında (o model için) veya **Ayarlar > Ağ > Dinleme** sayfasında (genel olarak) artırın. Günlükte başka bir hata varsa önce onu çözün. |
| `llama-server yüklenirken sonlandı ({status}). Logları kontrol edin. [{code}]` | Süreç yükleme sırasında çıktı. **Sunucu Logları**'nın son satırları nedenini söyler (bellek, bozuk dosya, desteklenmeyen mimari, eksik kütüphane). |

### Port Kullanımda

| Gerçek mesaj | Neden ve ne yapılır? |
| --- | --- |
| `{port} portunu “{program}” (PID {pid}) kullanıyor. Başka bir port seçin ya da portu Otomatik yapın.` | Seçtiğiniz portu başka bir program tutuyor. **Dashboard > Ağ** sayfasında başka bir port yazın veya **Otomatik** seçin. Tanımadığınız bir programı kapatmak yerine portu değiştirin. |
| `{port} portunu “{server}” sunucusu kullanıyor. Başka bir port seçin ya da portu Otomatik yapın.` | İki Harness Studio sunucusuna aynı port verilmiş. Farklı port verin veya otomatik seçimi kullanın. |
| `Gateway bu adreste başlatılamadı (port kullanımda olabilir)` | Ağ geçidi portu dolu. **Ayarlar > Ağ Geçidi > Uç Nokta** sayfasında portu değiştirin; varsayılan port doluysa Harness Studio kendisi boş bir porta geçer ve yeni adresi **Bağlantılar** sayfasında gösterir. |

### Model Motoru (Motor) Eksik veya Başlamıyor

| Gerçek mesaj | Neden ve ne yapılır? |
| --- | --- |
| `llama.cpp runtime henüz kurulu değil. Ayarlar > Model Motoru bölümünden tek tıkla indirin veya sisteminizdeki llama-server’ı seçin.` | Motor kurulmamış. **Ayarlar > Model Motoru > Kurulu Model Motoru** sayfasında yönetilen motoru indirin. |
| `llama-server bulunamadı: {path}` | Seçilen dosya yok. Yönetilen motoru kurun veya özel ikilinin yolunu düzeltip **Doğrula** seçin. |
| `Sistemde geçerli bir llama-server ikili dosyası bulunamadı.` | Sisteme kurulu motor aranırken hiçbiri bulunamadı. Yönetilen motoru kurun. |
| `Özel llama-server yolu ayarlanmamış veya geçerli değil.` | **Özel İkili** seçili ama yol boş veya yanlış. Yolu girin veya **Yönetilen (önerilen)** seçeneğine dönün. |
| `Runtime'ın ihtiyaç duyduğu bir kütüphane yüklenemedi (GPU sürücüsü ya da runtime dosyaları eksik veya uyumsuz).` | Sürücü eksik/eski veya derleme donanıma uymuyor. **Sağlık** denetimini çalıştırın; önerilen derlemeyi ve güncel bir sürücüyü kullanın (örneğin CUDA için yeterince yeni bir NVIDIA sürücüsü, yoksa Vulkan derlemesi). |
| `Aşama 1 doğrulaması başarısız: binary çalıştırılamadı (...). Eksik DLL veya mimari uyumsuzluğu olabilir.` | İndirilen motor çalıştırılamadı. Başka bir derleme (Vulkan veya CPU) kurmayı deneyin. |
| `Runtime değiştirilmeden önce çalışan tüm sunucuları durdurun.` | Motoru değiştirmek için önce sunucuları durdurun. |
| `llama-bench bulunamadı: {path}. Resmi llama.cpp paketini kullanın.` | Benchmark için `llama-bench` gerekir. Yönetilen resmi paket onu içerir; özel paket kullanıyorsanız dosyanın orada olduğunu kontrol edin. |

Model Hub'daki veya Kütüphanem'deki bir kart bir motorun gerektiğini söylüyorsa (Konuşma modülü, Görsel modülü, ONNX Runtime, sherpa-onnx), onu Model Hub'daki **Kur** düğmesinden veya **Ayarlar > Model Motoru > Registry** sayfasından kurun. Bir GGUF dönüştürme başarısız olursa mesaj aşamayı ve dönüştürücünün çıktısının sonunu söyler; dönüştürme için resmi yönetilen llama.cpp motoru, yeterli boş disk alanı ve kurulu dönüştürücünün mimarisini desteklediği bir depo gerekir.

### Model İndirme

| Gerçek mesaj | Neden ve ne yapılır? |
| --- | --- |
| `SHA-256 doğrulaması başarısız ({file}). Bozuk dosya silindi, lütfen tekrar indirin.` | İnen dosya beklenen özetle eşleşmedi (bağlantı kesintisi veya bozulma); dosya otomatik silindi. İndirmeyi baştan başlatın. Lisansı yeni onaylanmış modellerde maskeli özet geçersiz sayılır ve doğrulamada taze bir özet çekilir; bu yüzden bu uyarı artık gerçek bir uyuşmazlığa işaret eder. |
| `Eksik indirme ({file}): {downloaded} / {total} bayt. Tekrar denerseniz kaldığı yerden devam eder.` | Bağlantı koptu. İndirme kuyruğunda **Devam et**'e basın; bağlantıyı ve disk alanını kontrol edin. |
| `Yetersiz disk alanı. Hedef sürücüde en az {gb} GB boş alan olmalıdır.` | Yer açın veya **Ayarlar > Depolama > Dizinler** sayfasından yeterli boş alanı olan bir birincil model klasörü seçin. |
| `İndirme başarısız: HTTP {status}. Bu model erişim onayı (gated) veya geçerli bir Hugging Face token’ı gerektiriyor olabilir.` | Model lisans onaylı olabilir; aşağıdaki bölüme bakın. |
| `Hugging Face istek sınırına ulaşıldı. {retry} saniye sonra tekrar deneyin; giriş yapmak sınırı yükseltir.` | Hugging Face'in anonim istek sınırı. Bekleyin veya hesabınızı bağlayın. |
| `İndirilen dosya geçerli bir GGUF değil (sihirli baytlar uyuşmuyor).` | İnen şey bir model dosyası değil (örneğin bir hata sayfası). Adresi ve depoyu kontrol edin. |
| `Model bulunamadı: {path}` | Dosya taşınmış veya silinmiş. **Kütüphanem**'de **Kütüphaneyi Yeniden Tara**'ya basın ve model tanımındaki dosya seçimini düzeltin. |

### Lisans Onaylı (Gated) Model ve Hugging Face Hesabı

| Gerçek mesaj | Neden ve ne yapılır? |
| --- | --- |
| `{repo} lisans onayı gerektiriyor. Hugging Face ile giriş yapın (Ayarlar > Depolama > Hugging Face Hesabı), model sayfasında lisansı kabul edin ve indirmeyi yeniden deneyin.` | Hesap bağlı değil. [Hesap bağlama ve lisans akışını](02-model-hub-ve-kutuphane.md#lisans-onaylı-bir-model-i̇ndirme) tamamlayın. |
| `{repo} lisans onayı gerektiriyor ve hesabınızın henüz erişimi yok. Model sayfasında lisansı kabul edin (bazı modellerde onay elle verilir), ardından indirmeyi yeniden deneyin.` | Hesap bağlı ama lisans kabul edilmemiş veya onay bekliyor. Aynı hesapla model sayfasında lisansı kabul edin, sonra **Tekrar Dene**'ye basın. Elle onayda beklemek gerekebilir ("Erişim hâlâ yok. Onay elle veriliyorsa biraz sonra tekrar deneyin."). |
| `Hugging Face tokenı geçersiz ya da süresi dolmuş.` | Yeni bir **Read** tokenı oluşturup **Ayarlar > Depolama > Hugging Face Hesabı** sayfasında **Erişim tokenı** alanına kaydedin. |
| `Hugging Face ile giriş için bir OAuth uygulaması istemci kimliği (client ID) gerekiyor. Ayarlar > Depolama > Hugging Face Hesabı bölümünden girin ya da erişim tokenı kullanın.` | Tarayıcıyla giriş için istemci kimliği yok. En basiti erişim tokenı kullanmaktır. |
| `Hugging Face girişi zaman aşımına uğradı (5 dakika). Yeniden deneyin.` | Tarayıcı onayı zamanında tamamlanmadı. **Hugging Face ile Giriş Yap** ile yeniden deneyin. |

### Model Meşgul (503)

Ağ geçidi bir modeli yüklemek istediğinde, bellekte yer açmak için boşaltabileceği bir model bulamayabilir; çünkü diğer modeller istek işliyordur. Bu durumda istek 503 ile reddedilir.

| Gerçek mesaj | Neden ve ne yapılır? |
| --- | --- |
| `{model} şu an yüklenemedi: {busy} istek işliyor ve kapatılamıyor. {wait} tekrar deneyin.` (Playground) | Yeni modeli yüklemek için bellek yok ve yüklü model(ler) o anda yanıt üretiyor. Yanıtlar bitince Playground'daki **Tekrar Dene** düğmesine basın. |
| `{models} meşgul; istekleri bitince {model} için tekrar deneyin.` ve `Meşgul modelin isteği bitince tekrar deneyin.` | Aynı durum, ağ geçidinin bildirdiği haliyle. HTTP istemciniz İngilizce bir ileti (ör. `Cannot load "{model}": model(s) … are busy; retry after their requests finish.`), `model_busy` kodu, 503 durumu ve `Retry-After` başlığı alır; bekleyip yeniden denemelidir. Sık oluyorsa **Ayarlar > Ağ Geçidi > Yönlendirme** sayfasında **En fazla yüklü model**, **Model başına kuyruk** ve **VRAM payı (MiB, 0 = otomatik)** değerlerini gözden geçirin veya bir modeli **Sabit (hep yüklü)** yapın. |

## Başlatma Hatasının Açıklaması

Sunucu başlatma hataları ayrıca sınıflandırılmış bir açıklama ve **Kurtarma Seçenekleri** gösterebilir. **Logları aç** düğmesi ilgili satırları (**Log Satırları**) açar. Yukarıdaki tablolardaki bellek, kütüphane, bozuk model, mimari ve zaman aşımı açıklamaları bu sınıflandırmadan gelir.

## Sık Sorulan Sorular

**Terminal öğrenmem gerekiyor mu?** Hayır. Hazır uygulama paketinde motor kurma, model indirme, sunucu başlatma ve sohbet etme arayüzden yapılır. macOS'ta whisper sunucusu gibi hazır paketi bulunmayan akışlar ek hazırlık gerektirir; bunlar [Kurulum](01-kurulum.md) ve [Konuşma](04-playground.md#konuşma-yazıya-dökme-ve-sesli-okuma) bölümlerinde belirtilmiştir.

**Hangi modeli indirmeliyim?** Önce **Model Hub**'da **Donanım uyumu** sütunu **VRAM'e sığıyor** diyen bir dosya seçin. Emin değilseniz Q4_K_M kuantını ve küçük bir modelle (1B-8B parametre) başlayın; sonra büyütün.

**Model indirdim, neden sohbet edemiyorum?** İndirilen dosyanın bir sunucuda yüklenmesi gerekir. Dashboard'dan başlatın veya açık ağ geçidinde model tanımını seçin. Model embedding, reranker, Whisper, ses veya görsel modeliyse ilgili Playground sekmesini kullanın.

**Bu kılavuzun andığı bir sayfayı bulamıyorum.** **Basit** arayüz düzeyinde olabilirsiniz. Ana gezinmenin altındaki anahtarla **Gelişmiş** düzeye geçin (bkz. [Basit ve Gelişmiş düzeyler](README.md#basit-ve-gelişmiş-arayüz-düzeyleri)); geçiş kayıtlı hiçbir değeri değiştirmez.

**Model aramada görünmüyor; hesap gerekir mi?** Önce filtreleri sıfırlayın. Varsayılan Hub listesi GGUF depolarıyla sınırlıdır; bu yüzden GGUF dosyası olmayan resmi depolar (ör. `meta-llama/Llama-3.2-1B-Instruct`) görünmez, yalnız üçüncü kişilerin GGUF kopyaları listelenir. Konuşma için `whisper` arayın. Özel veya lisans onaylı depolarda ayrıca hesap erişimi gerekir.

**Token ekledim, neden gated model inmiyor?** Tokenın hesabı ilgili modelin lisansını kabul etmiş olmalıdır. Bazı model sahipleri erişimi elle onaylar. OAuth istemci kimliği, model erişim izninin yerini tutmaz.

**Sohbet uzadıkça model neden önceki bilgileri unutuyor?** Bağlam sınırı vardır. Bağlam kaydırma eski içeriği çıkarabilir. Yeni bir sohbet açın, **Geçmiş ve Compaction** ile sıkıştırın veya belleğiniz uygunsa sunucuyu daha büyük bağlamla başlatın.

**Üretim neden yavaş?** GPU katmanlarının hepsi ekran kartına sığmıyorsa kalan kısım CPU'da çalışır ve hız belirgin biçimde düşer. Dashboard'da **GPU katmanları** ve donanım uyumu rozetini, **Performans** sayfasında üretim hızını (t/s) kontrol edin; daha küçük bir kuant veya bağlam hızı artırır. Kurulu motor yalnız CPU kullanıyorsa **Ayarlar > Model Motoru > Kurulu Model Motoru** sayfasındaki uyarıyı okuyup bir GPU derlemesi kurun.

**Dosya aracı neden klasör istiyor?** Sohbetin çalışma klasörü seçilmeli veya **Ayarlar > Araçlar > Yerleşik Araçlar** içindeki varsayılan klasör kullanılmalıdır. Sohbet listesindeki klasörler bu seçimden ayrıdır.

**Mikrofon çalışmıyor; ne yapmalıyım?** Sistem gizlilik ayarlarında Harness Studio'ya mikrofon izni verin ve etkin mikrofonu kontrol edin. Kayıt yerine **Dosya Seç** ile bir ses dosyası da kullanabilirsiniz. Desteklenen biçimler [Konuşma bölümünde](04-playground.md#konuşma-yazıya-dökme-ve-sesli-okuma) listelenmiştir.

**Pencereyi kapatmak modeli durdurur mu?** **Tepsiye Küçült** sunucuları açık bırakır. **Uygulamadan Çık** çalışan sunucuları durdurur. Tepside arayüz bellekte tutulmuyorsa kaydedilmemiş girdiler kaybolabilir; [Ayarlar](08-ayarlar.md#tepside-sayfa-bırakmak) bölümünü okuyun.

**Konuşmalar veya istekler diske kaydediliyor mu?** Sohbet geçmişi yerel olarak saklanır; veritabanı şifreli değildir. Analitik varsayılan olarak yalnız meta veri (zaman, model, token, süre) tutar. Bir model tanımında içerik kaydı açılırsa istemler ve yanıtlar düz metin olarak saklanır; [Analitik](07-performans-ve-analitik.md#analitik-ve-i̇stek-günlüğü) bölümüne bakın. Uygulama otomatik telemetri veya çökme raporu göndermez.

**Yeni sürümü nasıl denetlerim?** **Ayarlar > Güncellemeler** sayfasına bakın veya en yeni kurulum paketini [GitHub Releases](https://github.com/struklabs/harness-studio/releases) sayfasından indirin. Motor güncellemeleri ayrıca **Ayarlar > Model Motoru > Kurulu Model Motoru** sayfasında yapılır.

**Eski sürüme nasıl dönerim?** Harness Studio'yu kapatın ve eski sürümü kurmadan önce veri klasöründeki `config/settings.json.bak-v2` dosyasını `config/settings.json` üzerine kopyalayın. Yeni ayar biçimine geçtikten sonra değiştirdiğiniz ayarlar kaybolur. Ayrıntı için [sürüm notlarına](../../tr/RELEASE_NOTES_0.1.0.md) bakın.

Sorun sürerse hata metnini, uygulama ve model motoru sürümlerini, **Sağlık** denetim sonucunu ve dışa aktarılan günlükleri toplayın. Paylaşmadan önce içeriği inceleyin. [Güvenlik modeli](../../tr/SECURITY_MODEL.md), [entegrasyonlar](../../tr/INTEGRATIONS.md), [CLI](../../tr/CLI.md), [MCP](../../tr/MCP.md) ve [araçlar](../../tr/TOOLS.md) belgeleri daha ayrıntılı başvurulardır.
