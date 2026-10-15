# Playground

English: [../en/04-playground.md](../en/04-playground.md)

**Bu sayfada ne yapılır:** Yerel bir modelle sohbet edilir, sohbetler düzenlenir, belgeler ve araçlar kullanılır, iki yanıt karşılaştırılır, embedding ve rerank ile çalışılır, konuşma yazıya, yazı da konuşmaya çevrilir ve görseller üretilir.

## Ekranın Yapısı

Playground'un üstünde mod sekmeleri (**Playground modu**) bulunur: **Sohbet**, **Embedding**, **Rerank**, **Konuşma**, **Görsel Üretimi** ve **Görevler**. **Basit** arayüz düzeyinde **Sohbet**, **Konuşma** ve **Görsel Üretimi** görünür; **Embedding**, **Rerank** ve **Görevler** **Gelişmiş** düzeyde açılır. Sohbet modunda solda sohbet listesi (**Sohbetleri göster**), ortada konuşma ve **Mesajınız** alanı, sağda **Oturum Ayarları** paneli (**Ayarları göster**) yer alır. Oturum Ayarları şu sayfalara ayrılır:

| Grup | Sayfa | Ne için? |
| --- | --- | --- |
| Oturum | **Model ve Preset** | Model seçimi ve bir preset (kayıtlı ayar takımı) seçimi. |
| Oturum | **Sistem İstemi** | Modele verilen genel yönerge. |
| Üretim (Gelişmiş) | **Örnekleme**, **Çıktı Sınırları** | Yaratıcılık ve yanıt uzunluğu. |
| Bağlam (Gelişmiş) | **Geçmiş ve Compaction**, **RAG** | Uzun sohbetleri özetleme ve belge arama. |
| Araçlar | **Seçim ve İzinler** | Modelin kullanabileceği araçlar ve çalışma klasörü. |

## İlk Sohbetiniz

1. [Dashboard'dan bir sohbet sunucusu başlatın](03-dashboard-ve-sunucular.md) veya ağ geçidini açıp bir model tanımı seçin.
2. **Playground > Sohbet** sekmesini açın. Model seçiciyi kontrol edin. Ağ geçidini kullanıyorsanız model ilk istekte yüklenebilir; bu birkaç saniyeden birkaç dakikaya kadar sürebilir.
3. İsteğinizi **Mesajınız** alanına yazın ve **Gönder** düğmesine basın.
4. Üretimi yarıda kesmek için **Durdur** düğmesini kullanın.
5. Yanıtın altında token sayısını, üretim hızını ve ilk token süresini inceleyin. Token, bir kelime ya da kelimenin bir parçasıdır.

**Sunucu çalışmıyor** uyarısını görürseniz sunucuyu kontrol etmek için **Dashboard'a Git** düğmesini kullanın. Embedding veya rerank modunda başlatılmış bir sunucu sohbeti yanıtlamaz; banner'daki **Embedding Sekmesini Aç** / **Rerank Sekmesini Aç** bağlantılarını kullanın.

Mesaj kutusunda ayrıca görsel destekli modeller için bir **Görsel Ekle** düğmesi (PNG, JPEG, WebP ve GIF; görselleri sürükleyip bırakabilir veya yapıştırabilirsiniz; bu modellerin projektör (mmproj) dosyasının yüklü olması gerekir), yazmak yerine konuşmanızı sağlayan bir **Dikte** düğmesi ve modele web aramasını ile seçtiğiniz araçları sunan bir **Web ve Araçlar** anahtarı bulunur.

## Sohbet Listesi ve Klasörler

İlk mesajınızı göndermek sohbet kaydını oluşturur; kayıt yerel bir veritabanında saklanır. Listeyi **Sohbetleri göster** ile açın; eski bir konuşmayı **Sohbetlerde ara** ile bulun.

1. **Yeni Sohbet** ayrı bir konuşma başlatır.
2. Bir sohbetin işlem menüsünden **Yeniden Adlandır**, **Sabitle**, **Favorilere ekle** veya **Kopyasını Oluştur** seçeneğini seçin.
3. **Yeni Klasör**, sohbetleri düzenlemek için bir klasör oluşturur; bir sohbeti **Klasöre Taşı** ile (ya da bir klasörün üzerine sürükleyerek) yerleştirin.
4. **Dışa Aktar** JSON veya Markdown kaydeder. **Sohbet İçe Aktar (JSON)** kayıtlı bir konuşmayı ekler.

Sohbet klasörü, dosya araçları için bir çalışma klasörü değildir (bkz. [Araçları Kullan ve Onayları Gözden Geçir](#araçları-kullan-ve-onayları-gözden-geçir)). Bir sohbet klasörünü silmek içindeki sohbetleri silmez; bir sohbeti **Sil** ile kaldırmak ise onu dalları ve yanıt sürümleriyle birlikte kalıcı olarak siler.

Farklı bir yanıt almak için **Yanıtı Yeniden Oluştur** seçeneğini kullanın; bu, geçmişin herhangi bir yerindeki yanıt için de çalışır. Yeni yanıt o mesajın yeni bir sürümü olur, sonraki mesajlar eski sürümde kalır ve **< 1/3 >** okları her sürümün kendi devamını gösterir. Daha önceki bir mesajınızı değiştirmek için **Düzenle ve Yeniden Gönder** simgesini kullanın: metin yerinde düzenlenir, kaydettiğinizde yanıt oradan yeniden üretilir ve eski devam bir yan dal olarak saklanır. **Buradan Dallandır**, o mesaja kadar olan geçmişi yeni bir sohbete kopyalar.

Yanıtlar Markdown'ı tam olarak (listeler, tablolar, başlıklar) işler; `mermaid` kod blokları diyagram olarak görünür, bunları kaynağa çevirebilir, kopyalayabilir veya SVG olarak kaydedebilirsiniz. Bir yanıttaki **Sesli Oku** düğmesi yanıtı sesli okur (bkz. [Metinden Konuşmaya](#metinden-konuşmaya)).

## Sistem İstemi, Presetler ve Örnekleme

1. **Ayarları göster** ile **Oturum Ayarları** panelini açın.
2. **Sistem İstemi** sayfasına modelin izlemesini istediğiniz genel yönergeyi yazın. Örnek: "Kısa cümlelerle yanıt ver."
3. **Örnekleme** sayfasında **Sıcaklık (temperature)**, **Top P**, **Top K**, **Min P**, **Tekrar cezası** ve **Seed** alanlarını düzenleyebilirsiniz. Değiştirmeniz gerekmiyorsa varsayılanları kullanın; boş alanlar isteğe eklenmez.
4. **Çıktı Sınırları** sayfasında **Maks. token** ile yanıtın üst sınırını belirleyin. Çıktıya yer kalması için **Bağlam kullanımı** göstergesini izleyin.

Ne işe yaradıkları: **Sıcaklık** yükseldikçe yanıtlar daha çeşitli ve öngörülemez, düştükçe tutarlı ve tekrara yatkın olur. **Top P**, **Top K** ve **Min P**, modelin hangi kelime adayları arasından seçim yapacağını daraltır. **Tekrar cezası** tekrarlanan ifadeleri azaltır. **Seed** sabit olduğunda aynı istem benzer bir yanıt verir.

**Preset**, bu istek parametrelerini (örnekleme, cezalar, akıl yürütme, çıktı sınırı) ve isteğe bağlı bir sistem istemini tek bir ad altında toplar. **Model ve Preset** sayfasındaki **Preset** seçicisinden seçilir. Yerleşik presetler değiştirilemez; bir tanesini çoğaltıp düzenlemek için **Kopyala**'yı kullanın. **Presetleri Yönet** yeni presetler oluşturur, içe ve dışa aktarır. Bir model tanımına **Varsayılan preset** atarsanız o modelle açılan yeni sohbetlerde bu preset seçili gelir.

Modelin her kelimede ne kadar emin olduğunu görmek için bir presetin örnekleme ayarlarında **Olasılık günlüğü** seçeneğini açın (kaç **Alternatif sayısı** döndürüleceğini de ayarlayabilirsiniz). Yanıtlarda **Belirteç olasılıklarını göster** seçeneği belirir: her token renklendirilir (soluk renk emin, belirgin renk emin değil anlamına gelir) ve üzerine gelince seçilen token ile alternatifleri görünür. **Normal görünüme dön** düz metne döndürür. Olasılıklar yalnızca geçerli oturum boyunca saklanır.

Bağlam dolduğunda model eski mesajları unutabilir veya istek reddedilebilir. Yeni bir sohbet başlatın ya da **Geçmiş ve Compaction** sayfasındaki bağlam sıkıştırmayı kullanın: bir sohbet bağlamı doldurmaya yaklaştığında eski mesajlar modelin yazdığı bir özetle değiştirilir, son turlar olduğu gibi kalır, asıl mesajlar silinmez ve sıkıştırma **Geri Al** ile geri alınabilir. Mesaj kutusuna `/compact` yazarak elle de sıkıştırabilirsiniz; ayrıntılar için [sıkıştırma başvurusuna](../../tr/COMPACTION.md) bakın. Daha büyük bir bağlam için sunucunun yükleme ayarlarını değiştirin; bu daha fazla bellek ister.

## İki Yanıtı Karşılaştırma

Karşılaştırma, aynı mesajı iki modele veya aynı modelin iki presetine gönderir ve yanıtları yan yana gösterir.

1. **Karşılaştır**'ı açın (kısayol: Ctrl+Shift+M).
2. A ve B için modelleri veya presetleri seçin. Ağ geçidi modelleri (ilk istekte yüklenenler dahil) ve çalışan sunucular ayrı gruplarda görünür.
3. Aynı mesajı gönderin. Yanıtları, süreleri, kelime sayılarını, benzerlik oranını ve hız farkını görürsünüz; **Farkları Göster** kelime düzeyindeki farkları işaretler.
4. İki model birlikte belleğe sığmıyorsa ya da ağ geçidi aynı anda yalnızca bir model tutuyorsa uygulama kendiliğinden sırayla çalıştırmaya geçer ve nedenini söyler. Bunu zorlamak için **Sırayla çalıştır** seçeneğini açın.
5. Seçtiğiniz tarafla devam etmek için **Bu yanıtı sohbete al ({side})** düğmesine basın; `{side}` yerinde A veya B görünür. Karşılaştırma kapanır ve diğer taraf bir yanıt sürümü olarak kalır.

Karşılaştırma sırasında belge araması kullanılmaz. Araçlar açıksa her taraf kendi araç döngüsünü çalıştırır; her çağrının izinlerini ayrı ayrı gözden geçirin.

## Araçları Kullan ve Onayları Gözden Geçir

Araç, modelin metin üretmenin dışında yaptığı bir işlemdir; örneğin bir dosyayı okumak, bir komut çalıştırmak veya web'de arama yapmak. Araçlar varsayılan olarak onay ister.

1. **Oturum Ayarları > Seçim ve İzinler** sayfasında **Araç kullanımını etkinleştir** seçeneğini açın.
2. Modele hangi araçların sunulacağını seçin (**Gruptaki tüm araçlar** bir grubu tek seferde açıp kapatır). Modelin sohbet şablonu araç çağrısını desteklemelidir; desteklemeyen bir model araç çağırmaz.
3. Dosya veya komut araçları için **Çalışma klasörü** altından **Klasör Seç** ile bir klasör belirleyin (giriş alanındaki klasör düğmesi de aynı işi yapar). Bu seçim o sohbete aittir. **Ayarlar > Araçlar > Yerleşik Araçlar** içindeki **Varsayılan klasör**'ü de kullanabilirsiniz. Dosya araçları yalnızca bu klasörün içinde çalışır; üst klasöre, mutlak bir yola veya sembolik bağa erişemezler. Komut aracı (`exec_shell_command`) komutu bu klasörde başlatır ama klasörle sınırlı değildir: bir komut, kullanıcı hesabınızın bilgisayarda yapabildiği her şeyi yapabilir (tehlikeli sınıftadır). Onaylamadan önce komutu dikkatle okuyun. Klasör yoksa dosya ve komut araçları hata verir.
4. Model bir araç çağırdığında araç kartındaki **Parametreler** bölümünü okuyun. İşlemi kabul etmek için **Onayla**, reddetmek için **Reddet** düğmesine basın.

Onay, aracın bilgisayarınızda çalışmasına izin verir. Dosya yazma ve komutlarda hedefi ve içeriği kontrol edin. Politikayı ve güvenilir araçları [Araçlar bölümünden](06-araclar.md) ayarlayın. Ağ araçları sorgularını veya isteklerini dış servislere gönderir.

## Belge Ekleme

1. **Belge ekle** ile bir PDF, DOCX, Markdown, metin veya kod dosyası seçin ya da dosyayı sohbete sürükleyin.
2. Okuma ve dizinlemenin bitmesini bekleyin (**okunuyor**, parça sayacıyla).
3. Belge hakkında somut bir soru gönderin.
4. **Yanıtın kaynakları** altındaki alıntıları ve sayfa bilgilerini kontrol edin; yanıtın atıf yaptığı alıntı işaretlenir.

Belgenin yanındaki **Kullanım** seçimi belgenin nasıl kullanılacağını belirler. **Otomatik**'te bağlama rahatça sığan bir belge tam metin olarak eklenir; büyük belgelerde her soruda ilgili parçalar aranır. Bunu **Tam Metin** ve **Arama** ile değiştirebilirsiniz. **Oturum Ayarları > RAG** sayfasında bir **Embedding modeli** seçilmemişse yalnızca anahtar kelime araması yapılır; anlamsal arama için Model Hub'dan küçük bir embedding modeli (örneğin nomic-embed-text) indirin. İsteğe bağlı **Yeniden sıralama**, küçük bir hız kaybıyla bir reranker modeli kullanarak sonuçları daha isabetli sıralar. **Soru başına parça**, arama modunda yanıta eklenen alıntı sayısıdır. Belge işleme ve indeksler yerel çalışır. Belge araması karşılaştırma görünümünde kullanılmaz.

## Embedding ve Rerank

**Embedding**, metni sayısal bir vektöre çevirir; arama ve benzerlik için kullanılır. Uygun bir embedding modeliyle bir sunucu başlatın (model türü embedding olan bir sunucu bu modda kendiliğinden başlar), **Embedding** sekmesindeki **Metinler** kutularını doldurun ve **Embedding Al**'a basın. Benzerlik matrisi için boş olmayan en az iki metin gerekir.

**Rerank**, belgeleri bir soruyla eşleşme düzeylerine göre sıralar. Uygun bir reranker modeli başlatın, **Sorgu** ve **Belgeler** alanlarını doldurun ve **Sırala**'ya basın. Puanları yalnızca aynı sorgunun belgeleri arasında karşılaştırın.

## Konuşma: Yazıya Dökme ve Sesli Okuma

**Konuşma** sekmesi her iki yönü de kapsar. Ses, ağ geçidi üzerinden gider; kapalıysa **Gateway'i Aç** düğmesini kullanın. Modeller ilk istekte yüklenir.

### Konuşmayı Yazıya Dökme

1. **Model Hub**'da `whisper` aratın ve bir Whisper ggml modeli indirin. Klasörlerinizde zaten bulunan bir `ggml-*.bin` dosyası **Model** listesinde kendiliğinden görünür. Türkçe veya başka diller için yalnızca İngilizce bilen bir model (`.en` modeli) seçmeyin.
2. whisper.cpp motorunu kurun: Kütüphane kartında, Model Hub'da ya da bu sekmedeki hata mesajında görünen **whisper.cpp Kur** düğmesine basın; **Ayarlar > Model Motoru > Kurulu Model Motoru** sayfasındaki **Diğer Model Motorları** listesinden de kurabilirsiniz. macOS'ta hazır bir whisper sunucu paketi yoktur; kendi derlediğiniz bir `whisper-server`'ın klasörünü ekleyebilirsiniz.
3. **Playground > Konuşma** sekmesini açın.
4. Bir **Model** seçin. **Dosya Seç** ile ses ekleyin ya da **Ses Kaydet** ile mikrofon kaydını başlatıp **Durdur** ile bitirin. İşletim sistemi mikrofon izni isteyebilir.
5. **Dil** alanına `tr` gibi bir dil kodu yazabilir veya otomatik bırakabilirsiniz. **İş** için **Yazıya dök** ya da **İngilizceye çevir** seçin, sonra **Yazıya Dök**'e basın.
6. Sonuca bakın. **Sohbete Ekle** metni sohbet taslağına koyar; göndermek için yine **Gönder**'e basmanız gerekir.

Whisper'ın yanı sıra **Önerilen sherpa-onnx Tanıma Modelleri** bölümünden NVIDIA Parakeet (25 Avrupa dili), SenseVoice (Çince, İngilizce, Japonca, Korece, Kantonca) ve Omnilingual (Türkçe dahil 1600 dil) modellerini indirebilirsiniz. Önce sherpa-onnx motorunu kurmak için **Motoru Kur**'a basın. Uzun kayıtlar duraklardan bölünür ve zamanlı parçalar olarak okunur; sohbetteki **Dikte** düğmesi de aynı model seçimini kullanır. Bu modeller yalnızca yazıya döker (**İngilizceye çevir** bir Whisper işidir) ve Omnilingual çıktısında noktalama ile büyük harf yoktur. Bu modeller için yalnızca 16 bit mono WAV doğrudan okunur; MP3 dahil diğer biçimler ffmpeg gerektirir.

WAV, MP3, FLAC ve Ogg doğrudan okunur. M4A, WebM ve Opus gibi biçimler ffmpeg gerektirir. Mikrofon kaydı 16 kHz WAV üretir. Mikrofon izni reddedilmişse sistemin gizlilik ayarlarında Harness Studio'ya izin verin.

Model Hub’dan veya kütüphane klasörlerinden eklenen ONNX CTC modelleri (wav2vec2, HuBERT, WavLM) de model listesinde görünür. Kütüphane kartındaki **Playground’da Aç** ile birini seçebilirsiniz. Yönetilen ONNX Runtime motorunu kullanır ve yalnızca yazıya çevirir: dil ve istem alanları gizlenir, çeviri kullanılamaz. Çıktıda zamanlı parçalar bulunmaz.

### Metinden Konuşmaya

**Konuşma** sekmesindeki **Metinden Konuşmaya** bölümü metni sese çevirir. sherpa-onnx motorunu kurmak için **Motoru Kur**'a basın (yaklaşık 20 MB, doğrulanır), ardından **Önerilen Ses Modelleri** altından bir ses indirin: Kokoro (çok dilli) ve Türkçe için önerilen Piper sesleri. Bir **Model**, bir **Ses** ve bir **Hız** seçin, **Okunacak metin**'i yazın ve **Seslendir**'e basın. Ses uygulama içinde çalar ve indirilebilir. Bir ses modeli kurulduktan sonra, sohbet yanıtındaki **Sesli Oku** düğmesi yanıtı o sesle okur; ses modeli yoksa uygulama sizi bu bölüme yönlendirir.

### Sesi İşle (Deneysel)

**Sesi İşle** bölümü bir kayıttaki sesleri ayırır ya da kaydın kalitesini iyileştirir. Deneysel audio.cpp motoruyla çalışır ve varsayılan olarak kapalıdır.

1. **Motoru Etkinleştir**'e basın. **Ayarlar > Model Motoru > Kurulu Model Motoru** sayfasında **audio.cpp Kur** ile motoru kurun, **Deneysel audio.cpp Model Motoru** anahtarını açın ve uyarıyı onaylayın.
2. **Model Hub**'da `audio-cpp/audio.cpp-gguf` deposunu açın ve **Deneysel** etiketli bir model indirin. Örneğin vokali ayırmak için BS-RoFormer, kaydı iyileştirmek için Apollo seçebilirsiniz.
3. Bir **Model** seçin. **Dosya Seç** ile bir WAV dosyası ekleyin ya da **Ses Kaydet** ile mikrofondan kaydedin.
4. Model ek ayar istiyorsa doldurun. Örneğin SAM Audio, ayırmak istediğiniz sesi bir cümleyle tarif etmenizi ister. Ardından **Sesi İşle**'ye basın.
5. Her çıktı kanalını (örneğin **Vokal** ve **Enstrümantal**) uygulama içinde dinleyin ve **Kaydet** ile bilgisayarınıza kaydedin.

Yalnızca WAV kabul edilir: en fazla 32 MiB, 10 dakika ve iki kanal. Bazı modeller 44,1 kHz ister; panel bunu belirtir ve mikrofon kaydını o hızda alır. AudioSR yalnızca GPU arka ucunda çalışır.

## Görsel Üretme

**Görsel Üretimi** sekmesi, stable-diffusion.cpp veya Python motoru (diffusers) üzerinden difüzyon modellerinden resim üretir. Model Hub'dan bir görsel modeli indirin (VAE ve metin kodlayıcılar gibi gereken ek dosyalar onunla birlikte gelir) ya da klasörlerinizde zaten bulunan birini kullanın: **Model** listesinde kendiliğinden görünür ve kütüphane kartındaki **Playground'da Aç** bu sekmeyi o model seçili olarak açar. Dosya orada olduğu halde liste boş kalıyorsa gereken ek dosyalardan bazıları eksiktir; sekme bunu belirtir ve istendiğinde **Görsel modülü (stable-diffusion.cpp)** kurun. Ağ geçidini açın, **Model**'i seçin ve bir **Prompt** yazın (isteğe bağlı olarak bir **Olmaması Gerekenler** alanı da doldurulabilir). **Boyut**, **Adım**, **Tohum**, **Adet** ve **CFG Ölçeği** ayarlanabilir; boş alanlarda modelin kendi değerleri kullanılır. **Üret**'e basın: model ilk istekte yüklenir. Resimler **Üretilenler** altında görünür ve yalnızca bu oturum boyunca saklanır; bir tanesini tutmak için **Kaydet**'i kullanın.

Görselden görsele üretim için isteğe bağlı **Başlangıç Görseli** ekleyin ve **Değişim Oranı** ile ne kadar değişeceğini ayarlayın. Yalnız düzenleme yapan modeller başlangıç görseli gerektirir. Python diffusers modelleri de bu sekmede açılır.

## Görevler (Gelişmiş)

**Görevler** sekmesi ONNX ve Python görev modellerini çalıştırır: modeli seçin, gereken metin, görsel veya ses girdisini verin ve çalıştırın. Sonuçlar etiket, açıklama, kutu, maske veya derinlik haritası olabilir.

Sohbette **Düşünme**, **Model Varsayılanı**, **Kapalı** veya modelin desteklediği düzeylerle o konuşmadaki düşünme davranışını yönetir. Ses kabul eden projektörü bulunan modellere dosya ekleyerek veya mikrofondan kayıt yaparak ses girdisi verebilirsiniz.

Ayrıca [Araçlar başvurusuna](../../tr/TOOLS.md) ve [Özellikler başvurusuna](../../tr/FEATURES.md) bakın.

Sıradaki: [Bağlantılar ve Ağ Geçidi](05-baglantilar-ve-ag-gecidi.md).
