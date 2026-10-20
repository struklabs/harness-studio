# Araç Yazma ve Oluşturma

[English](../en/TOOLS.md)

Harness araçları Playground, CLI ve Control API'de aynı kayıt defterini ve izin politikasını kullanır. **Oluştur → Araçlarım** tüm yerleşik, kurulu/üretilmiş ve MCP araçlarını gösterir. MCP araçları sunucularına göre, her kaynak da işlevine göre gruplandırılır. Kapalı veya bozuk kurulu paketler de listelenir; bunlar çalıştırılabilir araç sayılmaz.

## Elle Bir Paket Yazma

Bir klasöre `manifest.json`, `index.js` ve tercihen `test.js` koyun. İsteğe bağlı `README.md` araç ayrıntılarında görünür.

```json
{
  "name": "text_reverse",
  "version": "0.1.0",
  "category": "text",
  "description": "Metnin karakter sırasını tersine çevirir.",
  "input_schema": {
    "type": "object",
    "properties": { "text": { "type": "string" } },
    "required": ["text"]
  },
  "output": "Ters çevrilmiş metin.",
  "permissions": {
    "network": [],
    "fs": { "read": [], "write": [] },
    "shell": false
  },
  "runtime": "js",
  "entry": "index.js",
  "test": "test.js"
}
```

| Alan | Kural |
|---|---|
| `name` | `[a-z][a-z0-9_]{1,47}`; araç çağrısı ve klasör adı. Mevcut araçla çakışmamalı. |
| `version` | Semantik sürüm, örneğin `0.1.0`. |
| `category` | `file`, `web`, `calculation`, `datetime`, `text`, `other`. Yoksa `other`; bilinmeyen değer reddedilir. |
| `description` | Modelin hangi durumda çağıracağını anlatan, en fazla 1024 karakter açıklama. |
| `input_schema` | `type: object` içeren JSON Schema; `properties` ve `required` ile parametreler. |
| `output` | Sonuç metninin ne içerdiği. |
| `permissions` | Ağ kapsamı ve çalışma alanına göre klasör izinleri; boş liste erişim vermez. Ağ için `["*"]` tüm genel internete izin verir. |
| `runtime` | `js` (QuickJS). |
| `entry`, `test` | Paket içindeki göreli dosya yolları; mutlak yol ve `..` yasak. `test` isteğe bağlı. |

`index.js`:

```js
function run(args) {
  if (typeof args.text !== 'string') throw new Error('text must be a string');
  return args.text.split('').reverse().join('');
}
```

Sonuç string veya JSON'a çevrilebilen bir değer olabilir. `btoa`, `atob`, `TextEncoder`, `TextDecoder`, `JSON`, `Math`, `Date` ve `RegExp` globalleri kullanılabilir. Eksik web globalleri saf JS ile sağlanır; mevcut uygulamalar değiştirilmez ve yeni dosya/ağ/süreç erişimi verilmez. `btoa`/`atob` Latin-1 ikili dizeler kullanır; Latin-1 dışındaki `btoa` girdisi veya geçersiz base64 hata verir. `atob` ASCII boşlukları ve eksik padding'i kabul eder. Unicode metni önce `new TextEncoder().encode(text)` ile UTF-8 baytlarına, ardından ikili dizeye çevirip `btoa` kullanın. `TextEncoder.encode` Uint8Array döndürür; `TextDecoder.decode` ArrayBuffer veya typed array görünümünü UTF-8 olarak çözer. Polyfill decoder yalnız UTF-8 destekler, streaming desteklemez; bozuk baytları varsayılan olarak U+FFFD ile değiştirir, `{fatal: true}` ile hata verir. `require`, `import`, `fetch`, `process` ve zamanlayıcılar bulunmaz. Bellek, süre ve sonuç boyutu sınırları vardır.

### Ağ ve Dosya API'si

- `harness.http.get(url)` ve `harness.http.post(url, jsonBody)` eşzamanlıdır; `{status, content_type, url, body}` döndürür. `permissions.network` üç kapsam taşır: `[]` ağ erişimi yok, `["*"]` tüm genel internet, diğer listeler yalnız belirtilen alan adları (`example.com`, `*.example.com`; protokol/yol yok). `*.example.com` alt alan adlarını kapsar, kök alanı kapsamaz; `"*"` başka girdilerle birleştirilemez. Eski boş listeler ağ erişimi vermez. Her kapsamda loopback, link-local/metadata, özel ağ, CGNAT, IPv6 ULA ve Harness portları engellenir. Her DNS cevabı ve yönlendirme denetlenir; bağlantı doğrulanan adreslere sabitlenir ve sistem proxy’si kullanılmaz.
- `harness.fs.read(path)` metin okur, `harness.fs.write(path, text)` yazar, `harness.fs.list(path)` dizini listeler. Yollar sohbetin çalışma alanına görelidir. Okuma ve yazma izinleri ayrıdır; yazma okuma izni vermez. `"."` bütün çalışma alanını, `"out"` yalnızca o klasörü ve altını kapsar. Sembolik bağlantılar sınır dışına erişim sağlayamaz.
- `shell` yalnızca `false` olabilir. JS paketleri süreç başlatamaz.

`test.js`:

```js
const cases = [
  { name: 'reverse', args: { text: 'abc' }, expect: { equals: 'cba' } },
  { name: 'invalid input', args: {}, expect: { error: 'must be a string' } },
  { 
    name: 'network call', 
    args: { q: '123' }, 
    http: [{ url_prefix: 'https://example.com/', status: 200, body: 'mocked' }], 
    expect: { equals: 'mocked' } 
  }
];
```

Her `expect` tam bir seçenek içerir: `equals`, `contains` (string veya string listesi), `error` (`true` veya hata metni). Her test vakası ayrı, boş bir geçici çalışma alanında çalışır. Dosya girdisi için vakaya `fs: [{path: "input.csv", content: "name,age\nAlice,30"}]` ekleyin; çalıştırıcı UTF-8 içeriği `run(args)` öncesinde yazar. İç içe klasörler oluşturulur. Göreli yollar kullanılır; mutlak yol, `.`/`..`, boş yol bileşeni ve yinelenen dosya reddedilir. Vaka başına en fazla 32 dosya ve toplam 1 MiB içerik kabul edilir. Fixture hazırlamak araca izin vermez: manifest okuma/yazma kapsamları aynen uygulanır. Vakanın yazdığı dosyalar sonraki vakaya taşınmaz; geçici klasör vaka bitiminde temizlenir. `setup` callback desteklenmez ve açık hatayla reddedilir; `http` içindeki `file://` mock'u dosya oluşturmaz. Ağ çağrıları yapan paketlerin testlerinde, isteğe bağlı `http` dizisi (URL eşleşmesi, HTTP status, headers ve body) tanımlanabilir. Bir test senaryosunda `http` tanımlıysa, `harness.http` çağrıları YALNIZCA mock yanıtlarını kullanır; ağa çıkmaz (eşleşmeyen istek hatayla reddedilir). Otomatik üreticinin testlerinde gerçek ağ erişimi kapalıdır, dosya erişimi yalnız fixture içeren geçici çalışma alanına yönelir; otomatik üretici her ağ aracının testini mutlaka `http` mock'larıyla yazar; aksi halde test `network` reddiyle başarısız olur ve modelden bunu düzeltmesi istenir. Elle kurulan paketlerin (ve mock kullanmayan senaryoların) ağ testleri gerçek ağa erişebilir.

### Kurulum ve Yönetim

**Oluştur → Araçlarım → Kurulu Araçlar** bölümünde klasörü seçin. Gösterilen izinleri inceleyip kurulumu onaylayın. Paket uygulamanın veri dizinindeki `tools/<name>/` altına kopyalanır. Aç/kapa, test et, ayrıntıları incele ve kaldır işlemleri aynı yerde bulunur. Ayarlar'daki kurulu araç yönetimi de aynı bileşeni kullanır. Kurulum, araç çağrılarının izin politikasını gevşetmez.

## Araç Oluşturucu

**Oluştur → Araç Oluşturucu** sayfasında model uç noktasını ve modeli seçin; varsayılan uç nokta mevcut çalışan sunucu/ağ geçididir. Aracı tarif edin, ağ kapsamını seçin ve isteğe bağlı olarak izin verilen alan adlarını ve göreli klasörleri yazın. Model paket ve testleri üretir; bozuk JSON, izin aşımı veya test hatasını en fazla dört düzeltme denemesiyle onarır. Manifest, kod, test ve izinleri okuyup oturum için onaylayabilir, ardından kalıcı kaydedebilirsiniz. Playground'daki **Araç Oluştur…** düğmesi aynı arka ucu kullanan bir üretici iletişim kutusu açar. Küçük modeller daha sık hatalı paket yazabilir.

Araç Oluşturucu varsayılan olarak **Tüm İnternet** seçer. **Ağ Erişimi Yok** veya **Yalnızca Bu Alan Adları** ile daraltabilirsiniz. Alan adı metin kutusu yalnız son seçenekte uygulanır; boş liste erişim vermez. Seçiminiz modelin isteyebileceği iznin üst sınırıdır; aşılırsa araç reddedilir ve modelden düzeltmesi istenir. Ağ/dosya izni isteyen araçlar, Otomatik Araç Oluşturucu dahil, kurulumda açık onay gerektirir; tam ağ kapsamı onay özetinde **Tüm İnternet** olarak gösterilir.

`null` ve `undefined` dönüşleri yalnız testlerde değil, normal çalıştırmada da dört karakterlik `"null"` sonuç metnidir; boş dize `""` boş kalır. Playground sonuç kartı `null` gösterir ve sonraki model turuna aynı metni yollar. Gateway'in ortak araç döngüsü de bunu başarılı tool sonucu olarak modele iletir; bu bir hata veya iptal işareti değildir. Kullanıcıya anlamlı bir onay vermesi gereken araç açık bir sonuç dizesi döndürmelidir.

## Otomatik Araç Oluşturucu

`equals`, beklenen ve gerçek metni iki tarafa `trim_end` uygulayarak karşılaştırır: yalnız sondaki satır sonu ve boşluk farkları yok sayılır. Baştaki ve ortadaki boşluklar tam eşleşmelidir. Rapor özgün beklenen/gerçek metni korur; çalıştırma çıktısı kırpılmaz. `contains`, `matches` ve `length` kuralları değişmez.

Sıranın üzerindeki **Tümü / Sırada / Üretiliyor / Kuruldu / İzin bekliyor / Başarısız** filtreleri her durumun sayısını gösterir. Üretiliyor; üretim, test ve kurulum aşamalarını kapsar. Sayaçlar da filtre seçer: Taranan ve Planlanan tüm sırayı, Üretilen taslağı olan kartları gösterir. Seçim yerel depolamada saklanır; depolama erişimi kapalı olsa da filtre çalışır. Sürükleme ve oklarla sıralama yalnızca Tümü/Sırada görünümünde kullanılabilir. **Başarısızları Tekrar Dene**, tekil tekrar deneme gibi başarısızları sıraya alır; üretim **Devam Et** ile sürdürülür.

Paket testindeki `expect` tam bir doğrulama içerir: `equals`, `contains`, `error` yanında `matches` (sandbox içinde JavaScript RegExp; tam eşleşme için `^…$`) veya `length` (sonuç metninin Unicode karakter sayısı, sıfır dahil) kullanılabilir. Örneğin rastgele şifre için `expect: { matches: /^[A-Za-z0-9]{12}$/ }`; bütün alfabeyi `contains` ile aramak doğru bir test değildir. Dize sonuçları düz metin, boolean/sayı/nesne/dizi/null JSON metni olarak karşılaştırılır. `undefined`, JSON karşılığı olmadığı için `null` olarak normalize edilir; boş dize boş kalır.

`matches` desen dizesi, `/desen/bayrak` dizesi veya JS regex literali alır. RegExp nesnesinin `source` ve `flags` değerleri case serileştirilmeden önce taşınır. JS sözdizimi (lookahead dahil) ve bayrakları QuickJS değerlendirir; her case yeni RegExp kullanır, `lastIndex` paylaşılmaz. Geçersiz desen/bayrak `bad_case` verir; Rust regex ile kıyaslama yapılmaz.

Üretim yanıtı önce yalnız manifest nesnesini içeren bir `json` bloğu, sonra `index.js:` / `test.js:` başlıklarıyla iki `js` bloğudur; `js index.js` / `js test.js` info string biçimi de kabul edilir. JavaScript ham yazılır. Eski tek-JSON (`manifest`, `index_js`, `test_js`) biçimi geriye uyumlu kalır. Serde geçersiz kaçış bildirirse yalnız JSON dize literallerindeki geçersiz kaçışlar bir kez çift ters eğik çizgiyle onarılır; geçerli kaçışlar ve diğer JSON hataları değiştirilmez. Kesik çit truncation/retry yoluna girer. Fikir aşaması katı JSON kalır.

Retry feedback'i `bad_case` için testi düzeltmeyi, diğer hatalarda kod ve beklentiyi sözleşmeyle karşılaştırmayı ister. Makul/doğru davranışla çelişen beklenti düzeltilebilir; model case adını ve nedenini tek satır JS yorumuyla belirtmelidir. Geçerli beklentiyi zayıflatmak ve case silmek yasaktır. Üretici en az iki case ister. Retry'da case sayısı önceki en büyük suite'ten azalırsa harness eski suite'i yeni kodla tekrar çalıştırır, hataları bildirir ve taslağın geçmesini engeller; sonraki retry'da da aynı sınır korunur. Bu sayısal kontrol beklentilerin anlamsal doğruluğunu kanıtlamaz; kod/test incelemesi gereklidir.

Üretici ilk yanıtın ardından en fazla dört düzeltme ister. Bağlam elverdiği sürece her düzeltmede önceki **tam yanıt** (kod ve testler dahil) ve bütün başarısız testlerin beklenen/gerçek sonuçları modele gönderilir. Eski tek-JSON biçiminde çevre açıklamalar ve JSON çitleri kabul edilir; yeni biçimde bloklar etiketlenmelidir. Bozuk ya da belirsiz çoklu paketler aynı retry bütçesini kullanır. Yanıt bütçesi 8192 token, özel `stop` dizisi yoktur. API `finish_reason=length` veya kapanmamış blok bildirirse yanıt eksik sayılır; önceki başarısız taslak korunur ve daha kısa kod istenir. JavaScript içindeki tek/çift tırnaklı dizelerde gerçek satır sonu bir sözdizimi hatasıdır; ham JS blokları çift JSON/JS kaçışı ihtiyacını kaldırır. Sözdizimi hatası yarım model yanıtı olarak sunulmaz; feedback `index.js` veya `test.js` dosyasını işaret eder.

**Oluştur → Otomatik Araç Oluşturucu** sayfasında model seçip **Otomatik Araç Oluştur** düğmesine basın. Varsayılan 8, en fazla 20 araç planlanır. Harness kota uygulamaz; uzak bir sağlayıcının kendi sınırları ve ücretleri geçerli olabilir.

1. Yerleşik araçlar, kurulu paketler ve bağlı MCP araçları ad/açıklama/kategori/kaynak bilgileriyle taranır. MCP bağlantıları için bekleme sınırlıdır (8 sn); geç bağlanan sunucular kaçmasın diye envanter her ideasyon denemesinden önce yeniden okunur ve sonradan görünen araçlar etkinlik günlüğüne "Late tools" olarak yazılır.
2. Model yeni amaçlar önerir. Katı JSON doğrulanır, aynı ad, anlamca eşdeğer ad (kök/çekim eki ve eşanlamlı katlanmış belirteç kümesi; `word_counter` ≈ `count_words`) ve yüksek açıklama benzerliği elenir. Bozuk/tekrarlanan öneriler bir kez yeniden istenir. Bu sözcüksel eleme (gömme/sunucu gerektirmez) her anlam eşdeğerliğini yakalamaz; eleme sonrası sıra istenenden kısa olabilir.
3. Kuyruk otomatik işlenir. Bekleyen kartları sürükleyerek veya yukarı/aşağı düğmeleriyle önceliklendirebilir, çıkarabilirsiniz. Çalışan kartın sırası değişmez. Duraklat/durdur model isteğini iptal eder; devam et kesilen aracı yeniden üretir. Başarısız kart tekrar denenebilir.
4. Mevcut üretici her paket için manifest, JS ve test üretir; hataları modele ileterek en fazla dört kez düzeltir. Testler kullanıcı dosyalarına ve gerçek ağa erişmeden çalışır. Dosya girdileri vaka başına `fs` fixture içerikleriyle, ağ girdileri `http` mock yanıtlarıyla test edilir; saf mantık ve erişim reddi de test edilebilir; gerçek dış sistem davranışı ayrıca doğrulanmalıdır.
5. Testi geçen, ağ/dosya istemeyen paket kalıcı kurulur. Ağ veya dosya isteyen paket **İzin bekliyor** durumuna geçer. Kart Tüm İnternet veya alan adı sınırlamasını, okuma/yazma klasörlerini ve shell yasağını gösterir; **Bu İzinlerle Kur** o paketi kurar. İzinler sessizce genişletilmez. Başarısız paket kurulmaz; mevcut araçlar üzerine yazılmaz.

Sıra, paket taslakları, durumlar ve son 500 etkinlik `auto-tools/run.json` dosyasında saklanır. Sayfadan ayrılmak işi durdurmaz. Uygulama kapanırsa yeniden açılışta çalışma duraklatılmış olur; model seçip devam edin. Uç nokta kimlik bilgileri bu dosyada tutulmaz. İzin bekleyen/henüz işlenmeyen kartlar bitirilmeden veya çıkarılmadan yeni çalışma başlatılamaz.

**Canlı İşlem Günlüğü** içindeki **Günlüğü Temizle**, görünen ve kayıtlı günlüğü temizler. Bir kontrol işlemi sürerken veya günlük boşken devre dışıdır. Sıra, sonuçlar, envanter, token toplamları ve çalışan üretim korunur.

### CLI ve Control API

```powershell
harness tools inventory --json
harness tools auto start --endpoint http://127.0.0.1:11440/v1 --model Qwen3-4B-Instruct-2507-Q4_K_M --count 4 --json
harness tools auto status --json
harness tools auto start --endpoint http://127.0.0.1:11440/v1 --resume --json
harness tools auto action pause
harness tools auto action retry --id ITEM_ID
harness tools auto action reorder --order ITEM_ID_2,ITEM_ID_1
```

CLI başlatma komutu tamamlanmayı bekler; Ctrl+C çalışmayı durdurur. API: `GET /tools/inventory`, `GET /tools/automatic`, `POST /tools/automatic/start` (`endpoint`, `count`, `resume`), `POST /tools/automatic/action` (`action`, `id`, `order`). Yollar Control API'nin `/api/harness/v1` öneki altındadır. Çalışma/taslak okuma dahil `/tools/automatic` işlemleri `admin` kapsamı ister. API `approve` işlemi yalnızca saklanan kart kimliğini kabul eder; kendi arayüzünüzde kesin izinleri göstererek insan onayını alın. CLI izin onayı yerine uygulama/API kullanır.


CLI sahiplik kararı: `harness tools auto` komutları `session::connect` üzerinden Control API'ye gider; `run.json` dosyasını doğrudan açmaz. GUI/`harness serve` veri dizininin `instance.lock` kilidini tutuyorsa mevcut sahibin API'si kullanılır. API kapalıysa `cli.owner_without_api` (çıkış 4), sahip bilgisi henüz okunamıyorsa `cli.lock_held` hatası döner; ikinci yerel yazıcı başlatılmaz. Sahip yoksa geçici örnek önce mevcut işletim sistemi dosya kilidini alır ve komut boyunca tutar. Kilit alma yarışında da yeni sahibin API'sine yönlendirilir veya hata verilir. Bu nedenle CLI için yeni dosya kilidi bağımlılığı eklenmedi; otomatik üreticinin bellekten yazdığı durum yalnızca bu tek sahip içinde değiştirilir.

Duraklat/durdur, çalışan sandbox testinin QuickJS iptal token'ına da iletilir; uzun JS döngüsü veya ağ beklemesi 30 saniyelik test sınırını beklemeden kesilir. Bozuk paket manifesti yalnızca ilgili kartı hata mesajıyla başarısız yapar; kuyruktaki diğer araçlar işlenmeye devam eder.

### Akıllı Düzeltme ve Üretici Tokenları

İlk üretim + dört düzeltme = en fazla **beş model çağrısı** (`build:1`…`build:5`). Düzeltme yanıtı ilk satırda `CHANGE: ne değiştiriyorum ve neden` verir; ayrıştırıcı bu satırı paket dışında tutar. Not, başarısız case sayısı, tokenlar, tekrar/strateji ve bağlam özeti bilgisi taslakta ve otomatik run'ın kart geçmişinde saklanır. Not vermeyen modeller için açık boş-not metni gösterilir.

Boşlukları normalize edilen index.js önceki bir denemeyle aynıysa veya başarısız case adları ve gerçek çıktıları yeniden oluşursa sonraki feedback farklı yaklaşım ister. İki ardışık tekrar ve en az iki kalan deneme, düzeltmeleri test-önce akışına geçirir: önce yalnız test.js incelenir (index.js değiştirilirse reddedilir), ardından kod en sade doğru yaklaşımla sıfırdan yazılır. Yalnız son deneme kaldıysa test inceleme adımı atlanır, doğrudan kod yeniden yazılır; son tur kod düzeltmesini yasaklayan bir incelemeyle tüketilmez. Case-count guard, bad_case yönlendirmesi, ağ mock zorunluluğu ve kesik yanıtta önceki taslağı koruma sürer. Bu kontroller, testlerin sözleşmeye anlamsal olarak uygunluğunu kanıtlamaz.

Her isteğin konuşma büyüklüğü mevcut compaction sayacıyla ölçülür: llama-server `/tokenize`, uyumlu count_tokens, son seçenek ≈ üç karakter/token + mesaj overhead'i. `/props` içindeki n_ctx okunur; bilinmiyorsa güvenli varsayılan **16.384** kullanılır. Konuşma + **8.192** yanıt tokenı sığmıyorsa eski denemeler tek kısa özete çevrilir; sistem, asıl kullanıcı isteği, son tam yanıt ve son feedback aynen korunur. Özet notu ve kısaltılmış beklenen/gerçek case geri bildirimini taşır. Bu sabit parçalar tek başına pencereyi aşarsa asıl istek kırpılmaz; sunucu isteği reddedebilir.

Usage `prompt_tokens` ve `completion_tokens` deneme, araç ve run düzeyinde toplanır; ideation çağrıları ve kesik tamamlamalar da dahildir. API usage eksikse kendi sayımımız kullanılır, `estimated:true` ve UI'da **≈** gösterilir. Alanlar serde default ile eski run.json dosyalarını okuyabilir. Tekil/toplu retry aynı öğenin harcamasına eklenir; kaldırılan kartın harcaması run toplamından silinmez. Son 100 önceki denemenin gerçek yanıt ortalaması sonraki otomatik run tahminini besler; yoksa 2.000 yanıt tokenı varsayılır.

Başlangıç tahmini planlama prompt sayımı + araç başına ölçülen sistem/istek ve beklenen yanıt × N × 1,25–3,5 çağrıdır (beklenen retry 0,25–2,5; büyüyen retry bağlamı için aralık). Manuel Create üreticisinde planlama eklenmez. Model/araç uzunluğu ve retry konuşmasının büyümesi nedeniyle bu garanti veya kota değildir. Sayaç şeridi, araç kartları, Paketi incele → Deneme geçmişi ve canlı `↑giriş ↓çıkış` satırları gerçek harcamayı gösterir. Son fark kartı gerçekleşen toplamdan tahmin aralığının orta noktasını çıkarır.

Üretim/ideation `x-harness-client: harness-auto-tools`, manuel üretim `harness-tool-creator` gönderir. Gateway bunu kayıt için kullanır; ikinci kayıt yapılmaz. **Doğrudan analitik API sınırı:** analytics/direct.rs yalnız sunucu/dakika toplamını tutar; istemci etiketi veya istek kaydı API'si yoktur. Telemetry toplamları manuel bir direct_add ile iki kez sayılmaz. İstemci bazlı doğrudan kayıt için API genişletmesi gerekir.

Create sayfasının manuel bileşeni `create/ToolGenerator.svelte` içindedir; Playground'ın korunan diyaloğu değiştirilmemiştir. Backend taslak tokenları her iki çağrı yolunda da döner; yeni manuel tahmin/geçmiş görünümü Create sayfasındadır.

### Üretici Hata İşleme

Dizin araçları `harness.fs.list(path)` kullanır; sonuç ada göre sıralanmış `{name, dir, size}` dizisidir ve `dir` bir boolean değeridir. `read_directory` adlı bir host işlevi yoktur. Boş dizin test verisi `{path: "empty", dir: true}` biçimindedir ve `content` içermez. Dosya test verileri üst dizinlerini oluşturur. Hiç test çalışmadan oluşan kurulum/yükleme hataları yanıltıcı `failed: 0` sayısı yerine nedenleriyle gösterilir. Başarılı denemelerde geçen test sayısı gösterilir.

Araç oluşturucu varsayılan olarak ağ erişimi istemez. Onaylama/kaydetme öncesinde ağ, dosya okuma, dosya yazma ve komut çalıştırma izinleri ayrı gösterilir. Kaydedilen oluşturucu araçları **Kullanıcı Tarafından Oluşturulan**, katalog ve paket araçları **Katalogdan veya Paketten Kurulan** etiketi taşır. Kaynak bilgisi olmayan eski paketler paket etiketiyle kalır.

### Ajan Becerileri

**Oluştur → Araçlarım** veya **Ayarlar → Araçlar** altında **Ajan Becerileri** bölümünü açın. **Beceriler Klasörünü Aç**, `<veri dizini>/skills` klasörünü açar. `<beceri-adı>/SKILL.md` dosyasına YAML ön bilgisinde klasör adıyla eşleşen `name` ve bir `description`, ardından Markdown yönergeleri ekleyin. İsteğe bağlı metadata kabul edilir; `allowed-tools` izin vermez. Biçim için [Agent Skills standardına](https://agentskills.io/specification) bakın.

Yerel değişiklikleri keşfetmek için yenileyin. Etkinleştirme/devre dışı bırakma klasörü korur; kaldırma onayı beceriyi ve kaynaklarını kalıcı olarak siler. Geçersiz becerilerin hatası gösterilir ve bu beceriler modele sunulmaz. Resmî katalog becerileri sabitlenmiş katalog sürümünden incelenip kurulabilir. Mevcut katalog bağımsız `SKILL.md` dosyaları sunar; yerel beceri klasörleri kaynak dosyaları da içerebilir.

Playground araçları etkinken sistem bağlamına yalnızca etkin becerilerin adları ve açıklamaları eklenir. Model, görevle ilgili becerinin yönergelerini `load_skill` aracının `name` alanıyla, ilgili UTF-8 kaynaklarını ise `resource` alanıyla (örneğin `references/guide.md`) ister. Dosyalar 1 MiB ile sınırlıdır ve sembolik bağlantılar çözüldükten sonra da beceri klasörü içinde kalmalıdır. Kaynaklar ve betikler okunur, otomatik çalıştırılmaz. Mevcut araç izinleri geçerlidir. Yükleme, modelin araç çağırma desteğine ve seçimine bağlıdır; ilgisiz beceri içerikleri her isteğe eklenmez.

### Diğer Oluşturucu Kurtarma Davranışları

Metin/veri dönüşümleri (CSV/JSON, HTML, Markdown, base64) planlama ve üretim istemlerinde varsayılan olarak string içerik alıp içerik döndürür; dosya sözleşmesi yalnız açık dosya işlemlerinde önerilir. Mevcut dosya sözleşmeli paketler fixture testleriyle desteklenir; izin onayı sürer. QuickJS DOM sağlamaz (`DOMParser` ve `document` yoktur). Markdown için satır/blok ayrımı, başlık seviyeleri ve satır sonu olmayan son satır istemde açıkça belirtilir.

Sayaç şeridi pencere genişliğine bağlı 2/3/4/7 sabit grid sütunu kullanır; sayı değişimi sütun sayısını veya etiket yerini belirlemez. Sayılar `font-num`, filtre sayıları sabit `3ch`, kart/manuel tokenları minimum `12ch` alan kullanır. Canlı günlük ve deneme tokenları tabular rakam kullanır. Başarısız kart kısa TR/EN özet verir; teknik hata, test çıktısı ve geçmiş **Paketi İncele** altında kalır.

Gerçek bir üretim kaydındaki dört paketin kod/test/manifest kopyaları `crates/harness-core/tests/data/tool-generator-reported-failures.json` içindedir. CSV fixture regresyonu önce `path does not exist: input.csv` ile başarısız oldu, düzeltmeden sonra geçti. Gerçek kayıt kodlarıyla dosya yokluğu ve Markdown'ın `<p><h1>Header 1</h1><br>#<h1>Header 2</h1><br>### Header 3</p>` çıktısı yeniden üretilir. Base64 aynı kodla fixture eklendiğinde geçer; HTML fixture sonrasında ikinci hata olan eksik `DOMParser` görünür. Geç Markdown tekrarı, sahte chat uç noktasıyla beşinci turda kod düzeltmesini kabul edip testleri geçirir. Bu testler gerçek bir modelin yeni üretim başarısını garanti etmez.
