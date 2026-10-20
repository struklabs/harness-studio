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

Algılama PATH ve standart kaynak kurulum yollarını, süre sınırlı sürüm sorgusunu ve Python API çalışma zamanını denetler. Sürüm kısa süre önbelleğe alınır. Güncel PM kaynak başlatıcısı ve eski sanal ortamlar desteklenir. Kaynak/API çalışma zamanı bulunamayan yalnızca masaüstü kurulumları eksik bağımlılık olarak gösterilir; resmî kaynak kurulumunu kullanın. Güncellemeler upstream'e aittir; kaynak kurulumunda `hermes update` kullanılır.

## Güvenlik ve Kurtarma

Bağlan, yerel Gateway adresi ve model önizlemesinden sonra yalnızca `inference` kapsamı, seçilen tek model ve sohbet/model listesi uç noktaları için token oluşturur. Kontrol, araç, kabuk veya admin yetkisi verilmez. Anahtar IPC'ye veya süreç argümanlarına girmez; Hermes'in özel yapılandırmasında bulunur. Harness mevcut tuzlanmış token özeti deposunu kullanır. Yerel Gateway tokensız erişime açıksa diğer yerel istemciler kısıtları aşabilir; tüm yerel bağlantıları kısıtlamak için token gereksinimini açın.

Yapılandırmanın özgün baytları özel `config.yaml.harness.bak` dosyasına kaydedilir. Atomik değiştirmeden önce özel `harness-connection.json` kurtarma kaydı yazılır. Bozuk YAML ve mevcut yedek reddedilir; ilgisiz ayarlar korunur. Bağlantıyı Kes önce tokenı iptal eder, sonra özgün dosyayı geri yükler veya daha önce olmayan yapılandırmayı siler. Dışarıdan düzenlenen dosyanın üzerine yazılmaz; yedek ve kurtarma kaydı korunur. Yedeği inceleyip özgün baytları `config.yaml` dosyasına geri yükleyin ve Bağlantıyı Kes işlemini yeniden deneyin. Kesinti sonrası sahipsiz kalmış yedek yeniden bağlanmadan önce incelenmelidir.

Görev ayrı bir geçici Hermes klasörü, açık yerel model kimlik bilgileri ve temizlenmiş kimlik ortamı kullanır. Kullanıcı bağlamı, bellek, beceriler, kancalar ve MCP yapılandırması yüklenmez; `enabled_toolsets=[]` uygulanır ve buna rağmen araç açılırsa görev reddedilir. Bu, uygulama düzeyinde bir araç kısıtıdır; güvenilen upstream Python kodunu işletim sistemiyle yalıtmaz. Diğer Hermes arayüzlerinin kendi izin kuralları geçerlidir. Çıktı 64 KiB ile sınırlıdır; token gizlenir. Görev en fazla on dakika, iki yineleme ve 512 üretilen token sürer. Harness görev durumunu ve çıktısını kalıcı saklamaz.

## Doğrulama

Rust birim testleri yedek/geri yükleme, olmayan/bozuk yapılandırma, dış düzenlemeler, token kapsamı/iptali, algılama, süreç hataları ve durdurmayı kapsar. Gerçek uçtan uca testin komutu [İngilizce belgede](../en/HERMES_AGENT.md#verification) bulunur.

Windows'ta mevcut upstream Hermes v0.14.0, gerçek Harness Gateway kimlik doğrulaması, llama-server ve Qwen3-4B-Instruct-2507 Q4_K_M ile çıktı, Stop, birebir geri yükleme ve iptal geçici klasörlerde doğrulandı. Kullanıcı yapılandırması ve mevcut kurulum değiştirilmedi. macOS/Linux ve daha yeni Hermes sürümleri için platform uçtan uca doğrulaması gereklidir; sıfırdan kurulum yapıldığı iddia edilmez.
