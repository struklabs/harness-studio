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
| `hardware_status` | read | salt okuma | CPU, RAM, GPU'lar (`--list-devices` toplam/boş VRAM), önerilen backend, model motoru, çalışan sunucu sayısı |
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
