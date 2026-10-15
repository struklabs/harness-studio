# Bağlantılar ve Ağ Geçidi

English: [../en/05-connections-and-gateway.md](../en/05-connections-and-gateway.md)

**Bu sayfada ne yapılır:** Yerel modelleriniz tek adresten sunulur ve başka bir uygulama ya da kodlama ajanı bu adrese bağlanır.

## Doğrudan Sunucu mu, Ağ Geçidi mi?

Başka bir program (Claude Code, Codex, OpenCode, bir editör eklentisi, Open WebUI vb.) yerel modelinize iki yoldan ulaşabilir:

| Yol | Nasıl çalışır? | Ne zaman kullanılır? |
| --- | --- | --- |
| Doğrudan sunucu | Dashboard'dan başlattığınız tek bir llama-server'ın kendi adresi (`http://127.0.0.1:<port>`). Port Otomatik ise her başlatmada değişebilir. | Tek model, hızlı deneme. |
| Ağ geçidi (gateway) | Harness Studio'nun içinde çalışan tek bir uç nokta. İstekteki `model` alanı **Takma ad (alias)** değerine göre doğru sunucuya yönlendirilir; model yüklü değilse ilk istekte yüklenir, boşta kalınca boşaltılabilir. | Birden fazla model, sabit adres, ajanlar. |

Ağ geçidinin en büyük yararı sabit tek adrestir: modeli ya da portu değiştirseniz bile istemcinizdeki adres aynı kalır. Ağ geçidi, isteğe bağlı motorlarda (konuşma, görsel, ONNX, Lemonade, mistral.rs) çalışan modelleri de llama.cpp modelleriyle aynı adresten sunabilir.

### Ağ Geçidini Aç

1. [Kütüphanem'de bir model tanımı](02-model-hub-ve-kutuphane.md#kütüphanem-ve-model-tanımları) bulunduğunu kontrol edin; istemcinizin kullanacağı ad, tanımın takma adıdır.
2. **Ayarlar > Ağ Geçidi > Uç Nokta** sayfasını açın.
3. **Gateway'i aç** seçimini etkinleştirip **Uygula** düğmesine basın. Durum **Çalışıyor** olmalıdır.
4. **Temel URL** alanındaki adresi (yanında **Kopyala** düğmesi var) istemcinize girin. Model adı olarak tanımın takma adını kullanın.

Ağ geçidi varsayılan olarak kapalıdır ve Harness Studio kapanınca durur. Playground'ın Konuşma ve Görsel sekmeleri ile entegrasyonlar da ağ geçidini kullanır.

Varsayılan port `11435`, adres `127.0.0.1`'dir; yani yalnızca kendi bilgisayarınız erişebilir. Port başka bir program tarafından kullanılıyorsa ağ geçidi boş bir porta geçer; **Bağlantılar** sayfasında (**Uç Noktalar > Port Notları**) güncel adres ve taşınması gereken istemciler gösterilir. Her zaman ekranda görünen adresi esas alın.

| İstemci türü | Varsayılan adres biçimi |
| --- | --- |
| OpenAI uyumlu (çoğu araç, editör eklentisi, SDK) | `http://127.0.0.1:11435/v1` |
| Anthropic uyumlu (Claude Code, Anthropic SDK) | `http://127.0.0.1:11435`, sonunda `/v1` olmadan |
| Ollama uyumlu | Ağ geçidinin `/api/*` yolları; ayar **Ayarlar > Model Motoru > Ollama** sayfasındadır ve Ollama'nın kendi portunda da dinleme seçeneği vardır |

Yerel ağ geçidi varsayılan olarak anahtar istemez. Anahtar isteyen araçlar için herhangi bir değer yeterlidir (Harness Studio `harness-local` yer tutucusunu kullanır).

### Ağ Geçidi Ayarları

| Sayfa | Ne yapar? |
| --- | --- |
| **Ayarlar > Ağ Geçidi > Uç Nokta** | Açma/kapama, **Port**, **Bağlanma adresi**, **Ağdan erişime izin ver**, **Bu bilgisayarda da token iste**, **İzinli tarayıcı origin'leri** (web uygulamaları için). |
| **Ayarlar > Ağ Geçidi > Yönlendirme** | **En fazla yüklü model**, **Model başına kuyruk**, **İstek zaman aşımı (sn)**, **Varsayılan boşta kalma süresi (sn)**, **Bellek farkındalıklı tahliye**, **VRAM payı (MiB, 0 = otomatik)**, **Karar uç noktalarında sohbet modeli emülasyonu**; altında her modelin durumu (**Hazır**, **Yükleniyor**, **Yüklü değil**) ve **Boşalt** düğmesi. |
| **Ayarlar > Ağ Geçidi > Responses** | OpenAI Responses API'nin yanıtları yerelde saklayıp saklamaması (**Yanıtları sakla**) ve saklama sınırları. |

**Varsayılan boşta kalma süresi (sn)**, kendi boşta kalma süresi olmayan modellere uygulanır; boş bırakılırsa boşta kalma nedeniyle boşaltılmazlar. Uyumlu bir stable-diffusion.cpp derlemesi ve otomatik sığdırma açıkken ağ geçidi, ağırlıkların bir kısmını RAM’de tutan görsel modelini kabul edebilir: GPU, hesaplama tamponları ve bellek payını alabilmeli; kalan ağırlıklar boş RAM’in %90’ına sığmalıdır. Eski derleme, kapalı otomatik sığdırma veya açıkça belirlenmiş ağırlık yerleşimi, tam GPU tahmininin sığmasını gerektirir.

Ağ geçidi yalnızca kendi yüklediği modelleri (**Gateway yükledi**) boşta kalma süresine ve bellek kurallarına göre boşaltır. Sizin elle başlattığınız sunuculara (**Sizin sunucunuz**) yönlendirir ama onları hiçbir zaman kapatmaz. Bir model tanımında **Sabit (hep yüklü)** işaretlenirse ağ geçidi başlarken o model yüklenir ve asla boşaltılmaz.

Bir model yüklenirken ya da kuyruk doluyken istek reddedilirse Playground'da "{model} şu an yüklenemedi: {busy} istek işliyor ve kapatılamıyor. {wait} tekrar deneyin." gibi bir uyarı çıkar; istekler bitince **Tekrar Dene** ile yeniden gönderin ([Sorun Giderme](10-sorun-giderme.md#model-meşgul-503)).

Ayrıntılı yönlendirme kuralları, istemci başına ayarlar ve sınırlar için [entegrasyonlar başvurusunu](../../tr/INTEGRATIONS.md) okuyun.

### Karar Modelleri

Karar modeli bir metni (*durum*, `state`) okur ve türü belli sorulara yazılı bir yanıt yerine olasılıklarla cevap verir: evet ya da hayır, bir listeden tek seçenek veya bir ölçekte bir düzey. Ağ geçidi bunun için iki istek biçimi sunar; token, tarayıcı origin'i ve otomatik yükleme kuralları diğer uçlarla aynıdır:

| Yol | Biçim | Soru türleri | Hatalar |
| --- | --- | --- | --- |
| `/v1/systemone` | Ollama, LM Studio, llama.cpp: `state`, `images`, adla anahtarlanmış nesne olarak `questions`, `keep_alive` | `noul`, `choice`, `score` | Ollama gibi `{"error": "..."}` |
| `/v1/decisions` | OpenAI, LM Studio: `input`, dizi olarak `questions`; yanıtlar soru sırasıyla | `predicate`, `choice`, `score` | OpenAI'nin hata nesnesi |

```bash
curl http://127.0.0.1:11435/v1/systemone -H "Content-Type: application/json" -d '{
  "model": "my-model",
  "state": "Customer message: I was charged twice and nobody has replied.",
  "questions": {
    "route": {"type": "choice", "instructions": "Which team should handle this?",
      "criteria": {"billing": null, "shipping": null, "technical": null}},
    "angry": {"type": "noul", "instructions": "Is the customer angry?"},
    "urgency": {"type": "score", "instructions": "How urgent is this?",
      "criteria": ["can wait", "this week", "today", "right now"]}
  }
}'
```

- Dosyasında karar meta verisi olan bir model (Modellerim'de **Karar modeli** etiketiyle görünür) doğrudan llama.cpp tarafından yanıtlanır.
- Diğer sohbet modelleri emülasyonla yanıt verir: ağ geçidi soruları tek tek sorar, modelin yalnızca seçenek kodlarından birini yazmasına izin verir ve olasılıkları modelin token olasılıklarından okur. **Bu olasılıklar kalibre değildir**: seçenekleri birbiriyle karşılaştırmak için kullanın, ölçülmüş bir doğruluk gibi değil. **Ayarlar > Ağ Geçidi > Yönlendirme > Karar uç noktalarında sohbet modeli emülasyonu** ile kapatabilirsiniz; o zaman Ollama ve LM Studio'daki gibi yalnızca karar modelleri yanıt verir, sohbet modeli 400 hatası alır.
- `X-Harness-Decision-Mode` yanıt başlığı hangisinin yanıtladığını söyler: `native` ya da `emulated`.
- Sınırlar: istek başına en fazla 64 soru, seçim sorusu başına 2–255 seçenek (emülasyonda en fazla 52), 2–10 puan düzeyi, akış yok. Görseller base64'tür (data URL, `/v1/systemone`'da düz base64 de olur) ve görsel okuyabilen bir model gerektirir.

## Bağlantılar Sayfası

Sol menüdeki **Bağlantılar** bölümünün sayfaları:

| Grup | Sayfalar | İçeriği |
| --- | --- | --- |
| **Uç Noktalar** | **Uç Nokta Listesi**, **Ollama Uç Noktası**, **Port Notları** | Sunucuların ve ağ geçidinin adresleri; kopyalanabilir. |
| **Araçlar ve İstemciler** | **Otomatik Yapılandırma**, **Adım Adım Kurulum**, **SDK ve Kod Örnekleri**, **Harness MCP'yi Ekle** | Kodlama araçlarını bağlama ve kod parçaları. |
| **MCP Sunucuları** | **Sunucu Listesi**, içe aktarma, **Durum ve Canlı Araçlar** | Harness Studio'nun kendi MCP istemcisiyle bağlandığı sunucular. |
| **Test** | **Araç Çağırma Tezgâhı**, **Bağlantı Testleri** | Bağlantı ve araç çağırma denemeleri. |

**Basit** arayüz düzeyinde yalnızca **Otomatik Yapılandırma**, **Adım Adım Kurulum** ve **Uç Nokta Listesi** gösterilir; diğerleri Gelişmiş sayfalardır.

## Kodlama Ajanı Bağla

Claude Code, Codex, OpenCode, Aider ve Goose için uygulama içinden başlatma akışı vardır. İstemcinin kendisi bilgisayarınızda kurulu olmalıdır; kurulu araçlar **Algılandı** rozetiyle işaretlenir, **Yeniden Tara** ile liste yenilenir.

1. **Bağlantılar > Araçlar ve İstemciler > Otomatik Yapılandırma** sayfasındaki **Araç Entegrasyonları** alanından istemciyi seçin.
2. Model ve bağlantı adresini kontrol edin. Ağ geçidi açıksa adres ağ geçidinin adresidir; değilse seçili sunucunun doğrudan adresidir. Araç çağırabilen bir model kullanın; **Bağlam yetersiz** uyarısı çıkarsa okuyun. Kodlama ajanları büyük istemler gönderir: Claude Code, Codex, OpenCode ve Goose için 64k, Aider için 32k bağlam önerilir.
3. **Başlat** düğmesine basın. Açılan pencerede **Proje klasörü** ve **Model** seçin.
4. Bağlam yetersizse **Sunucuyu Hazırla** seçeneklerine bakın. **Hazırla ve Yükle**, model tanımına bağlam, Flash Attention, KV önbelleği ve eşzamanlı istek değerlerini yazar ve çalışan örnekleri durdurur; ekrandaki açıklamayı okuyun. Bellek yetmezse önceki ayarlar geri yazılır.
5. **Başlat** ile ajanı açın. Bu yol ayarları yalnızca o oturumun ortam değişkenlerine ve geçici bir dosyaya verir ve yeni bir terminal penceresi açar; aracın genel yapılandırmasını değiştirmez. Platform terminal açamıyorsa **Komut satırı karşılığı** komutunu kendiniz çalıştırın.

Kalıcı kurulum için desteklenen istemcilerde **Yapılandır** düğmesini kullanın. Harness Studio, aracın ayar dosyasında yalnızca kendi anahtarlarını değiştirir ve önce renkli bir fark önizlemesi gösterir; önizlemeyi okuyup uygulayın. **Geri Al**, yalnızca Harness Studio'nun yazdığı ve sonradan sizin değiştirmediğiniz anahtarları geri alır. Elle kurulan istemcilerde (**Adım Adım Kurulum** sayfası) yönergeleri ve kopyalanabilir alanları kullanın. Bir modelin adını değiştirirseniz kartta "Araç eski model adını kullanıyor" uyarısı ve **Düzelt** düğmesi çıkar.

Sık kullanılan üç istemci için özet (ayrıntılar ve tam ortam değişkenleri [entegrasyonlar başvurusundadır](../../tr/INTEGRATIONS.md)):

| İstemci | Ne gerekir? |
| --- | --- |
| Claude Code | Anthropic uyumlu adres (`http://127.0.0.1:11435`, `/v1` yok) ve bir model adı. **Başlat** veya **Yapılandır** bunu `ANTHROPIC_BASE_URL` ve ilgili model değişkenlerine yazar. |
| Codex | OpenAI Responses API üzerinden ağ geçidi adresi. Codex yalnızca Responses API konuşur; ayarlar bir `harness` sağlayıcısı olarak eklenir. |
| OpenCode | OpenAI uyumlu adres (`http://127.0.0.1:11435/v1`). **Yapılandır**, `opencode.json` içine `harness` sağlayıcısını ve ağ geçidindeki sohbet modellerini yazar. |

**Bağlantıyı Test Et** ile istemcinin kendi API biçiminde deneme yapın. Başarılı bir test, gerçek ajanınızın her görevde doğru aracı çağıracağı anlamına gelmez. Küçük yerel modeller araç çağrısında hata yapabilir; **Araç Çağırma Tezgâhı** bunu önceden denemenizi sağlar. Araç sonucunu girdikten sonra son yanıtı istemek için **Yanıtı Üret** düğmesini kullanın. İstemciye özgü sınırlamalar ve test kayıtları [entegrasyonlar başvurusundadır](../../tr/INTEGRATIONS.md).

## API Anahtarlarını Birbirinden Ayır

| Kimlik bilgisi | Ne için kullanılır? |
| --- | --- |
| Hugging Face erişim tokenı | Model deposuna erişim ve indirme. Sohbet sunucusunun anahtarı değildir. |
| Sunucu API anahtarı | Doğrudan llama-server bağlantısını korur. Dashboard'daki **Ağ** sayfasında **API anahtarı** alanından veya **Ayarlar > Güvenlik > API Anahtarları** üzerinden yönetilir. |
| İsimli token | Control API'ye veya ağ geçidine verilen kapsamlı erişim. **Ayarlar > Geliştirici > Control API** sayfasında oluşturulur. Model isteği için `inference` kapsamı gerekir. |

Ağ geçidinde **Bu bilgisayarda da token iste** kapalıysa yerel istemciler anahtarsız bağlanır. Açıksa `inference` kapsamlı bir isimli token kullanın. İsimli tokenın değeri yalnızca oluşturulurken gösterilir; o anda kopyalayın.

Başka cihazların ağ geçidine erişmesi için **Ağdan erişime izin ver** ve `inference` kapsamlı bir token gerekir; anahtarsız ağ erişimi yoktur. Bu kullanımda [güvenlik modelindeki](../../tr/SECURITY_MODEL.md) koşulları izleyin ve güvenlik duvarını kendiniz ayarlayın; Harness Studio bunu değiştirmez. Ağ geçidinin, Control API'nin ve doğrudan sunucunun anahtarlarını birbirinin yerine kullanmayın.

Sıradaki bölüm: [Araçlar](06-araclar.md).
