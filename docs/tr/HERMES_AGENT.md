# Bir Ajan Bağlayın ve Bir Görev Tamamlayın

[English](../en/HERMES_AGENT.md)

0.2 W2'nin ilk dilimi **Nous Research Hermes Agent** kullanır. Modüller → Ajanlar bölümünü açın. Kurulum izinlerini inceleyin, gerekiyorsa yükleyin, Ayarlardan Harness Gateway başlatın ve bir sohbet modeli seçip Bağlan düğmesine basın. Basit bir soru girip Görevi Çalıştır düğmesini kullanın. Durdur, Harness'in başlattığı görev süreç ağacını sonlandırır. Bağlantıyı Kes, önceki yapılandırmayı geri yükler ve tokenı iptal eder. Modülü kaldırmak bağlantıyı ve modül kaydını kaldırır; Hermes kurulumunu, konuşmaları ve kullanıcı dosyalarını korur.

Basit mod bağlantı ve görev kontrollerini gösterir. Gelişmiş mod çalıştırılabilir dosyayı, yapılandırma/yedek yollarını ve kurulum çıktısını da gösterir. Başarısız kurulum çıktısı iki modda da görünür. Model önerileri mevcut donanım uyumu hesabını ve araç çağırma kanıtlarını kullanır; yalnızca şablon desteği doğrulanmış araç testi olarak sunulmaz. Harness Link sohbet modelleri de seçilebilir; yerel donanım önerileri yerel dosyalar içindir.

## Ajan Seçimi

2026-10-20 tarihli resmî kaynak araştırmasının karşılaştırması ve bağlantıları [İngilizce belgede](../en/HERMES_AGENT.md#upstream-comparison) bulunur. İki proje de MIT lisanslıdır. OpenClaw macOS/Linux için kabuk kurucusu ve npm paketi; Windows için yerel Hub, PowerShell kurucusu ve WSL2 seçenekleri sunar. JSON5 `~/.openclaw/openclaw.json` dosyasındaki `models.providers` altında `baseUrl`, `apiKey` ve model tanımları kullanılır. Görevler `openclaw agent --message … --json` veya `--local` ile çalıştırılabilir; Gateway ve oturum arayüzleri durum/çıktı sunar.

Hermes macOS/Linux/WSL2 için resmî kabuk kurucusu; yerel Windows için PowerShell kurucusu sunar. Masaüstü paketleri de vardır. `HERMES_HOME` altındaki `config.yaml` ve `.env` kullanılır; güncel varsayılan veri yolu POSIX'te `~/.hermes`, Windows'ta `%LOCALAPPDATA%\hermes` olur. `model.default`, `model.provider: custom`, `model.base_url` ve `model.api_key` özel OpenAI uyumlu uç noktayı seçer. `hermes chat -q … -Q`, oturumları ve `/status` komutu vardır; resmî Python API'si tamamlanma, çıktı ve hata bilgisini döndürür.

Hermes'in açık boş araç listesi kabul eden süreç içi Python API'si ve yerel Windows kaynak kurulumu bu dilime doğrudan uyar. OpenClaw'ın sağlayıcı/model kataloğu ve durum sahipliğiyle ek eşleştirme gerekmez. Bu bir entegrasyon tercihidir; OpenClaw'ın WSL gerektirdiği anlamına gelmez. Güncel Hermes CLI sohbeti ayrı bir Gateway süreci başlatabildiği için Stop denetimi resmî Python API'siyle sağlanır.

## Kurulum ve Sınırlar

Harness, resmî `hermes-agent.nousresearch.com/install.ps1` veya `install.sh` dosyasını indirip etkileşimsiz, tarayıcı ve bilgisayar kullanımı kurulumu atlanmış şekilde çalıştırır. Fork, mesajlaşma kurulumu veya servis oluşturmaz. Bağımlılıkları upstream kurucu yönetir. Kurulum onayı bu betiğin kullanıcı yetkilerinizle çalışmasına izin verir; bu, Harness'in hash ile sabitlenmiş çalışma zamanı kurucusu değildir. Boyutlar tahmindir. Kaynak derleme bağımlılıkları gerekebilir. İptal, kurulum süreç ağacını sonlandırır.

Algılama yönetilen çalışma zamanını, PATH ve standart kaynak kurulum yollarını, süre sınırlı sürüm sorgusunu ve Python API çalışma zamanını denetler. Sürüm kısa süre önbelleğe alınır. Güncel PM kaynak başlatıcısı ve eski sanal ortamlar desteklenir. Kaynak/API çalışma zamanı bulunamayan yalnızca masaüstü kurulumları eksik bağımlılık olarak gösterilir; resmî kaynak kurulumunu kullanın. Yönetilen çalışma zamanının güncellemeleri [Modül Yaşam Döngüsü](MODULE_LIFECYCLE.md) belgesindeki kaynak sabiti ve geri alma akışını kullanır. Harici kaynak kurulumları kendi upstream güncelleme akışını korur.

## Güvenlik ve Kurtarma

Bağlan, yerel Gateway adresi ve model önizlemesinden sonra yalnızca `inference` kapsamı, seçilen tek model ve sohbet/model listesi uç noktaları için token oluşturur. Kontrol, araç, kabuk veya admin yetkisi verilmez. Anahtar IPC'ye veya süreç argümanlarına girmez; Hermes'in özel yapılandırmasında bulunur. Harness mevcut tuzlanmış token özeti deposunu kullanır. Yerel Gateway tokensız erişime açıksa diğer yerel istemciler kısıtları aşabilir; tüm yerel bağlantıları kısıtlamak için token gereksinimini açın.

Yapılandırmanın özgün baytları özel `config.yaml.harness.bak` dosyasına kaydedilir. Atomik değiştirmeden önce özel `harness-connection.json` kurtarma kaydı yazılır. Bozuk YAML ve mevcut yedek reddedilir; ilgisiz ayarlar korunur. Bağlantıyı Kes önce tokenı iptal eder, sonra özgün dosyayı geri yükler veya daha önce olmayan yapılandırmayı siler. Dışarıdan düzenlenen dosyanın üzerine yazılmaz; yedek ve kurtarma kaydı korunur. Yedeği inceleyip özgün baytları `config.yaml` dosyasına geri yükleyin ve Bağlantıyı Kes işlemini yeniden deneyin. Kesinti sonrası sahipsiz kalmış yedek yeniden bağlanmadan önce incelenmelidir.

Görev ayrı bir geçici Hermes klasörü, açık yerel model kimlik bilgileri ve temizlenmiş kimlik ortamı kullanır. Kullanıcı bağlamı, bellek, beceriler, kancalar ve MCP yapılandırması yüklenmez. Her görev için arayüzde yeniden izin verilir. Varsayılan `enabled_toolsets=[]` yalnızca çıkarıma izin verir. İsteğe bağlı `harness_read_file` aracı için en fazla sekiz klasörün mutlak yolu seçilebilir. Araç sembolik bağlantıları ve Windows bağlantı noktalarını çözerek klasör sınırını denetler; yalnızca normal dosyaları, okuma başına en fazla 32 KiB olacak şekilde okur. Upstream araç listesi izin verilen listeyle tam eşleşmezse görev reddedilir. Kabuk, yazma ve ek ağ erişimi verilemez; hassas eylemler ve etkileşimli onaylar desteklenmez. Bu, uygulama düzeyinde bir kısıttır; güvenilen upstream Python kodunu işletim sistemiyle yalıtmaz. Diğer Hermes arayüzlerinin kendi izin kuralları geçerlidir. Çıktı 64 KiB ile sınırlıdır; token gizlenir. Görev en fazla on dakika, dört yineleme ve yineleme başına 512 üretilen token sürer.

## Görev Geçmişi ve Denetim

Harness uygulama veri kökünde kendisine ait özel `agents/history.json` dosyasını saklar. Çalışan ve geçmiş Harness görevleri model, istem, başlangıç/bitiş zamanı, süre, upstream bildiriyorsa token kullanımı, hatalar ve yeniden açılabilir çıktıyla listelenir. Verilen klasör izinleri, araç çağrıları/retleri ve başlatma/durdurma olayları kaydedilir. Uygulama kesintisinde çalışan kalan kayıt sonraki okumada Kesintiye Uğradı olarak işaretlenir. Geçmiş Kaydını Sil yalnızca Harness kaydını, denetim bilgisini ve çıktısını siler; Hermes oturumlarını, veritabanlarını, yapılandırmasını veya kullanıcı dosyalarını değiştirmez. Çalışan kayıt silinmeden önce durdurulmalıdır. Dış Hermes oturumları içe aktarılmaz. Saklanan istem ve çıktı özel kullanıcı verisidir.

## Mevcut Paperclip Örnekleri

Yapılandırma Oluştur, seçilen yerel veya Harness Link sohbet modeli ile sohbet/model listesi uç noktalarına kısıtlı, ayrı bir Paperclip çıkarım tokenı üretir. Gateway token gerektirmelidir. Adres ve yapılandırma incelenip kopyalanabilir; açık token yalnızca bu aktarım ekranında gösterilir, Harness geçmişine yazılmaz. Buradan veya Ayarlardan iptal edilebilir. Adres Paperclip aracısının çalıştığı makineden erişilebilir olmalıdır; yerel adres aynı makineyi gerektirir.

Otomatik Uygula, mevcut Paperclip örneğindeki **mevcut bir `opencode_local` aracısını** hedefler. Resmî GET/PATCH aracı API'si, aracıya özel ortam yapılandırması ve OpenCode'un satır içi OpenAI uyumlu sağlayıcı ayarları kullanılır; [kaynak bağlantıları İngilizce belgede](../en/HERMES_AGENT.md#existing-paperclip-instances) bulunur. İlgisiz aracı ayarları, ortam değişkenleri, satır içi izinler ve diğer sağlayıcılar korunur; model ve `harness` sağlayıcısı güncellenir. Bozuk veya gizli bilgi başvurusu kullanan satır içi yapılandırma üzerine yazılmaz. HTTPS veya yerel HTTP gerekir; yönlendirmeler izlenmez. API kimlik bilgisi geçicidir; hata mesajlarına yanıt gövdeleri eklenmez. Paperclip kendi araç ve çalıştırma izinlerini yönetir. Kurulum, başlatma, güncelleme veya başka yaşam döngüsü işlemi yapılmaz. Diğer aracı türleri kendi desteklenen OpenAI uyumlu ayarlarıyla elle yapılandırılabilir; Uygula bunların şemasını tahmin etmez.

## Doğrulama

Rust birim testleri yedek/geri yükleme, dış düzenlemeler, token kapsamı/iptali, izin doğrulama, geçmiş kurtarma/silme, süreç hataları, alt süreçleri durdurma, Paperclip ayarlarını koruma ve yönlendirmeyi reddeden yerel GET/PATCH API testini kapsar. `python scripts/test-hermes-adapter.py`, deterministik upstream API test eşlemiyle araç listesini, dosya sınırlarını, yol geçişi/sembolik bağlantı retlerini, genişletilmiş Windows yollarını ve denetim olaylarını denetler. Windows UNC/aygıt yolları ve yeniden ayrıştırma noktaları, izinsiz ağ erişimini önlemek için çözümlemeden önce reddedilir. Gerçek uçtan uca testin komutu [İngilizce belgede](../en/HERMES_AGENT.md#verification) bulunur.

Windows'ta mevcut upstream Hermes v0.14.0, gerçek Harness Gateway kimlik doğrulaması, llama-server ve Qwen3-4B-Instruct-2507 Q4_K_M ile çıkarım, izinli dosya okuma ve gerçek araç denetim kaydı, token kullanımı, geçmişi açma/silme, Stop, iptal edilmiş tokenın reddi, birebir geri yükleme ve iptal geçici klasörlerde doğrulandı. Kullanıcı yapılandırması ve mevcut kurulum değiştirilmedi. Paperclip API'si canlı örnek yerine yerel test sunucusuyla doğrulanır. macOS/Linux ve daha yeni Hermes sürümleri için platform uçtan uca doğrulaması gereklidir; sıfırdan kurulum yapıldığı iddia edilmez.

## 0.2 W3

[Modül Yaşam Döngüsü](MODULE_LIFECYCLE.md), yönetilen kurulumun yeni güncelleme, onarma, geri alma ve açık veri silme seçimlerini açıklar. Yukarıdaki W2 doğrulaması mevcut harici kurulumla yapılan tarihsel testi anlatır; yeni yönetilen kurulumun doğrulaması değildir.
