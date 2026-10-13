# Kurulum ve İlk Açılış

English: [../en/01-installation.md](../en/01-installation.md)

**Bu sayfada ne yapılır:** Uygulama kurulur, işletim sisteminin ilk açılış uyarısı aşılır, kurulum sihirbazı tamamlanır ve modellerin saklanacağı klasör seçilir.

## Kurulum Paketini Edinme

Harness Studio 0.1.0, projenin [GitHub Releases](https://github.com/struklabs/harness-studio/releases) sayfasında Windows kurulum paketi olarak dağıtılır. Uygulamayı kendiniz derlemek isterseniz [ana README](../../../README.tr.md) içindeki derleme bölümüne bakın; bu sayfa hazır kurulum paketiyle kurulumu anlatır.

| Sistem | Paket | Durum |
| --- | --- | --- |
| Windows 10/11 (x64) | NSIS kurulum paketi (`.exe`) | Mevcut. Kurulum paketi henüz kod imzalı değil. |
| Linux | `.deb` / AppImage | Yakında. Paketler üretiliyor ancak henüz gerçek makinelerde doğrulanmadı. |
| macOS | DMG | Yakında. Paketler üretiliyor ancak henüz gerçek makinelerde doğrulanmadı. |

### Windows

1. `.exe` kurulum paketini çalıştırın ve adımlarını izleyin.
2. Windows SmartScreen bir uyarı gösterirse **Ek bilgi**, ardından **Yine de çalıştır** seçeneğine basın. Uyarı, kurulum paketi henüz kod imzalı olmadığı için çıkar.
3. Harness Studio'yu Başlat menüsünden açın.

Kaldırma işlemi kullanıcı verilerinizi silmez; veriler [aşağıda](#model-klasörü-ve-veri-dizini) anlatılan veri klasöründe kalır. `HKCU\Software\harnessstudio` altında kalan kayıt yalnızca kurulum konumunu hatırlar; zararsızdır ve silinebilir.

### Uygulamayı Güncelleme

Yeni sürümler, GitHub Releases'ten en son kurulum paketini indirip çalıştırarak kurulur. Ayarlar'daki **Güncellemeler** sayfası uygulama sürümünü gösterir ve daha yeni bir sürüm olup olmadığını denetleyebilir. llama.cpp motoru ayrıca güncellenir; bunu **Ayarlar > Model Motoru > Kurulu Model Motoru** sayfasında yönetirsiniz.

## İlk Açılış: Beş Adım

Uygulamayı ilk başlattığınızda **Kurulum sihirbazı** açılır. **Devam** ile ilerleyin.

1. **Donanım:** İşlemcinizi, RAM'inizi, ekran kartınızı ve önerilen hızlandırmayı (backend) inceleyin.
2. **Depolama:** **Model Klasörü** alanını kontrol edin. Başka bir sürücü kullanmak için **Değiştir** düğmesine basıp bir klasör seçin. Yeni modeller buraya iner. Büyük modeller onlarca GB tutabilir; yeterli boş alan bırakın.
3. **Motor:** Sisteminizde uygun bir llama.cpp varsa **Sistemdekini Kullan** seçilebilir. Yoksa **En Son Sürümü İndir ve Kur** düğmesine basın. İndirme ve doğrulama bitip **llama.cpp hazır.** yazısını görünce ilerleyin.
4. **Amaç:** **Sohbet**, **Kod ajanı**, **Belge / RAG** veya **Ses yazıya çevirme** seçeneğini belirleyin. Sihirbaz donanımınıza uygun bir motor ve modeller önerir. Önerileri kurabilir veya **Atla** seçebilirsiniz. Model indirmeleri arka planda sürer; **Model Hub** indirme kuyruğundan izleyin. Bu adım ayrıca Harness Studio'yu nasıl kullanmak istediğinizi de sorar: **Basit** (önerilir) veya **Gelişmiş** ([Basit ve Gelişmiş düzeyler](README.md#basit-ve-gelişmiş-arayüz-düzeyleri) bölümüne bakın).
5. **Gizlilik:** **Çökme ve hata raporlarını yerel kaydet** ve **Donanım bilgisini sakla** tercihlerini inceleyip **Kurulumu Bitir** düğmesine basın. Raporlar bu bilgisayarda kalır ve hiçbir yere otomatik gönderilmez.

Motor kurulumu veya **Amaç** kurulumu sürerken sihirbazın adımları arasında gezinemezsiniz; bitmesini bekleyin. Model indirmeleri kuyruğa alınır ve arka planda sürer, bitmesini beklemeniz gerekmez.

Sihirbazı daha sonra tekrar çalıştırmak için **Ayarlar > Model Motoru > Kurulu Model Motoru** sayfasındaki **Sihirbazı Aç** düğmesini kullanın.

## Model Motoru Seçimi

Model motoru, model dosyasını çalıştıran motordur. Sohbet llama.cpp kullanır.

| Seçenek | Ne zaman kullanılır? |
| --- | --- |
| **Yönetilen (önerilen)** | Çoğu kullanıcı için. Harness Studio resmi sürümü indirir, doğrular ve güncel tutar. |
| **Özel İkili** | Kendi `llama-server` dosyanız varsa. Konumunu **llama-server yolu** alanına yazıp **Doğrula** düğmesine basın. Özel ikili otomatik olarak güncellenmez veya geri alınmaz. |

Önerilen hızlandırma donanımınıza bağlıdır: NVIDIA için CUDA, uygun AMD kartlar için ROCm/HIP veya Vulkan, Intel Arc için SYCL veya Vulkan, Apple Silicon için Metal. GPU yoksa CPU kullanılır. Bir ekran kartının algılanması, seçilen derlemenin onu kullanabileceğini garanti etmez. Kurulu derleme kartı göremezse Harness Studio her kartta çalışan bir derlemeye (örneğin Vulkan) geçer ve bunu **Ayarlar > Model Motoru > Kurulu Model Motoru** sayfasında size bildirir.

Diğer model türleri, ihtiyaç duyduğunuzda kurduğunuz isteğe bağlı motorları kullanır. Bunlar tek tıkla kurulur: Kütüphane kartlarında, Model Hub'da ve Playground hatalarında "Bu model için … motoru gerekli" notunun yanında bir **Kur** düğmesi görünür; tüm motorlar **Ayarlar > Model Motoru > Kurulu Model Motoru** sayfasındaki **Diğer Model Motorları** listesinde de vardır (iki arayüz düzeyinde de). Ek derlemeler **Ayarlar > Model Motoru > Registry** sayfasından (Gelişmiş bir sayfa) eklenebilir:

| Yetenek | Motor |
| --- | --- |
| Konuşmayı yazıya dökme | whisper.cpp (Whisper modelleri), sherpa-onnx (Parakeet, SenseVoice, Omnilingual) ve ONNX Runtime (wav2vec2, HuBERT, WavLM CTC) |
| Metni konuşmaya çevirme | sherpa-onnx (Kokoro ve Piper sesleri) |
| Görsel üretimi | stable-diffusion.cpp |
| ONNX gömme, yeniden sıralama ve sohbet | ONNX Runtime ve ONNX Runtime GenAI (birlikte gelen `harness-onnx` yardımcısı üzerinden) |
| Safetensors modellerini GGUF'a dönüştürme | İlk kullanımda kurulan küçük bir dönüştürme ortamı |
| AMD Ryzen AI NPU ve ek llama.cpp derlemeleri | Lemonade (isteğe bağlı) |
| Dönüştürmeden Safetensors modelleri | mistral.rs (isteğe bağlı, kendi derlemenizi ekleyin) |

Bunlar [Model Hub ve Kütüphanem](02-model-hub-ve-kutuphane.md), [Playground](04-playground.md) ve [Ayarlar](08-ayarlar.md) bölümlerinde anlatılır.

## Model Klasörü ve Veri Dizini

**Ayarlar > Depolama > Dizinler** sayfasında iki bölüm vardır:

- **Birincil Model Klasörü**: yeni indirmelerin kaydedildiği yer. **Klasör Seç** ile değiştirilir.
- **Ek Klasörler**: başka sürücülerdeki mevcut modelleri kütüphaneye ekler. Bir klasör yolu yazıp **Ekle** düğmesine basın veya **Gözat** ile seçin. Her klasörün yanında erişilebilir olup olmadığı, boş alanı ve kaç model içerdiği görünür.

Uygulama verileri (ayarlar, sohbet veritabanı, motorlar ve günlükler) model klasöründe değil, ayrı bir veri klasöründe tutulur.

| Sistem | Varsayılan veri klasörü |
| --- | --- |
| Windows | `%APPDATA%\com.harnessstudio.app` |
| macOS | `~/Library/Application Support/com.harnessstudio.app` |
| Linux | `$XDG_DATA_HOME/com.harnessstudio.app`; değişken tanımlı değilse `~/.local/share/com.harnessstudio.app` |

Veri klasörünü komut satırından değiştirmek için [CLI başvurusuna](../../tr/CLI.md) bakın. Kimlik bilgilerinin nasıl saklandığı için [güvenlik modeline](../../tr/SECURITY_MODEL.md) bakın. Sıfırdan başlamak için uygulamayı kapatın, bu klasörü yedekleyin ve silin; ayarlarınız ve sohbetleriniz de onunla birlikte silinir.

Sonraki: [Model Hub ve Kütüphanem](02-model-hub-ve-kutuphane.md).
