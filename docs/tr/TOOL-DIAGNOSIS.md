# Araç çağrısı teşhisi

[English](../en/TOOL-DIAGNOSIS.md)

Soru: "LM Studio + OpenCode'da sürekli araç hatası alıyordum, suçlu kim?" Bir araç turunda üç taraf vardır ve hata türü suçluyu gösterir:

- **Sunucu / şablon (runtime):** llama-server ve modelin sohbet şablonu. Çağrıyı ayrıştırır, araç tanımlarını modele koyar.
- **Model:** çağrıyı yazan, doğru araç adını ve geçerli argümanları seçen taraf.
- **İstemci (OpenCode, Claude Code, Codex...):** aracı çalıştıran, sonucu bir sonraki istekte geri gönderen taraf.

Dışarıdan hepsi "araç çalışmadı" görünür. Gateway her isteği ve yanıtı gördüğü için turu sınıflandırıp istek kaydının yanına yazar. Kod: `harness_core::gateway::tool_diag` (arayüzden bağımsız, `axum`'suz); gateway yalnız bağlar (`crates/harness-gateway/src/proxy.rs`).

## Ne ölçülür

### Metinde kalan çağrının kurtarılması

`call_left_in_content` teşhisi ham upstream yanıt hakkında geçerliliğini korur; teşhis sınıflandırıcısı yanıtı değiştirmez. Kurtarma ajan katmanındadır: `tools/text_call.rs` içindeki tek ayrıştırıcıyı Playground Tauri `tools_recover_text_calls` komutuyla, çekirdeğin `run_agent` ve `run_agent_stream` döngüleri doğrudan kullanır. Düz proxy istekleri otomatik olarak değiştirilmez; gateway'in sunucu araçlı ajan döngüsü kurtarma uygular.

Koşullar: gerçek `tool_calls` yok, o istekte araç sunulmuş, tam JSON içindeki ad sunulan adlardan biri ve `arguments`/`parameters` (string-JSON dahil) bir nesne. Güçlü sarmalayıcılar (`<tool_call>`, `<tools>`, `<function=ad>`, `[TOOL_CALLS]`, `<|python_tag|>`) açıklama içinde de kurtarılır. Zayıf biçimler (her dil etiketli kod bloğu, XML dahil; yalın JSON/dizi/ardışık nesneler) yalnız çağrı(lar) çıkarıldığında boşluk veya çağrılardan önce gelen en fazla bir satır/120 Unicode karakter giriş kalıyorsa kurtarılır. Son açıklama veya bilinmeyen/geçersiz bir dizi üyesi varsa zayıf çağrı kurtarılmaz; metin aynen kalır. Eşik kısa “Arıyorum.” girişini kabul eder, çok satırlı anlatıyı ve sondaki kullanım açıklamasını reddeder; yalnız kısa ön giriş taşıyan bir örneğin niyeti biçimden kesin ayırt edilemez. Normal izin/şema/onay yolu korunur.

Kimlikli asistan `tool_calls` ve eşleşen `tool_call_id` geçmişe yazılır; kartta TR/EN kurtarma rozeti korunur. İptal/hata ve `length`/`content_filter` kurtarılmaz. Normal metin araçlar açıkken de anlık akar; yalnız olası çağrı başlangıcından sonrası ve kısmi işaret suffix’i bekletilir, kurtarılamazsa aynen gösterilir. TS ve Rust akış filtresi `shared/text-tool-call-markers.json` içindeki aynı işaret listesini kullanır; tam yanıtın ayrıştırılması Rust’tadır.

Başlatma incelemesi: `process.rs` kataloğu `params.jinja=true` → `--jinja`, `false` → `--no-jinja` olarak işler; `params.toml` ilgili iki bayrağı tanımlar. Jinja açık olması modelin her çağrısını native biçimde ayrıştıracağını garanti etmez: depodaki Qwen2.5-Coder-14B kaydı `<tools>` içinde JSON bırakır. Bu çalışma şablonu veya kayıtlı başlatma ayarını değiştirmez. Yeni canlı doğrulama denemeleri ve sınırları `TEXT-TOOLCALL-VALIDATION.md` içindedir.

Bir istek **araç turuna dahil** sayılır: istek `tools` gönderiyorsa ya da konuşmada araç sonucu (`role: tool`, `tool_result`, `function_call_output`) varsa. Araç içermeyen istekler hiçbir şey yazmaz (`tool_involved = 0`). Üç tel biçimi okunur: Chat Completions, Anthropic Messages, Responses; JSON ve SSE (akış) olarak. Yanıt metni ve araç çağrıları akarken sınırlı bellekle toplanır (metin 32 KiB, çağrı başına argüman 64 KiB); içerik kaydı ayarından bağımsızdır ve hiçbir içerik saklanmaz, yalnız bulgunun kanıtı (araç adı, kısa alıntı, şema yolu) `tool_diag_detail`'e yazılır.

Her isteğe şunlar yazılır (`analytics.db` şema **v5**, `requests` tablosu):

| Sütun | Anlam |
|---|---|
| `tool_involved` | tura dahil mi |
| `tools_offered` | istekteki araç sayısı |
| `tool_calls` | yanıttaki araç çağrısı sayısı |
| `tool_diag_category` | `runtime` / `context` / `model` / `client`; sorun yoksa boş |
| `tool_diag_code` | tam bulgu kodu |
| `tool_diag_detail` | JSON kanıt |

Taşıma `ALTER TABLE ... ADD COLUMN` ile yapılır: eski satırlar korunur ve "araç yok" okunur (testli: v4 veritabanı verisiyle açılır). `requests_tool` ve `requests_tool_issue` kısmi dizinleri filtreyi ucuz tutar.

## Kurallar

Sınıflandırma `classify(istek, yanıt, tur)` **saf bir fonksiyondur**: aynı girdi aynı sonucu verir. Bir tur en fazla bir **birincil** bulgu alır; aşağıdaki sırayla ilk eşleşen kazanır (sağlam kanıt önce). Birincil bulgu bir model/sunucu bulgusuysa ve önceki turun araç sonucu da hataysa, istemci hatası kanıtın içinde `also` / `client` olarak saklanır.

| Kategori | Kod | Ne zaman |
|---|---|---|
| bağlam | `context_exceeded` | llama-server `exceed_context_size_error` döndü (istem bağlama sığmadı); kanıt: `n_prompt_tokens`, `n_ctx` |
| bağlam | `output_truncated` | bitiş nedeni `length` ve yanıt çağrının içinde kesildi (yarım argüman ya da yarım işaret) |
| runtime | `template_no_tools` | araçlar gönderildi, çağrı yok ve şablon araç desteklemiyor (`/props` → `chat_template_caps.supports_tools = false`) |
| runtime | `call_left_in_content` | çağrı yok ama metin bilinen araç işareti taşıyor (`<tool_call>`, `<tools>`, `<function=`, `[TOOL_CALLS]`, `<\|python_tag\|>` ...) ya da yanıt `name` + `arguments` içeren bir JSON nesnesiyle başlıyor |
| runtime | `stream_broken` | akışta çağrı parçası `function` ya da `name` olmadan geldi, bir parça JSON değil ya da bitiş nedeni çağrı diyor ama çağrı yok |
| bağlam | `context_near_full` | araçlar var, çağrı yok ve (istem + yanıt) / bağlam >= %90 (bağlam kayması araç tanımlarını düşürür) |
| model | `invalid_json_args` | argümanlar JSON nesnesi değil |
| model | `unknown_tool` | çağrılan ad istekteki araçlarda yok |
| model | `schema_mismatch` | argümanlar aracın şemasına uymuyor (`type`, `enum`, `required`, `minimum`/`maximum`, `items`, `additionalProperties`...; kanıt: alan yolu ve kural) |
| model | `repeated_call` | aynı çağrı (ad + argümanlar, anahtar sırasından bağımsız) konuşmada üçüncü kez: iki kez denenip sonuç değişmediyse döngüdür |
| model | `no_call_when_required` | `tool_choice` çağrı zorunlu kıldı (`required` / `any` / adlı fonksiyon) ve çağrı yok |
| istemci | `tool_result_error` | istemcinin bu istekte gönderdiği araç sonucu hata: `is_error: true`, `{"error": ...}` / `success: false` JSON'u ya da metin hata biçiminde başlıyor (`Error`, `No match found`, `ENOENT`, `Permission denied`...) |

Bilinçli seçimler:

- Araç sonuçlarında metin deseni **başa bağlıdır** ("Error: ..." ile başlar); içeriğinde "error" geçen bir dosya normal sonuçtur. Yalnız son asistan turundan sonraki sonuçlar sayılır (eskileri önceki satırlar zaten açıkladı).
- "Araç çağrısı yok" tek başına sorun değildir: `tool_choice` otomatikken modelin metinle cevap vermesi geçerlidir. Sorun, çağrı zorunluyken ya da çağrı metinde kaldığında başlar.
- Başka bir upstream hatası (4xx/5xx, bağlam dışında) teşhis üretmez; durum alanı onu zaten söyler.
- Şema doğrulaması modelin kusurunu ölçtüğü için katıdır (çalışma zamanındaki `validate_args`'ın aksine sayı metnini kabul etmez). Tanımadığı anahtar sözcüğü (`$ref`, `pattern`, `format`) kabul eder: emin olamadığı yerde sessiz kalır.

### İstemci hatasının önceki isteğe bağlanması

İstemci hatası **bir sonraki istekte** görünür; hangi çağrıya ait olduğunu bulmak için çağrı kimliği kullanılır, tahmin yürütülmez: llama-server her çağrıya bir kimlik yazar, istemci sonucu aynı kimlikle (`tool_call_id` / `tool_use_id` / `call_id`) geri gönderir. Gateway çağrı kimliklerini son 4096 çağrılık bir dizinde (`CallIndex`, bellekte) tutar; sonuç geldiğinde kimlik aranır ve kanıta `linked_request` (çağrıyı yapan isteğin kimliği) eklenir. Kendi kimliğini uyduran bir istemci bağlanmaz; teşhis yine "araç istemci tarafında hata verdi" der. Dizin gateway yeniden başlayınca boşalır.

## Model düzeyi (native / generic / limited)

Model adından tahmin edilmez; gateway günlüğünden **ölçülür**. `tools_offered > 0` olan bir tur:

- **başarı:** temiz çağrı (çağrı var, runtime/model bulgusu yok);
- **başarısızlık:** runtime ya da model bulgusu;
- sayılmaz: metinle cevap, bağlam bulgusu, istemci hatası, iptal (modelin kusuru değil).

Başarı oranı `ok / (ok + bad)`; en az **5 tur** gerekir, yoksa düzey "henüz ölçülmedi" kalır.

| Düzey | Koşul |
|---|---|
| `native` | oran >= 0,85 (ve şablon araç destekliyor ya da bilinmiyor) |
| `generic` | 0,50 <= oran < 0,85; ya da oran yüksek ama şablon araç desteklemiyor (çağrılar llama.cpp'nin genel biçimiyle çalışıyor) |
| `limited` | oran < 0,50 |

Düzey, Bağlantılar'daki ajan modeli notunda (`AgentModelNotes`) mevcut araç çağrısı doğrulamasının (`Verified` / `Failed` / ...) altında gösterilir; `harness_core::integrations::agent_models::AgentModel` alanları: `tool_rounds_ok`, `tool_rounds_bad`, `tool_level`, `recommended_tools`.

### Önerilen en fazla etkin araç sayısı

Her etkin araç bağlamdan yer alır (tanımı ~150-250 token) ve modelin doğru seçmesi gereken listeyi büyütür; küçük modeller uzun listede hızla hata yapar (OpenCode ve Claude Code 10-30 araç gönderir). Kural model boyutuna göredir:

| Parametre | En fazla araç |
|---|---|
| < 2 B | 4 |
| 2 - 5 B | 8 |
| 5 - 10 B | 12 |
| 10 - 20 B | 20 |
| >= 20 B | 32 |
| bilinmiyor | 8 |

Ölçülen düzey sınırı düşürür: `generic` en fazla 8, `limited` en fazla 4. Bu bir öneridir, sınır değildir; gateway araç sayısını kısıtlamaz. İlk gerçek OpenCode ölçümü aşağıdadır; eşiklerin ve boyut basamaklarının değişmesi için daha geniş ve karşılaştırmalı kanıt gerekir.

## Eşik ölçümü

### Kurulum ve yöntem

Windows 11, Ryzen 7 9700X; RX 7900 GRE'nin boş VRAM'i yaklaşık 1,5 GB olduğu için dört model de **CPU** üzerinde çalıştırıldı. Kullanıcının 11440'taki 14B sunucusu durdurulmadı. Resmi [llama.cpp b11342 Windows CPU sürümü](https://github.com/ggml-org/llama.cpp/releases/tag/b11342), GGUF içindeki sohbet şablonu, `--jinja -ngl 0 -c 32768 -np 1 -t 8` kullanıldı; model dosyaları `D:\Models` altında yerinde, salt okunur açıldı. Gateway aynı anda yalnız bir ölçüm modeli yükledi.

Hazır headless `harness` ikilisi (0.1.0) ile `harness serve --no-restore --gateway --gateway-port 11535 --port 11536` çalıştırıldı. `HARNESS_DATA_DIR`, analiz veritabanı ve geçici çalışma dosyaları sürüm denetiminin dışındaki geçici bir dizindedir. Gateway sürecinde `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `CLAUDE_CONFIG_DIR` ve `CODEX_HOME` da bu dizinin altına yönlendirildi; yalnız `HARNESS_DATA_DIR` istemci ayarlarını izole etmeye yetmez.

OpenCode **1.18.34** ayrı npm kurulumundan, `opencode run --pure --model measurement/<alias> --format json` ile çalıştırıldı. `OPENCODE_CONFIG`, `OPENCODE_CONFIG_DIR`, dört XDG dizini ve `OPENCODE_TEST_HOME` izole edildi; proje ayarı, Claude Code ayarı, harici beceri ve model kataloğu okuma kapatıldı. Fixture ayrı git deposudur; miras kalan `PWD` ve `INIT_CWD` de fixture'a sabitlendi. `small_model` başlık üretimi için kullanılır; araç sunulmayan bu yardımcı istekler düzey hesabına girmez.

Sekiz sabit görev, her seferinde aynı başlangıç dosyalarıyla ayrı oturumda verildi: README'nin ilk satırını okuma, `.txt` dosyalarını glob ile listeleme, `NEEDLE_642` satırını grep ile bulma, `mode=old` → `mode=new` düzenlemesi, `CREATED_642\n` içeren dosya yazma, `node --version`, oku/düzenle/yeniden oku, iki dosyadan özet yazıp yeniden okuma. Araçlar OpenCode'un kendi `bash`, `edit`, `glob`, `grep`, `read`, `skill`, `todowrite`, `webfetch`, `write` listesidir (**9 araç**). İstemciye özel kısaltılmış sistem istemi veya elle yapılmış araç isteği kullanılmadı. Bu izole sürümün gerçek istemi yaklaşık 6,5 bin token çıktı; önceki ~29 bin token gözlemi bu kurulumda yeniden oluşmadı.

Çıktı sınırı 2048 token, `tool_choice: auto`, ajan adım sınırı 8 ve görev zaman aşımı 600 saniyeydi. İstek tarafında sıcaklık/tohum zorlanmadı; sonuçlar bu sürümün varsayılan örneklemesine aittir. Görev başarısı, OpenCode'un cevabı ve fixture'daki gerçek dosya içeriği birlikte incelenerek ayrıca değerlendirildi; “başarıyla yaptım” cümlesi yeterli sayılmadı. Paralel bir yanıtta üç çağrı olsa da **tek gateway isteği bir turdur**.

Sayımlar izole `analytics.db` üzerinden, `analytics/db.rs::agent_signals` ve `agent_models.rs::build` ile aynı koşullarla hesaplandı: `tools_offered > 0`, `cancelled = 0`; çağrı varsa ve birincil kategori `runtime`/`model` değilse `ok`, kategori bu ikisinden biriyse `bad`. GGUF şablonunda `tools`/`tool_call` bulunması ve tensör parametre toplamı ürünün GGUF okuma kuralıyla kontrol edildi. `MIN_ROUNDS = 5`, 0,85 / 0,50 ve mevcut araç önerileri aynen uygulandı. Kodun ayrıntısı: çağrı taşıyan bir `client`/`context` satırı mevcut SQL'de `ok` olabilir; kategori dökümü ayrıca verilir.

### Sonuçlar

| Model | Parametre | Şablonda araç | Sayılan tur | Temiz (`ok`) | Başarısız (`bad`) | Oran | Düzey | Önerilen araç | Görev başarısı (dosya ile doğrulanmış) | Ort. görev süresi |
|---|---|---|---|---|---|---|---|---|---|---|
| Qwen2.5-0.5B-Instruct Q4_K_M | 630 M | var | 8 | 8 | 0 | 1,00 | native | 4 | 3/8 | 42 sn |
| Qwen2.5-1.5B-Instruct Q4_K_M | 1,8 B | var | 9 | 9 | 0 | 1,00 | native | 4 | 6/8 | 74 sn |
| Qwen3-4B-Instruct-2507 Q4_K_M | 4,0 B | var | 14 | 14 | 0 | 1,00 | native | 8 | 7/9 | 80 sn |
| gemma-3-4b-it Q4_K_M | 3,9 B | yok | 8 | 0 | 8 (`runtime`: `template_no_tools`) | 0,00 | limited | 4 | 0/8 | 10 sn |

Sayılmayan istekler: araç sunulmayan başlık/yardımcı istekler (her modelde 8-9) ve aracı çağırmadan metinle biten turlar. Hiçbir modelde `model` kategorisinde bulgu (geçersiz JSON, olmayan araç, şemaya uymayan argüman) çıkmadı; llama.cpp araç adını ve şemayı ızgara ile zorluyor. Qwen3-4B'nin `multi` görevi iki kez denendi, ikisinde de özet dosyası eksik yazıldı. gemma-3-4b her görevde çağrıyı ```` ```tool_code ```` bloğu içinde düz metin olarak yazdı ya da yalnız tanıtım cümlesi verdi; OpenCode hiçbir aracı çalıştırmadı.

### Gözlemler ve karar

- **0,85 / 0,50 değişmedi.** Ölçülen oranlar uçlarda toplandı (üç Qwen 1,00, gemma 0,00); eşiklerin arasına düşen model yok, dolayısıyla eşikleri kaydıracak kanıt yok. Dört model de gözlenen çağrı davranışına uygun düzeye düştü: Qwen'ler çağrıları her turda yapılandırılmış `tool_calls` olarak verdi, gemma'nın şablonu araç desteklemediği için hiç çalıştırılabilir çağrı çıkmadı.
- **Düzey görev başarısını ölçmez.** Temiz çağrı oranı yalnız çağrının biçimini ve şemasını anlatır: 0,5B model `native` olduğu halde görevlerin yalnız 3/8'ini doğru tamamladı (boş okuma aralığı, değişmemiş dosyayı değişti sanma, özet yerine görev metnini yazma). Bu nedenle arayüzdeki düzey "araç çağrısı biçimi" olarak okunmalı; anlamsal başarı ayrıca görünmez.
- **Boyut basamakları değişmedi.** OpenCode 9 araç gönderdi. 2 B altındaki modellerin önerisi 4: bu modeller 9 araçla belirgin biçimde zorlandı (3/8 ve 6/8). Qwen3-4B'nin önerisi 8: 9 araçla 7/9, yalnız çok adımlı görevde kaldı. Gözlem basamaklarla çelişmiyor. 5 B üstü basamaklar bu turda ölçülmedi (14B boyutu VRAM'e sığmadı; 11440'taki kullanıcı sunucusu durdurulmadı).
- **Sınırlar:** tek istemci (OpenCode 1.18.34), model başına 8-14 tur, CPU, varsayılan örnekleme; Claude Code ölçülmedi. Arada kalan oranlar (0,5-0,85) görüldüğünde eşik yeniden değerlendirilmeli.

0,5B modelin ilk sekiz görevinde somut sonuç 3/8 idi (listeleme, tek satır düzenleme, shell). Okumada `limit: 0` ile boş aralık istedi; yeni dosyada son satır sonunu atladı; çok adımlı düzenlemede dosyayı değiştirmeden değiştirdiğini söyledi; özet dosyasına gerçek değerler yerine görevdeki açıklamayı yazdı. Bunlar geçerli araç adı ve JSON/şema taşıdığı için temiz çağrı oranı yüksek kaldı. Grep dosyayı bildirdi ama istenen satır numarasını vermedi.

1,5B modelde ilk sekiz görev 6/8 somut sonuç verdi. Grep aranan sabiti küçük harfe çevirip eşleşmeyi kaçırdı. Özet dosyasına `$(cat ...)` ifadelerini **düz metin** yazdı, ardından doğru bir özet oluşturduğunu iddia etti. Diğer dosya işlemleri ve shell görevi tamamlandı. Dolayısıyla `native` düzeyi, burada “ajan görevlerini güvenilir biçimde tamamlıyor” şeklinde okunamaz; yalnız gözlenen çağrı biçimi/şema başarısını anlatır.

### Ölçüm izolasyonu

Ölçümde gateway'in `HARNESS_DATA_DIR`, `USERPROFILE`, `APPDATA` ve `LOCALAPPDATA` dizinleri ayrılır; gerçek istemci yapılandırma dosyalarının özeti ölçüm boyunca değişmez.

Bu çalışma bir ürün hatasını ortaya çıkardı ve düzeltildi (`integrations::drift::follow_gateway_port`): port takibi, gateway'in hiç dinlemediği varsayılan 11435'i her zaman "eski gateway portu" sayıyordu; ayrık veri klasörlü bir örnek de kullanıcının gerçek istemci ayarını kendi portuna çekebiliyordu. Taşıma artık yalnız bu veri klasörünün gateway port geçmişindeki portlardan yapılır; uyarı (adres farkı algılama) varsayılan portu saymaya devam eder.

## Yüzeyler

- **İstek İnceleyici** (Analitik): satırda "kategori: bulgu" tek satırı (öneri fare üstünde), ayrıntı çekmecesinde teşhis bölümü (başlık, öneri, kanıt, çağrıyı yapan isteğe bağlantı), filtre "Araç sorunu" (hepsi / sorunu olanlar / sorunsuz turlar / kategori).
- **Control API:** `GET /analytics/requests?tool_issue=true` ve `tool_category=runtime|context|model|client`; `RequestRow` yeni alanları taşır.
- **CLI:** `harness requests --tool-issue [--tool-category model]`; `harness requests <id>` "Tool diag" satırlarını yazar.
- Öneri metinleri arayüz dilinde (`src/lib/locales/toolDiag.ts`); çekirdek yalnız kod ve kanıt üretir. Bağlam önerisi ("32k") kanıttan hesaplanır: `n_prompt_tokens × 1,25 + 2048` ya da mevcut bağlamın iki katı, hangisi büyükse, 8k / 16k / 32k / 64k / 128k / 256k adımlarına yuvarlanır.

## Testler

Birim testleri **gerçek llama-server yakalamalarıyla** çalışır (`crates/harness-core/tests/data/tooldiag/`, b11342, dosya adı: `b11342-<model>.<durum>.<req|resp>`): Qwen3-4B-Instruct-2507 (üç tel biçiminde JSON ve SSE temiz çağrı, araç sonucu sonrası cevap, hata sonucu sonrası cevap, `max_tokens` ile kesilen çağrı, bağlam aşımı hata gövdesi, zorunlu çağrıda çağrısız cevap), Qwen2.5-Coder-14B (`<tools>` ile metinde kalan çağrı, JSON ve SSE), gemma-3-4b (şablonda araç yok), Qwen2.5-0.5B ve gpt-oss-20b (üç kez tekrarlanan çağrı). llama.cpp ızgara (grammar) ile adları ve şemayı zorladığı için bu modellerde gerçek bir "olmayan araç adı" ya da "şemaya uymayan argüman" yakalanamadı; o iki kural gerçek yakalanmış bir yanıtın başka bir isteğin araç listesine karşı değerlendirilmesiyle test edilir (testin açıklamasında yazılı) ve şema doğrulayıcısı kendi birim testlerine sahiptir. İstemcinin kendi yazdığı baytlar (Anthropic `is_error` sonucu) gerçek model çıktısı değil, istemci verisidir.

Uçtan uca (`#[ignore]`, `crates/harness-gateway/tests/tooldiag_e2e.rs`): gerçek llama-server ile gateway üzerinden — yerel çağrı üç tel biçiminde temiz, istemci hatası önceki isteğe bağlanır, çıktı sınırında kesilme, metinde kalan çağrı (JSON ve akış), şablonu araç desteklemeyen model; araç içermeyen istek tura dahil değildir.


## Gateway metin çağrısı kurtarma

Ayarlar > Gateway > Yönlendirme’de **Metinde Kalan Araç Çağrılarını Gerçek Çağrıya Çevir** model başına açılır. `GatewaySettings.text_tool_calls` tanım kimliği → boolean haritasıdır; **varsayılan açıktır** (Qwen2.5-Coder-14B her çağrıyı `<tools>{json}</tools>` metni olarak yazıyor, kurtarma kapalıyken araç testi 5/38, açıkken 36/38). Yalnız kapatılan modeller `false` olarak kaydedilir (`settings.json` > `gateway.text_tool_calls`: `{"<tanım kimliği>": false}`); kayıt yoksa kurtarma çalışır. İstekte araç yoksa gateway yanıta hiç dokunmaz. Anahtar Ayarlar > Gateway > Yönlendirme > "Metinde Kalan Araç Çağrılarını Gerçek Çağrıya Çevir" altında her model için açık görünür. Sunucu taraflı araçlar gibi aynı gateway ayarı kaydedilir; değişiklik yeni isteğe uygulanır, model yeniden başlatılmaz. Bağlantılar’da araç düzeyi `limited` olan model için bu ayara bağlantı gösterilir.

Kurtarma `proxy.rs` içinde, uyumluluk işleminden sonraki Chat Completions isteğinde sunulan araçlarla ve yalnız başarılı (200) yanıtta uygulanır. `harness-core::tools::text_call` ayrıştırıcısı ve `TextCallStreamBuffer` yeniden kullanılır; işaretler `shared/text-tool-call-markers.json` kaynağından gelir. Güçlü sarmalayıcılar açıklama içinde de kabul edilir; çit/yalın JSON yalnız core’un çağrıdan ibaret yanıt / kısa giriş kurallarına uyarsa kabul edilir. Araç şeması yankıları, sunulmayan adlar, native çağrı içeren yanıtlar, `length`/hata/eksik bitişler kurtarılmaz. Argüman şema doğrulaması ve çalıştırma istemcinin sorumluluğundadır; gateway yalnız tel biçimini dönüştürür.

JSON’da tek `choices[0].message` için `tool_calls`, boşsa `content: null` ve `finish_reason: tool_calls` yazılır; açıklama ve usage korunur. Çok seçimli yanıtlar dönüştürülmez. SSE’de sıradan metin geldiğinde iletilir, olası çağrı işaretinden sonraki metin tamponlanır. `stop` olayında ortak ayrıştırıcı tam metni değerlendirir; çağrılar index/id/type/function.name/function.arguments içeren `delta.tool_calls` olaylarıyla bitiş olayından önce iletilir. Kurtarma yoksa tampon aynen içerik olarak gönderilir. Upstream usage ve `[DONE]` sırası korunur; UTF-8/TCP sınırları ve CRLF ayrık olabilir. Tam yanıt metni 1 MiB’ı aşarsa kurtarma bırakılıp tampon boşaltılır.

JSON’da `X-Harness-Compat: text_tool_calls=<n>` normal yanıt başlığıdır. SSE başlıkları normal metinden önce gönderildiği ve sayı ancak bitişte bilindiği için `Trailer: x-harness-compat` ilan edilir; sayı aynı adlı **HTTP trailer** olarak `[DONE]` sonrasında gider (HTTP/1 istemcisi `TE: trailers` göndermelidir). Trailer’ları okumayan istemciler analitik kaydından sayıyı görebilir. Tüm akışı baştan tamponlayarak TTFT’yi geciktirmez.

Teşhis upstream byte’larını gözlemlemeye devam eder: `call_left_in_content` bulgusu korunur, `tool_diag_detail` içine `recovered: true` ve `recovered_calls` eklenir; `compat` kaydına sayı yazılır. `tool_calls` native sayaçtır, kurtarılan çağrılar native başarı gibi sayılmaz. Analitik istek ayrıntısı kurtarılan çağrı sayısını gösterir ve öneri metninde kurtarmayı açıklar. Kurtarılan id’ler `CallIndex` içine kaydedilir; sonraki istemci araç hata sonucu ilk istekle eşleştirilebilir. SSE istemcisi `[DONE]` sonrasında bağlantıyı kapatsa da kurtarma işareti kayıtta korunur. Eski sınıflandırıcı ortak ayrıştırıcının bildiği bir biçimi tanımazsa kurtarma kanıtı `call_left_in_content` olarak kaydedilir; önceki farklı bulgu `upstream_finding` içinde korunur.

Anthropic Messages ve Responses gateway’de Chat Completions’a çevrilmez; aynı uç noktaya upstream edilir. Bu ayar yalnız Chat Completions proxy yoluna uygulanır. Sunucu taraflı araç döngüsü ayrı bir yoldur ve mevcut core kurtarmasını kullanmaya devam eder; bu bayrak onun davranışını değiştirmez. Ollama biçimi bu kapsamda değildir.
