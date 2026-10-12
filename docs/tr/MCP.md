# Harness MCP sunucusu

[English](../en/MCP.md)

Kodlama ajanları (Claude Code, Codex, OpenCode, Cursor...) Harness'ı araç olarak kullanır: yerel modelleri listeler, belleğe sığanı seçer, yükler, OpenAI uyumlu uç noktayı alır, metrik/log okur, benchmark çalıştırır. Kaynak: `crates/harness-mcp`.

## Taşıyıcılar

| | stdio | Streamable HTTP |
|---|---|---|
| Başlatma | Ajan `harness mcp` komutunu kendisi çalıştırır | Control API: `POST http://127.0.0.1:11500/api/harness/v1/mcp` |
| Kimlik | Yerel oturum token'ı (CLI keşfi) ya da `--host` + `HARNESS_TOKEN` | `Authorization: Bearer <isimli token>` (oturum token'ı da geçer) |
| Kapsam | `--scope` (varsayılan `read,servers:write,tools`) | Token'ın kapsamları |
| Önerilen | Evet: yapılandırmaya sır yazılmaz | Uzak makine, stdio başlatamayan istemciler |

`harness mcp` diğer CLI komutları gibi çalışır ([CLI.md](CLI.md)): çalışan sahibin (GUI'de Ayarlar > Geliştirici > Control API açık, ya da `harness serve`) Control API'sini kullanır. Sahip her araç çağrısında yeniden bulunur. Böylece GUI sonradan açılsa da, oturum token'ı yenilense de bağlantı çalışır. Çalışan örnek yoksa araçlar `cli.no_instance` hatası döner; MCP bağlantısı yine kurulur. `--headless` verilirse süreç içinde geçici bir örnek açılır, stdin kapanınca başlattığı sunucularla birlikte kapanır. stdout'a yalnız MCP mesajı yazılır, günlük stderr'e gider.

HTTP'de her araç, API yönlendiricisine süreç içinde, çağıranın token'ıyla istek atar. Kapsam ve çekirdek kuralları böylece bir kez MCP'de, bir kez API'de, aynı tabloyla denetlenir.

## Protokol

El yazımı, çift dönemli (dual-era) JSON-RPC:

- **2026-07-28** (modern, durumsuz): her istekte `params._meta` (`io.modelcontextprotocol/protocolVersion`, `.../clientCapabilities`), `server/discover`, sonuçlarda `resultType: "complete"`, `tools/list`/`server/discover` için `ttlMs` + `cacheScope` (`tools/list` token'a göre değiştiği için `private`). HTTP'de `MCP-Protocol-Version`, `Mcp-Method` ve `tools/call` için `Mcp-Name` gövdeyle aynı olmalı (`-32020`, 400; `=?base64?…?=` biçimi çözülür). Desteklenmeyen sürüm `-32022` (400, `supported` listesiyle). Eksik `_meta` alanı `-32602` (400). Bilinmeyen yöntem 404 + `-32601`.
- **2025-11-25, 2025-06-18, 2025-03-26, 2024-11-05** (eski, `initialize` el sıkışmalı): bugün istemcilerin çoğu bunu konuşur. Sunucu oturum tutmaz. `Mcp-Session-Id` üretmez, GET akışı yok (GET/DELETE → 405), bildirimler 202.
- stdio'da istekler eşzamanlı işlenir (uzun bir `load_model` diğerlerini bekletmez). `notifications/cancelled` ilgili isteği iptal eder.
- HTTP `Origin` başlığı: yoksa (masaüstü istemciler) geçer, varsa yalnız Control API'nin CORS listesindeki origin'ler; aksi 403.

**Neden `rmcp` değil:** resmî Rust SDK (`rmcp` 3.5, iki dönemi de destekliyor) makrolar ve `schemars` ile gelir ve 2026-07-28 desteği çok yeni (bir ayda 3.2 → 3.5). Harness'ın **sunucu** tarafına yalnız araçlar gerekiyor; kaynak, istem, örnekleme ya da abonelik yok. HTTP tarafında kimlik ve kapsam zaten API'nin koruyucusunda. Protokol yüzeyi küçük: birkaç yüz satır, yeni bağımlılık yok, testle sabitlenmiş. İleride kaynak/abonelik gerekirse `rmcp`'ye geçmek yalnız `harness-mcp`'yi değiştirir.

## Harness'ın MCP istemcisi

Bu sayfanın geri kalanı Harness'ın **sunucu** tarafıdır. Harness ayrıca **istemcidir** (`harness_core::tools::mcp_client`): `mcp_servers.json`'daki sunucuların araçlarını kendi araç motoruna katar. Aynı el yazımı çift dönemli yaklaşım:

- **Müzakere:** stdio'da `server/discover` yoklaması, 5 sn içinde modern yanıt gelmezse (hata ya da sessizlik) `initialize` (2025-11-25 teklif edilir; sunucunun yanıtladığı bilinen sürüm kabul edilir). HTTP'de ilk istek modern (`MCP-Protocol-Version: 2026-07-28`, `Mcp-Method`, `Mcp-Name`, `x-mcp-header` → `Mcp-Param-*`); tanınan modern hata (`-32022`, `-32020`) dışındaki 4xx eski sunucu sayılır ve `initialize` ile devam edilir. Ortak sürümü olmayan modern sunucu hata verir, geri düşülmez. Dönem bağlantı boyunca sabit.
- **Bildirimler:** modern dönemde `subscriptions/listen` (`toolsListChanged`) uzun ömürlü isteği, eski dönemde stdio boru hattı ya da HTTP GET akışı. Liste değişince `tools/list` yeniden okunur ve kayıt güncellenir.
- **İptal ve zaman aşımı:** stdio ve eski HTTP'de `notifications/cancelled` (`requestId`, `reason`), modern HTTP'de yanıt akışını kapatmak; `initialize` hiç iptal edilmez.
- **Neden `rmcp` değil (istemci için de):** gereken yüzey küçük (araçlar, liste değişimi, iptal, kaynaklar, eski HTTP+SSE ve OAuth; hepsi mevcut `reqwest`, `sha2` ve `hf_auth`'un PKCE/yönlendirme koduyla yazıldı, `rmcp` zaten bağımlılık değil); Windows süreç ağacı ve konsol penceresi yönetimi Harness'ın kendi yardımcılarıyla yapılmalı; `rmcp` makrolar ve `schemars` getirir ve 2026-07-28 desteği çok yeni (3.2 → 3.5 bir ayda). Sunucu tarafıyla aynı gerekçe, aynı sonuç: yeni bağımlılık yok (`command-group`'un `with-tokio` özelliği dışında), testle sabitlenmiş birkaç yüz satır.
- **Desteklenmeyenler:** `roots` / `sampling` / `elicitation`, istemler (prompts), dinamik kayıt sunmayan yetkilendirme sunucuları için önceden kayıtlı OAuth istemci kimliği (aşağıya bakın).

### Eski HTTP+SSE taşıyıcısı (2024-11-05)

URL'li bir sunucuda `transport` alanı (`mcp_servers.json`, Bağlantılar > MCP sunucuları formunda "Aktarım") taşıyıcıyı seçer:

| Değer | Davranış |
|---|---|
| yok / `auto` | Önce Akışlı HTTP. Sunucu ilk isteği (POST) **4xx** ile reddederse (401, 403, 407, 429 hariç) MCP'nin geriye uyumluluk kuralı gereği aynı URL'ye `GET` akışı açılır. |
| `http` | Yalnız Akışlı HTTP. |
| `sse` | Yalnız eski HTTP+SSE; Akışlı HTTP hiç denenmez. |

Eski taşıyıcıda istemci `GET <url>` ile `text/event-stream` açar; ilk olay `endpoint` olmalıdır (veri: yol ya da URL, SSE adresine göre çözülür) ve sonraki her JSON-RPC iletisi o adrese `POST` edilir; yanıtlar ve bildirimler akışta `message` olayları olarak gelir. **Güvenlik:** `endpoint` başka bir kökene (şema/ana makine/port) işaret ederse bağlantı reddedilir; iletiler ve token oraya gitmesin. Akış kapanırsa bağlantı "bozuk" sayılır ve ilk çağrıda yeniden kurulur. Durum tablosunda ve `harness tools mcp` çıktısında taşıyıcı `sse` olarak görünür (`auto` için bulunan gösterilir). Diğer istemcilerin `"type": "sse"` girişleri artık İçe Aktarmada `transport: "sse"` olarak alınır.

### OAuth yetkilendirmesi (MCP authorization, 2025-06-18)

401 yanıtı veren uzak sunucu için:

1. `WWW-Authenticate: Bearer resource_metadata="…"` okunur; korunan kaynak metaverisi (RFC 9728, yoksa `/.well-known/oauth-protected-resource[/yol]`) ve yetkilendirme sunucusu metaverisi (RFC 8414, sonra OpenID Connect `openid-configuration`; yol varyantlarıyla) bulunur. Kaynak metaverisi hiç yoksa MCP sunucusunun kendi kökeni yetkilendirme sunucusu sayılır (2025-03-26 davranışı). Sunucu PKCE yöntemlerini bildirip `S256`'yı saymıyorsa reddedilir.
2. **Dinamik istemci kaydı** (RFC 7591, kamusal yerel istemci, `token_endpoint_auth_method: none`). Kayıt uç noktası yoksa yetkilendirme yapılamaz (önceden kayıtlı istemci kimliği henüz desteklenmiyor).
3. **Authorization Code + PKCE (S256)**, sistem tarayıcısı ve `127.0.0.1:<rastgele port>/callback` yönlendirmesi (`hf_auth`'taki PKCE, `state` ve dinleyici kodu ortak). Yetkilendirme ve token isteklerinde `resource` parametresi (RFC 8707) MCP uç noktasının kanonik adresidir; `scope` 401'deki `scope`, yoksa kaynak metaverisinin `scopes_supported` listesidir. En fazla 5 dakika beklenir; vazgeçilebilir.
4. **Saklama:** oturum (erişim ve yenileme tokenı, istemci kimliği, uç noktalar, bitiş zamanı) işletim sistemi anahtarlığında tek kayıt: `mcp-oauth:<sunucu adı>`. JSON dosyalarına ya da günlüğe yazılmaz; `Debug` çıktısı sır göstermez. Sunucu yeniden adlandırılırsa oturum taşınır, silinirse silinir; sunucunun URL'si değişirse eski token gönderilmez.
5. **Yenileme:** erişim tokenı bitmeden 60 sn önce ya da sunucu 401 verdiğinde bir kez `refresh_token` ile yenilenir (aynı anda tek yenileme); yenilenemezse sunucu "Yetkilendirme Gerekli" olur. Kullanıcının kendi `Authorization` başlığı varsa o önceliklidir ve OAuth devreye girmez.
6. **Çıkış:** oturum yerelde silinir (token sunucuda iptal edilmez, iptal uç noktası kullanılmıyor).

Arayüz: Bağlantılar > MCP sunucuları > "Harness MCP İstemcisi"nde 401 veren sunucuda "Yetkilendirme Gerekli" rozeti ve **Yetkilendir** düğmesi; yetkilendirilmişse "Yetkilendirildi" rozeti ve **Oturumu Kapat**. CLI: `harness tools mcp-authorize <sunucu>` (tarayıcı `harness`'ın çalıştığı makinede açılır; CLI'nin API istemcisi 3 dakikada zaman aşımına uğrar, onay bundan uzun sürerse uygulamadaki düğmeyi kullanın), `harness tools mcp-logout <sunucu>`. Control API: `POST /tools/mcp/{server}/authorize` ve `/logout` (`admin`).

### Kaynaklar (resources)

`resources` yeteneğini bildiren bir sunucudan `resources/list` (tüm sayfalar), `resources/templates/list` ve `resources/read` kullanılır (`McpConnection::list_resources` / `read_resource`). Model kaynakları kendiliğinden görmez; kullanıcı ekler:

- **Arayüz:** Bağlantılar > MCP sunucuları > "Harness MCP İstemcisi" bağlı sunucunun altında **Kaynaklar** (liste ve şablonlar). Playground'da yazma alanındaki **MCP Kaynağı Ekle** düğmesi (yapılandırılmış MCP sunucusu varsa görünür) kaynakları listeler; **Sohbete Ekle** kaynağı okuyup `<veri klasörü>/mcp-resources/<sunucu>/` altına, belge okuyucunun tanıdığı bir uzantıyla (MIME türünden: `.md`, `.txt`, `.json`, `.pdf`…) kaydeder ve sohbete diğer belgeler gibi (yerel RAG) ekler. Şablon kaynakları için URI elle girilir. İkili (görsel vb.) kaynaklar belge olarak eklenemez; PDF eklenebilir.
- **CLI:** `harness tools mcp-resources <sunucu>` listeler, `--read <uri>` içeriği yazdırır (metin olduğu gibi, ikili için özet).
- **Control API:** `GET /tools/mcp/{server}/resources`, `GET /tools/mcp/{server}/resource?uri=…` (`tools` kapsamı).
- **İzin sınıfı:** `annotations.readOnlyHint: true` → kısıtlı, aksi → tehlikeli; kullanıcı geçersiz kılar (bkz. [SECURITY_MODEL.md](SECURITY_MODEL.md)).
- **İçe Aktarma kaynakları:** Claude Desktop, Claude Code (`~/.claude.json` ve proje `.mcp.json`), Codex (`~/.codex/config.toml`), Cursor, LM Studio.

## Harness'ın yerleşik araçları (opt-in)

Model yönetimi araçlarının yanında Harness'ın kendi araç motoru da MCP istemcilerine sunulabilir: `read_file`, `file_glob_search`, `grep_search`, `get_info`, `write_file`, `edit_file`, `exec_shell_command`, `current_datetime`, `calculator`, `fetch_url`, `web_search`, `geocode`, `weather` (ad, şema ve sonuç metni Control API `GET /tools` ile aynı). Kaynak: `crates/harness-mcp/src/harness_tools.rs`.

- **Varsayılan kapalı.** Ayarlar > Araçlar > "MCP İstemcilerine Aç" (`settings.tools.mcp_expose`). Kapalıyken `tools/list` yalnız aşağıdaki yönetim araçlarını gösterir; eski istemcilere bir anda dosya ve kabuk araçları görünmez. Ayar Control API'den `PUT /tools/policy` (admin) ile de değişir.
- **Kapsam `tools`.** `read`'i içermez; HTTP'de token `read` + `tools` taşımalı, stdio'da varsayılan `--scope read,servers:write,tools` zaten içerir (`--scope read` gibi daraltılırsa araçlar gizlenir).
- **Hangi araçlar.** Yalnız yerleşik kaynaklı araçlar. Harness'ın kendi MCP istemcisiyle tükettiği sunucuların araçları (`<sunucu>_<araç>`) yeniden sunulmaz: Harness kendi `harness mcp`'sini tüketirse sonsuz yansıma olurdu, bir istemci o sunucuya doğrudan bağlanabilir ve iki katmanlı ad/izin karışıklığı çıkardı. Yönetim aracıyla aynı adlı bir Harness aracı olursa yönetim aracı kalır.
- **Aynı politika, onay kanalı yok.** Çağrı `POST /tools/{ad}/run` üzerinden kayıtlı politikayla karara bağlanır. MCP kullanıcıya soramaz, bu yüzden yalnız politikanın sormadan çalıştırdığı çağrılar çalışır: güvenli araçlar her zaman (politika kapalı değilse), diğerleri "güvenilir araçlara izin" + güvenilir listede ya da "hepsine izin" iken. Sormadan çalışmayacak bir araç listede yine görünür ama açıklamasında bunu söyler; çağrılırsa `isError: true` ve "Ayarlar > Araçlar'da güvenilir yapın" diyen bir metin (`tools.approval_required`) döner. Politika "kapalı" ise hiçbir Harness aracı sunulmaz (`tools.disabled`). İstemci `approved` gönderemez ve çalışma klasörünü seçemez; dosya ve kabuk araçları Ayarlar > Araçlar'daki varsayılan klasörde çalışır (yoksa `no_workspace` araç hatası).
- **İpuçları sınıftan.** Güvenli ve kısıtlı araçlar `readOnlyHint: true`, tehlikeli araçlar (`write_file`, `edit_file`, `exec_shell_command`) `readOnlyHint: false` + `destructiveHint: true`; çalışma klasörü kullanmayan kısıtlı araçlar (ağ: `fetch_url`, `web_search`, `geocode`, `weather`) `openWorldHint: true`.
- **Günlük.** Her çağrı (reddedilenler dahil) `tool_calls`'a yazılır; `client` = `mcp:<istemci adı>` (stdio: `initialize`'daki `clientInfo.name`; HTTP'de oturum olmadığı için token adı, ya da modern istekte `_meta`'daki `io.modelcontextprotocol/clientInfo`). Analitik > İstek inceleyici > Araç çağrıları'nda araç ve istemci süzgeciyle görünür.
- Hata sözleşmesi ve sonuç: çalışan araç `content` metnini döner, araç başarısızsa (`error` kodu) `isError: true`; sonuç 16 000 karakterde kesilir.

## Araçlar ve kapsamlar

`tools/list` yalnız çağıranın kapsamının izin verdiği araçları gösterir. Gizli bir araç çağrılırsa gereken kapsamı adıyla belirten bir araç hatası (`api.insufficient_scope`) döner. Her araçta MCP ipuçları (`readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint`) ve `title` vardır. Model argümanı bir tanım alias'ı, kütüphane model kimliği/dosya adı/adı ya da mutlak `.gguf` yolu olabilir.

| Araç | Kapsam | İpucu | Ne yapar |
|---|---|---|---|
| `list_models` | read | salt okuma | Kütüphane (tür, mimari, kuant, boyut, parametre, bağlam, yetenekler), tanımlar (alias), yüklü olduğu sunucular, süren indirmeler. `kind` süzgeci |
| `get_model_info` | read | salt okuma | GGUF üst verisi, tanımlar, bellek tahmini (`context_size` için VRAM/RAM, sınıf, önerilen `-ngl`; ölçülen verilerle düzeltilmiş) |
| `hardware_status` | read | salt okuma | CPU, RAM, GPU'lar (`--list-devices` toplam/boş VRAM), önerilen backend, runtime, çalışan sunucu sayısı |
| `recommend_models` | read | salt okuma | Yerel modelleri bu makinenin belleğine sığma sınıfına göre sıralar (tam GPU > sıkışık > hibrit > CPU ağırlıklı > OOM riski; sınıf içinde büyük model önce). `purpose`: chat, tools, reasoning, vision, embedding, rerank |
| `load_model` | servers:control | yazma | Tanım yoksa dosya adından oluşturur, aynı modeli son sunmuş durmuş örneği yeniden kullanır, hazır olana kadar bekler. Başarısız başlatmada sınıflandırılmış hata (`servers.out_of_memory`...), log sonu ve OOM kurtarma seçenekleri |
| `unload_model` | servers:control | yazma, idempotent | Modeli sunan sunucuları (ya da bir sunucuyu) durdurur |
| `list_servers` | read | salt okuma | Sunucular: durum, sunulan model kimliği, uç nokta, profil, son hata |
| `start_server` | servers:control | yazma | Kayıtlı profili (ya da bilinen örneği) başlatıp bekler. Ağ kuralları geçerli |
| `stop_server` | servers:control | yazma, idempotent | Sunucuyu durdurur |
| `get_endpoint` | read | salt okuma | OpenAI uyumlu `base_url` (`.../v1`), istekte kullanılacak `model`, API anahtarı gerekip gerekmediği. Anahtarın kendisi asla dönmez |
| `get_metrics` | read | salt okuma | Canlı telemetri (t/s, slotlar, bağlam, KV, RAM/VRAM); `history` ile 1 Hz geçmiş |
| `get_logs` | read | salt okuma | Son log satırları; `since` ile devam |
| `run_benchmark` | servers:write | yazma | llama-bench taraması (`n_prompt`, `n_gen`, `gpu_layers`, `flash_attn`...) ve bitince sonuç |
| `compare_performance` | read | salt okuma | Kayıtlı benchmark'larda model başına en iyi pp/tg t/s + çalışan sunucuların canlı ortalaması |
| `list_conversations` | conversations | salt okuma | Playground sohbetleri (`query`, `limit`); başlık ve içerikte arar |
| `get_conversation` | conversations | salt okuma | Sohbetin geçerli yolu; `last` yalnız son N mesajı döndürür (`messages_omitted`) |
| `export_conversation` | conversations | salt okuma | `format`: `markdown` (varsayılan) ya da içe aktarılabilir `json` belgesi |
| `compact_conversation` | conversations | yazma | Eski turları özetle değiştirir (`keep_turns`, `instruction`, `model`...); gateway çalışıyor olmalı. Özgün mesajlar silinmez |
| `pull_model` | downloads:write | yazma, açık dünya | Hugging Face'ten indirme (kuyruk); `wait` ile bitişi bekler |
| `delete_model` | models:write | **yıkıcı** | Model dosyalarını siler (kullanımdaysa, bağlıysa ya da kütüphane dışındaysa reddedilir) |

`conversations` kapsamı sohbetlerin tamamını (özel içerik) açar; bu yüzden `read`'e dahil değildir ve varsayılan `--scope`'ta yoktur (`--scope read,conversations`). `servers:write` `servers:control`'ü, `models:write` `downloads:write`'ı kapsar; bu yüzden eski `--scope read,servers:write` ve `read,servers:write,models:write` ayarları aynı araçları açar.

LAN'a açma MCP'den yapılamaz: hiçbir araç bağlanma adresini değiştirmez ya da "Yine de Aç" onayı vermez (`/exposure-ack` admin ister ve MCP'de yok). Ağa bağlanan bir profili `start_server` ile başlatmak aynı ağ kuralına takılır (`network.lan_without_key`).

### `get_endpoint` ve yönetilen uç nokta

Gateway çalışıyorsa ve istenen model bir Model Tanımı'na çözülüyorsa `get_endpoint` yönetilen uç noktayı döner:

```json
{
  "kind": "managed",
  "base_url": "http://127.0.0.1:11435/v1",
  "chat_completions_url": ".../v1/chat/completions",
  "responses_url": ".../v1/responses",
  "messages_url": ".../v1/messages",
  "model": "<alias>",
  "loaded": false,
  "api_key_required": false,
  "direct": null,
  "note": "..."
}
```

- Model yüklü değilse de döner (`loaded: false`); gateway ilk istekte yükler (JIT) ve boşta süresi dolunca boşaltır. Yüklüyse `direct` alanı llama-server'ın kendi uç noktasını taşır.
- Gateway loopback'e bağlıysa ve istek uzak bir Harness'e gidiyorsa yönetilen uç nokta verilmez (ajan ona ulaşamaz); doğrudan uç nokta döner.
- `api_key_required: true` ise `inference` kapsamlı isimli bir token gerekir; token'ın kendisi hiçbir zaman dönmez.
- Gateway kapalıysa eskisi gibi `kind: "direct"` (llama-server, sunucu boşaltılınca kapanır) ve model yüklü değilse `mcp.not_loaded`.

Test: `tests/mcp.rs` `e2e_mcp_get_endpoint_managed_via_gateway` (`#[ignore]`, portlar 19200+): `loaded: false` → dönen adrese gerçek sohbet (JIT) → `loaded: true`.

## "Harness MCP'yi Ekle"

Bağlantılar > Araçlar ve İstemciler. Çekirdek: `harness_core::integrations::harness_mcp`.

| Araç | Dosya | Kayıt | Kaynak |
|---|---|---|---|
| Claude Code | `~/.claude.json` (`CLAUDE_CONFIG_DIR` varsa `$CLAUDE_CONFIG_DIR/.claude.json`), kullanıcı kapsamı | `mcpServers.harness`: `{"type": "stdio", "command", "args", "env"}` ya da `{"type": "http", "url", "headers"}` | code.claude.com/docs/en/mcp |
| Codex | `~/.codex/config.toml` (`CODEX_HOME`) | `[mcp_servers.harness]`: `command`, `args`, `env` ya da `url`, `http_headers` | learn.chatgpt.com/docs/extend/mcp |
| OpenCode | `~/.config/opencode/opencode.json(c)` | `mcp.harness`: `{"type": "local", "command": [...], "environment", "enabled"}` ya da `{"type": "remote", "url", "headers", "oauth": false}` | opencode.ai/docs/mcp-servers |
| Cursor | `~/.cursor/mcp.json` | `mcpServers.harness`: `{"type": "stdio", ...}` ya da `{"url", "headers"}` | cursor.com/docs/context/mcp |

- Önizleme: yazılacak dosyanın önü/sonu. Büyük dosyalarda (`~/.claude.json` binlerce satır olabilir) yalnız eklenecek kayıt gösterilir.
- Sağlayıcı entegrasyonlarıyla aynı yazma yolu (`keyed`): yalnız `harness` kaydı yerinde düzenlenir, JSONC/TOML yorumları, girinti ve anahtar sırası korunur; her yazmadan önce `<dosya>.harness-<tarih>.bak` yedeği (dosya başına son 5), atomik yazma. Ayrıştırılamayan dosyaya dokunulmaz.
- Defter (`config/integrations.json`, `harness-mcp:<araç>`): "Kaldır" kaydı siler, Harness'ın yarattığı üst nesneleri de kaldırır ve Harness'ın yarattığı dosyayı geri siler; dosya Harness yazdıktan sonra değişmediyse eski haline bayt bayt döner. Defteri olmayan eski kayıtlar `harness` adıyla bulunup silinir. Diğer MCP sunucuları ve ayarlar korunur. Kaldırmadan sonra Harness'ın yedeklerindeki isimli token'lar `removed-by-harness` ile değiştirilir.
- stdio komutu: uygulamayla birlikte gelen `harness` (sidecar; bkz. [CLI.md](CLI.md#kurulum-ve-path)), sonra PATH'teki `harness` (`cli_install::locate`); bulunamazsa (ör. geliştirme derlemesinde `cargo build -p harness-cli` yapılmadıysa) kullanıcı yolu girer. "İndirme ve model silmeye izin ver" `--scope read,servers:write,tools,models:write` ekler. GUI'nin veri dizini CLI varsayılanı değilse `HARNESS_DATA_DIR` ortam değişkeni yazılır.
- HTTP: Control API adresi + isimli token (Ayarlar > Geliştirici > Control API). Token aracın dosyasına düz metin olarak yazılır; stdio bu yüzden önerilir.
- Claude Code ve Codex için elle eklenecek tek satırlık komut da gösterilir (`claude mcp add -s user harness -- <harness> mcp`, `codex mcp add harness -- <harness> mcp`).

## Testler

- Birim: `cargo test -p harness-mcp` (araç kataloğu, argüman doğrulama, HTTP başlık kuralları, base64), `cargo test -p harness-core harness_mcp` (dört aracın yapılandırma biçimleri, yorum/diğer kayıtların korunması, bozuk dosya, kaldırma).
- Harness araçları: `crates/harness-api/tests/tools_mcp.rs` (gerçek HTTP: `tools` kapsamı, `approved` / klasör reddi, kapalı → yalnız yönetim araçları, açık → araçlar ve işaretler, onay isteyen çağrının hatası, politika kapalı, `mcp:<istemci>` günlüğü), `crates/harness-cli/tests/mcp.rs` `stdio_offers_harness_tools_only_when_opted_in` (gerçek `harness mcp` ikilisi, `initialize` istemci adı, `tool_calls` kaydı).
- Gerçek HTTP (`crates/harness-api/tests/mcp.rs`): 401, eski el sıkışma, 202 bildirimi, oturum başlığı yok, kapsamla süzülen `tools/list`, kapsam reddi, modern başlık kuralları (`-32020`, `-32022`, `-32602`, 404), GET 405, yabancı Origin 403.
- Gerçek stdio (`crates/harness-cli/tests/mcp.rs`): gerçek `harness mcp` ikilisi, satır satır JSON-RPC istemcisi, geçici veri dizini.
- Uçtan uca (`#[ignore]`, `HS_LLAMA_SERVER` + `HS_TEST_MODEL`, portlar 18900+): stdio `harness serve` üzerinden `load_model` → `get_endpoint` → dönen uç noktaya gerçek `/v1/chat/completions` → metrik, log, bilgi, öneri → `unload_model` → benchmark → karşılaştırma. `--headless` stdin kapanınca llama-server'ı kapatır. HTTP'de aynı akış.
- Gerçek istemciler (`#[ignore]`): `e2e_claude_code_connects_to_harness_mcp`, entegrasyonun geçici `CLAUDE_CONFIG_DIR`'a yazdığı kaydı `claude mcp list` ile hem stdio hem HTTP için "Connected" görür. `e2e_codex_reads_harness_mcp_entry`, geçici `CODEX_HOME`'daki kaydı `codex mcp get --json` ile okur.


### OpenCode gerçek istemci doğrulaması

OpenCode **1.18.34**, yalnız ayrı bir klasöre `npm install --prefix <klasör> opencode-ai` ile kuruldu. [OpenCode yapılandırma belgesi](https://opencode.ai/docs/config/) uyarınca `OPENCODE_CONFIG` global dosyayı değiştirmez, onunla birleşir; tek başına izolasyon sağlamaz. XDG config/data/cache/state kökleri ve `OPENCODE_TEST_HOME` ayrık profile yönlendirildi, proje yapılandırması ve otomatik güncelleme kapatıldı. `debug paths` ve `debug config --print-logs` gerçek kullanıcı dosyasının yerine ayrık yolların kullanıldığını doğruladı.

`harness_mcp::apply_in`, ayrık profile `mcp.harness` kaydını yazdı: `type: local`, `command: [harness.exe, mcp, --headless, --scope, read]`, `environment.HARNESS_DATA_DIR` ayrık veri klasörü, bellek içi secrets. `opencode mcp list` → **✓ harness connected / 1 server(s)**. OpenCode [1.18.34 bağlantı kodu](https://github.com/anomalyco/opencode/blob/v1.18.34/packages/opencode/src/mcp/index.ts) bu durumdan önce MCP araç kataloğunu da alır; katalog alınamazsa bağlantı başarısız sayılır.

Kurulu `http://127.0.0.1:11435/v1` gateway'ine, zaten hazır `qwen2.5-coder-14b-instruct-q4_k_m` modeliyle bir `opencode run --format json` oturumu gönderildi. Yalnız `harness_list_models` aracı açık ve izinliydi; MCP kapsamı `read`. Model çağrıyı kod bloğunda JSON metni olarak yazdı; `tool_use` olayı çıkmadı ve araç yürütülmedi. Bağlantı/katalog doğrulandı, model üzerinden araç çağrısı doğrulanmadı. Kurulu uygulamanın verileri kullanılmadı; geçici headless MCP örneği model sunucusu başlatmadı ve stdin kapanınca kapandı. Cursor test makinesinde yoktu, denenmedi.

Tekrar çalıştırma (PowerShell; `HS_OPENCODE_MODEL_TURN` verilmezse yalnız bağlantı testi):

```powershell
$env:HS_OPENCODE_BIN = (Resolve-Path <opencode-klasörü>/node_modules/opencode-ai/bin/opencode.exe).Path
$env:HS_OPENCODE_MODEL_TURN = '1'
$env:HS_OPENCODE_REPORT_DIR = (Resolve-Path <rapor-klasörü>).Path
cargo test -p harness-cli --test mcp e2e_opencode_connects_to_harness_mcp -- --ignored --nocapture
```

Test `McpRoots` ile geçici profil kullanır; XDG yollarını çalıştırmadan önce `debug paths` çıktısında denetler. Kullanıcının gerçek OpenCode/Claude/Codex dosyalarına yazmaz.
