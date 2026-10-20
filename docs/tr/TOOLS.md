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


### Ajan Becerileri

**Oluştur → Araçlarım** veya **Ayarlar → Araçlar** altında **Ajan Becerileri** bölümünü açın. **Beceriler Klasörünü Aç**, `<veri dizini>/skills` klasörünü açar. `<beceri-adı>/SKILL.md` dosyasına YAML ön bilgisinde klasör adıyla eşleşen `name` ve bir `description`, ardından Markdown yönergeleri ekleyin. İsteğe bağlı metadata kabul edilir; `allowed-tools` izin vermez. Biçim için [Agent Skills standardına](https://agentskills.io/specification) bakın.

Yerel değişiklikleri keşfetmek için yenileyin. Etkinleştirme/devre dışı bırakma klasörü korur; kaldırma onayı beceriyi ve kaynaklarını kalıcı olarak siler. Geçersiz becerilerin hatası gösterilir ve bu beceriler modele sunulmaz. Resmî katalog becerileri sabitlenmiş katalog sürümünden incelenip kurulabilir. Mevcut katalog bağımsız `SKILL.md` dosyaları sunar; yerel beceri klasörleri kaynak dosyaları da içerebilir.

Playground araçları etkinken sistem bağlamına yalnızca etkin becerilerin adları ve açıklamaları eklenir. Model, görevle ilgili becerinin yönergelerini `load_skill` aracının `name` alanıyla, ilgili UTF-8 kaynaklarını ise `resource` alanıyla (örneğin `references/guide.md`) ister. Dosyalar 1 MiB ile sınırlıdır ve sembolik bağlantılar çözüldükten sonra da beceri klasörü içinde kalmalıdır. Kaynaklar ve betikler okunur, otomatik çalıştırılmaz. Mevcut araç izinleri geçerlidir. Yükleme, modelin araç çağırma desteğine ve seçimine bağlıdır; ilgisiz beceri içerikleri her isteğe eklenmez.
