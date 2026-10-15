# Model Hub ve Kütüphanem

English: [../en/02-model-hub-and-library.md](../en/02-model-hub-and-library.md)

**Bu sayfada ne yapılır:** Uygun bir model dosyası bulunur ve indirilir, Hugging Face erişimi ayarlanır, modeller dönüştürülür veya içe aktarılır ve yerel model tanımları yönetilir.

## Model Arama

1. Sol menüden **Model Hub** sayfasını açın.
2. **Model ara** alanına bir model adı yazıp **Ara** düğmesine basın. Hiçbir şey yazmadan da göz atabilirsiniz: arama boşken liste filtreleri ve **Sırala** düzenini (**Trend olan**, **En çok indirilen**, **En çok beğenilen**, **Son güncellenen**, **En yeni**) izler.
3. **Filtreler** altında **Görev**, **Lisans**, **Yazar** ve **Sırala** seçeneklerini kullanın. Sohbet için **Metin üretimi (LLM)**, konuşmayı yazıya dökmek için **Konuşma tanıma (ASR)** seçin. Görev listesi Hugging Face'in kendi kategorilerini izler ve her görevin Harness Studio'da çalışıp çalışamayacağı işaretlenir. **Karar modeli**, Hugging Face'in karar modeli olarak etiketlediği depoları (evet/hayır, seçim ve puan sorularını olasılıklarla yanıtlayan modeller) listeler; kartlarında **Karar modeli** etiketi bulunur.
4. Sonuçları **Etiket**, **Kuant** ve **Boyut** ile daraltın. Hiçbir şey eşleşmezse **Filtreleri Sıfırla** düğmesine basıp yeniden deneyin.

Varsayılan olarak liste yalnızca GGUF depolarını gösterir; bu, llama.cpp'nin kullandığı dosya biçimidir. Konuşma modelleri için `whisper` arayın (bu, Whisper ggml `.bin` dosyalarını bulur) veya **Konuşma tanıma (ASR)** görevini seçin.

**GGUF olmayan modelleri de göster** seçeneği bu varsayılanı genişletir. Listelenmek, bir modelin çalışabileceği anlamına gelmez. Desteklenmeyen biçimlerin indirmesi kapalıdır; bu yüzden kartlardaki **Burada çalışmaz** ve **Kısmi destek** notlarına dikkat edin. Kart ve ayrıntı paneli, deponun neden çalışamadığını belirtir: desteklenmeyen mimari, CTranslate2 dışa aktarımı, yalnız Apple için bir biçim veya tam modeli gerektiren bir parça gibi. Tanınan `Systran/faster-whisper-*` dışa aktarımlarında **whisper.cpp Sürümünü Aç**, `ggerganov/whisper.cpp` içindeki eşleşen ggml dosyasını açar; CTranslate2 dışa aktarımının kendisi burada çalışmaz. Bazı biçimler isteğe bağlı motorlar üzerinden desteklenir; kart, bir modelin hangisine ihtiyaç duyduğunu söyler:

| Biçim | Neye ihtiyaç duyar? |
| --- | --- |
| Whisper ggml `.bin` | Konuşma modülü (whisper.cpp) |
| ONNX konuşma ve gömme modelleri, ONNX sohbet modelleri | sherpa-onnx veya ONNX Runtime ([Ayarlar](08-ayarlar.md#model-motoru) bölümüne bakın) |
| Difüzyon görsel modelleri (SD1.5, SDXL, SD-Turbo, SD3.5, FLUX paketleri) | Görsel modülü (stable-diffusion.cpp). Bir modelin ihtiyaç duyduğu ek dosyalar (VAE, metin kodlayıcıları) onunla birlikte indirilir. |
| Safetensors LLM'ler | GGUF'a dönüştürün ([aşağıya](#safetensors-modellerini-gguf-biçimine-dönüştürme) bakın) veya isteğe bağlı mistral.rs motorunu kullanın |
| CrispASR / audio.cpp GGUF ses modelleri | CrispASR; deneysel audio.cpp (varsayılan olarak kapalı) |
| MLX biçimli sohbet, görüntü ve ses | MLX motorları (Apple Silicon, macOS 14+) |
| Hugging Face görev klasörleri / diffusers paketleri | Python (`python-hf`, CPU/CUDA); diffusers Görsel Üretimi sekmesinde açılır |

Bir kartta **Gerekli: Konuşma modülü (whisper.cpp)** veya **Gerekli: Görsel modülü (stable-diffusion.cpp)** yazıyorsa modeli yine de indirebilirsiniz, ancak çalıştırmadan önce **Kur** düğmesine (listenin üstünde ya da modelin ayrıntılarında) basmanız gerekir.

**Bir model neden listede yok?** Birçok resmi model deposunda hiç GGUF dosyası yoktur; `meta-llama/Llama-3.2-1B-Instruct` bunun bir örneğidir. Varsayılan Model Hub listesi bu tür depoları göstermez, yalnızca üçüncü kişilerin yayımladığı GGUF kopyalarını gösterir. Bu kopyalar genellikle lisans onayı gerektirmez, ancak onları özgün yayıncı yapmamıştır: indirmeden önce yayıncıyı ve lisansı **Model Kartı** üzerinden kontrol edin.

## Varyant, Kuant ve Bellek

1. Bir deponun **Dosyalar ve kuantlar** bölümünü açın.
2. **Model Kartı** üzerindeki görevi, lisansı ve açıklamayı okuyun.
3. **Dosyalar** tablosunda **Kuant**, **Boyut** ve **Donanım uyumu** sütunlarını karşılaştırın.
4. Önerilen varyanta bakıp **İndir** düğmesine basın. Boyut gösteren düğmelerde metin **İndir ({size})** biçimindedir; `{size}` yerine dosya boyutu görünür.

Kuant, model ağırlıklarının daha derli toplu bir biçimde saklanmasıdır. Filtre **Q4_K_M (önerilen)**, **Q5_K_M (yüksek kalite)**, **Q8_0 (maksimum)** ve **Q4_0 (hafif)** seçeneklerini sunar. Kural olarak sayı küçüldükçe dosya küçülür ve kalite biraz düşer. Emin değilseniz Q4_K_M ile başlayın. Bu etiketler, bir dosyanın bilgisayarınıza sığacağını garanti etmez.

Dosya boyutu ile çalışma belleği aynı şey değildir. Sohbet geçmişi bir KV önbelleği, hesaplama ise geçici alan gerektirir. **Donanım uyumu** bir tahmindir; bağlam büyüdüğünde veya başka modeller yüklendiğinde değişebilir. Bir model zor sığıyorsa önce daha kısa bir bağlam deneyin ([Dashboard bölümüne](03-dashboard-ve-sunucular.md#ayarlar-ne-anlama-gelir) bakın).

Parçalı bir varyantın tüm parçaları gerekir. Kuyruk bunları birlikte indirir; tek bir parçayı seçip çalıştırmaya uğraşmayın. Görsel modeller ayrıca bir projektör (mmproj) dosyası gerektirebilir; model kartını ve dosya eşleşmesini kontrol edin.

## Safetensors Modellerini GGUF Biçimine Dönüştürme

Bazı depolar yalnızca safetensors ağırlıkları içerir. Harness Studio bir depoyu dönüştürebiliyorsa kartında **GGUF’a dönüştürülebilir** etiketi görünür ve **GGUF’a Dönüştür ve İndir** düğmesi sunulur. Panel; mimariyi, parametre sayısını, indirme boyutunu, dönüştürme sonrası boyutu ve gereken ile boş disk alanını gösterir.

1. Kartta veya depo ayrıntılarında **GGUF’a Dönüştür ve İndir** düğmesine basın.
2. Bir **Nicemleme** seçin (Q4_K_M, Q5_K_M, Q6_K, Q8_0, F16 veya BF16) ve indirilen safetensors dosyalarının saklanıp saklanmayacağına karar verin.
3. Dönüştürmeyi başlatın ve **Dönüştürmeler** listesinden izleyin: ortam hazırlanıyor, indiriliyor, GGUF’a dönüştürülüyor, nicemleniyor ve kütüphaneye yerleştiriliyor.

İlk dönüştürme, yönetilen bir Python ile bir dönüştürme ortamı kurar (yaklaşık 700 MB, tek seferlik); Python yalnızca dönüştürmeler sırasında çalışır. Bu ortamı **Ayarlar > Model Motoru > Registry** sayfasında (**GGUF Dönüştürme Ortamı**, Gelişmiş bir sayfa) kurabilir veya silebilirsiniz. Dönüştürme, resmi yönetilen llama.cpp motorunu gerektirir. Yalnızca LoRA adaptörü içeren depolar veya mimarisi kurulu dönüştürücünün bilmediği modeller dönüştürülemez. Görsel modellerde görsel kodlayıcı (mmproj) da dönüştürülür.

## İndirme Kuyruğu

**İndirme Kuyruğu**; indirilen, doğrulanan ve başarısız olan işleri gösterir. İndirmeler uygulama açık olduğu sürece, tepsiye küçültülmüş olsa da devam eder.

| İşlem | Ne yapar? |
| --- | --- |
| **Durdur** | İndirmeyi duraklatır ve inen kısmı korur. |
| **Devam Et** | Duraklatılmış bir indirmeyi sürdürür. |
| **Öne al** | Bekleyen bir indirmeye öncelik verir. |
| **Kaldır** | İşi listeden kaldırır; **Yarım kalan veriyi de sil** ayrı bir seçimdir. |
| **Tamamlananları Temizle** | Tamamlanan işleri kuyruk görünümünden temizler. |

**Doğrulanıyor (SHA-256)** bitmeden bir dosyayı tamamlanmış saymayın. Doğrulama, dosyanın bozulmadan geldiğini denetler. Hugging Face bir dosya için sağlama toplamı vermiyorsa yalnızca boyutu ve biçimi denetlenir; SHA-256 doğrulaması yapılmaz. **Tamamlandı** durumundan sonra dosya birincil model klasörünüzdedir. Bir indirme başarısız olursa [Sorun Giderme](10-sorun-giderme.md) bölümüne bakın.

**Özel URL** ile doğrudan bir bağlantıdan da indirebilirsiniz (yalnızca HTTPS; Harness Studio iç ağlara işaret eden adresleri engeller). Dosya bir GGUF veya Whisper ggml modeli olmalıdır.

Daha önce indirdiğiniz depolarda Hugging Face'te daha yeni bir sürüm varsa **Güncelleme Var** rozeti görünür. Varyant panelinde, indirilmiş bir varyant için güncelleme varsa **Güncelle** sunulur; **Yeniden İndir**, **Gelişmiş** düzeyde kullanılabilir.

## Hugging Face Hesabı: İki Yol

Hesap ayarları **Ayarlar > Depolama > Hugging Face Hesabı** sayfasındadır. Lisans onayı gerektiren (gated) veya özel modelleri indirmek için hesap gerekir; giriş yapmak ayrıca Hugging Face'in anonim kullanıcılara uyguladığı istek sınırını yükseltir. Açık lisanslı modeller için hesap gerekmez.

İki yol olmasının nedeni, basitlik ile rahatlık arasında bir denge kurmalarıdır. Erişim tokenı en basit olandır: tek bir değeri yapıştırırsınız. Tarayıcınız üzerinden giriş (OAuth) token oluşturup kopyalamanızı gerektirmez ve oturumu kendiliğinden yeniler, ancak önce Hugging Face'te bir OAuth uygulaması oluşturup istemci kimliğini girmeniz gerekir. Çoğu kullanıcı için token yeterlidir.

### Basit Yol: Erişim Tokenı

1. [Hugging Face token sayfasında](https://huggingface.co/settings/tokens) oturum açın (**Token Sayfasını Aç** düğmesi aynı adresi açar).
2. **Read** yetkisine sahip bir token oluşturun.
3. Harness Studio'da **Ayarlar > Depolama > Hugging Face Hesabı** sayfasını açın.
4. Değeri **Erişim tokenı** alanına yapıştırın ve yanındaki **Kaydet** düğmesine basın.
5. **Hesap** satırında **Erişim tokenı ile bağlı** yazdığını kontrol edin. Token işletim sisteminizin anahtarlığında saklanır ve varsa mevcut OAuth girişinin yerini alır. Silmek için **Sil** düğmesini kullanın.

### Tarayıcıyla Giriş: OAuth

1. Aynı sayfada **Gelişmiş: OAuth Uygulaması** bölümünü açın. Bu yol, bir OAuth uygulamasının istemci kimliğini gerektirir.
2. **Uygulama Oluştur Sayfasını Aç** düğmesine basın ve Hugging Face'te gizli anahtarı olmayan (public) bir OAuth uygulaması oluşturun. Uygulama şu değerleri ister:
   - Yönlendirme adresi (redirect URI): `http://127.0.0.1/callback`
   - Kapsamlar (scopes): `openid profile read-repos gated-repos`

   Uygulamanın aynı bölümü bu değerleri gösterir; orada gösterilenleri kullanın.
3. İstemci kimliğini **İstemci kimliği (client ID)** alanına girip **Kaydet** düğmesine basın.
4. **Hugging Face ile Giriş Yap** düğmesine basın ve tarayıcıda onaylayın. Beklerken **İptal** düğmesine basabilirsiniz; 5 dakika içinde tamamlamazsanız giriş zaman aşımına uğrar.
5. Uygulamaya dönünce **Hesap** satırında **Hugging Face ile giriş yapıldı** yazdığını kontrol edin.

**Çıkış Yap** yalnızca tokenı bu bilgisayardan siler. Uygulamanın hesabınıza erişimini tamamen kaldırmak için huggingface.co'daki bağlı uygulamalar sayfasından erişimi iptal edin (**Bağlı Uygulamaları Aç** bu sayfayı açar).

İstemci kimliğiniz yoksa erişim tokenını kullanın. Bu kimlik bilgilerinin nasıl işlendiği için [güvenlik modeline](../../tr/SECURITY_MODEL.md) bakın.

## Lisans Onaylı Bir Model İndirme

Lisans onaylı (gated) bir model örneği `google/gemma-3-1b-it-qat-q4_0-gguf` deposudur. Bu, Google'ın resmi GGUF deposudur ve elle lisans onayı gerektirir. Tek bir dosya içerir: `gemma-3-1b-it-q4_0.gguf` (Q4_0, yaklaşık 1 GB).

1. Önce yukarıdaki yollardan biriyle hesabınızı bağlayın.
2. **Model Hub** içinde depo adını arayın ve **Dosyalar ve kuantlar** bölümünü açın.
3. Kartta **Lisans onayı gerekiyor** görünür. Model sayfasını açmak için **Lisansı huggingface.co'da Kabul Et** düğmesine basın.
4. Model sayfasında, uygulamaya bağladığınız hesapla oturum açın ve lisans koşullarını kabul edin. Elle onaylanan modellerde model sahibini beklemeniz gerekebilir.
5. Harness Studio'ya dönüp **Tekrar Dene** düğmesine basın. **Lisans kabul edildi, erişiminiz var** yazısını görünce dosyayı **İndir** ile indirin.

Erişiminiz yokken **Yine de Ekle** indirmeyi kuyruğa alır ama izin vermez; büyük olasılıkla başarısız olur. Token eklemek, lisansı kabul etmenin yerini tutmaz.

Lisansı yeni onaylanmış modellerde Hugging Face bazen maskelenmiş bir sağlama toplamı döndürür. Uygulama maskelenmiş sağlama toplamını geçersiz sayar ve dosyayı doğrularken güncel tokenınızla yenisini alır. Bir indirme yine de SHA-256 hatasıyla biterse sağlama toplamı gerçekten eşleşmiyordur: bozuk dosya silinir, indirmeyi yeniden başlatın.

## Kütüphanem ve Model Tanımları

1. Sol menüden **Kütüphanem** sayfasını açın. Soldaki listede **Modeller** grubu (**Tümü**, **Sohbet**, **Görsel Anlama**, **Embedding**, **Reranker**, **Konuşma**, **Görsel Üretimi**, **Seslendirme**), **Yardımcı Dosyalar** grubu (**Draft**, **Adapter**, **Projektör**) ve **Yönetim** grubu (**İçe Aktarma**, **Model Tanımları**) bulunur. Yardımcı dosya sayfaları ve **Model Tanımları** Gelişmiş düzeydeki sayfalardır.
2. Bir dosya görünmüyorsa **Kütüphaneyi Yeniden Tara** düğmesine basın. Diğer klasörlerdeki modelleri **Ayarlar > Depolama > Dizinler** altından ekleyebilirsiniz.
3. Bir model kartında boyuta, kuanta, bağlama, donanım tahminine ve eksik parça uyarısına bakın. Listeyi favoriler ve son kullanımla düzenleyin. Sohbet modeli kartı **Yükle ve Sohbet Et** seçeneğini sunar. Görsel, konuşma, ses, embedding veya reranker kartı ise **Playground'da Aç** seçeneğini sunar; bu, ilgili sekmeyi dosya seçili olarak açar. Bu, Model Hub'dan indirilmemiş, klasörlerinizde zaten bulunan dosyalar için de çalışır; önce bir tanım oluşturmanız gerekmez.
4. Dosyanın **Tanımlar (Alias)** bölümünü kontrol edin. **Yükle ve Sohbet Et** veya **Playground'da Aç**'a bastığınızda tanım sizin için oluşturulur; ayrıca Playground sekmeleri taranmış görsel, konuşma, embedding ve reranker dosyalarını kendiliğinden listeler. Kendi ayarlarınızla ek bir tanım istiyorsanız **Yeni Tanım**'ı seçin.
5. Bir **Takma ad (alias)** ve gerekirse bir **Varsayılan preset** belirleyin.

Bir model tanımı; bir model dosyasını, onunla kullanılan yükleme ayarlarını (bağlam, GPU katmanları, KV önbelleği, sohbet şablonu), yardımcı dosyaları (LoRA adaptörü, taslak model, görsel projektör) ve varsayılan bir preseti tek bir ad altında bağlar. Aynı dosyanın farklı amaçlar için farklı tanımlarını oluşturabilirsiniz; örneğin biri uzun bağlamlı, biri hızlı olabilir. **Takma ad (alias)**, başka programların kullandığı model adıdır. Bir harf ya da rakamla başlamalı ve yalnızca harf, rakam ile `. _ - : /` karakterlerini içermelidir.

Bir tanımı silmek model dosyasını silmez. Bir takma adı değiştirirseniz eski adı kullanan istemcilerin ayarlarını **Bağlantılar** üzerinden yeniden uygulayın. Tanımlar dışa ve içe aktarılabilir (`.harness.toml`) ve bir Ollama Modelfile'dan oluşturulabilir.

**İçe Aktarma** sayfası, Ollama ve LM Studio modellerini kütüphaneye bağlamanızı veya kopyalamanızı sağlar. Bağlı modeller taşınamaz veya silinemez; yalnızca bağlantı kaldırılır. Silme önizlemesini okuyun ve kullanımdaki bir modeli silmeden önce sunucuyu durdurun. Eksik parçası olan bir model yüklenemez; sunulduğunda **Onar ({repo})** işlemini kullanın.

Ayrıca [Özellikler başvurusuna](../../tr/FEATURES.md) ve [güvenlik modeline](../../tr/SECURITY_MODEL.md) bakın.

Sonraki: [Dashboard ve Sunucular](03-dashboard-ve-sunucular.md).
