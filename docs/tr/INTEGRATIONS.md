# Entegrasyonlar ve `harness launch`

[English](../en/INTEGRATIONS.md)

## 0. Windows başlatma, Zed ve VS Code notları

- GUI yolu: `LaunchDialog.svelte` → `launchTool` IPC → `launch_integration_tool` → `LaunchInput/LaunchPlan`. OpenCode npm kurulumu `opencode.cmd` → `opencode.exe`; argüman yok, ortam `OPENCODE_CONFIG_CONTENT` + `HARNESS_API_KEY`. Harness CLI bu GUI zincirinin parçası değildir. CLI `spawn_and_wait` istemciyi aynı konsolda çalıştırıp bekler; GUI bekleme iş parçacığı yalnızca temizlik yapar.
- Windows konsol yönetimi: yeni bir konsol penceresi açmak istemcinin giriş/çıkış/hata akışlarını kendiliğinden konsola bağlamaz (doğrulandı: eski `CREATE_NEW_CONSOLE` + yönlendirilmiş ebeveyn tanıtıcıları testi giriş/çıkış/hata için `True/True/True` verdi). Özel `launch.ps1`, `CreateFile(CONIN$/CONOUT$)`, `SetHandleInformation`, `SetStdHandle` ile akışları bağlar. `.NET ProcessStartInfo` yönlendirme olmadan istemciyi başlatır (PowerShell'in normal `&` çağrısı çıkış boruları oluşturabildiğinden kullanılmaz). `.cmd/.bat` için `cmd /d /s /c` kullanılır; `start`, `DETACHED_PROCESS` ve `wt.exe` devir zinciri yoktur. Yeni konsolun barındırıcısını Windows seçer. Plan/token özel dosyadadır; süreç argümanlarında değildir.
- `windows_launcher_reconnects_all_tools_to_console_and_keeps_plan`: beş aracın gerçek plan ortamları, Unicode/boşluk/apostrof/metakarakterli argümanlar, çalışma dizini, kaldırılan ortam ve dosya temizliği; native ve npm batch benzetimi altında akışlar `False/False/False`. Batch argümanında çift tırnak/kontrol karakteri güvenli biçimde reddedilir.
- Zed'in [resmi anahtar belgesi](https://zed.dev/docs/ai/use-api-access#api-keys-and-environment-variables), anahtarların settings.json yerine anahtar deposunda tutulduğunu; OpenAI-compatible kimliğinin upper snake case + `_API_KEY` ortam değişkenini kullandığını belirtir. [Kaynak kodu](https://github.com/zed-industries/zed/blob/main/crates/language_models/src/provider/open_ai_compatible.rs) anahtar yoksa `NoApiKey` döndürür. `Harness Studio` → `HARNESS_STUDIO_API_KEY`. Satırdan anahtarı kopyalayıp Zed LLM Providers → Harness Studio alanına yapıştırmak gerekir; anahtarsız gateway için `harness-local` yeterlidir. Kalıcı ortam değiştirilmez.
- VS Code'un [güncel resmi biçimi](https://code.visualstudio.com/docs/agent-customization/language-models#add-a-custom-endpoint-model) kök dizi, `vendor: customendpoint`, `apiType: chat-completions`, `models[].url`, yetenek/token sınırları ve gerektiğinde `apiKey: ${input:...}` kullanır. Mevcut yazıcı uyumludur; `github.copilot.chat.customOAIModels` yazılmaz. [1.122 sürüm notu](https://code.visualstudio.com/updates/v1_122#_custom-endpoint-provider-in-stable) Stable desteğini doğrular; eski OpenAI Compatible yolu Insiders'taydı. `[]` için Algılandı yalnız kurulum tespitidir, Yapılandır henüz yapılmamıştır. İzole bir test yapılandırma/güncelleme/geri almayı doğrular.
- Gerçek kullanıcı ayarları ve kalıcı ortam değişkenleri bu testlerle hiçbir zaman değiştirilmez.

Bağlantılar > Araçlar ve İstemciler ekranı, `harness launch` ve kalıcı entegrasyonların
başvurusu. Biçimler aşağıda bağlantısı verilen resmi belgelere göre yazıldı (son
kontrol Eylül 2026). "Doğrulanmadı" diye işaretli satırlar belgeden okundu ama gerçek istemciyle
denenmedi.

Kod: `crates/harness-core/src/integrations/` (`keyed.rs` düzenleme ve geri alma motoru,
`launch.rs` başlatma planları, `conntest.rs` bağlantı testi, `mod.rs` kayıt defteri),
`crates/harness-cli/src/launch.rs`, `src/lib/components/connections/`.

## 1. Uç nokta, model adı ve anahtar

- Gateway açıksa her şey **managed endpoint**'e (`http://127.0.0.1:11435/v1`) yazılır ve model
  adı Model Tanımı'nın alias'ıdır. Model yüklü değilse gateway ilk istekte yükler (JIT).
- Gateway kapalıysa seçili sunucunun doğrudan adresi ve sunucunun bildirdiği model adı kullanılır.
- **Port değişince:** Her uygulama (`apply`) isteği anahtarı olmadan ledger'a yazılır
  (`LedgerEntry.request`, `StoredRequest`: adres, model, bağlam, model listesi, anahtar var mıydı).
  Gateway 11435 başka bir programdayken yedek porta geçerse ya da yedekten geri dönerse,
  Harness'ın gateway'in önceki bir portu (`config/ports.json` → `gateway_history`) için yazdığı
  ayarlar başlangıçta yeni adrese yeniden yazılır (`drift::follow_gateway_port`; yalnız adres
  değişir, aracın varsayılan modeline dokunulmaz). Anahtar yazılmış ayarlar ve başka bir loopback
  portuna (ör. bir llama-server'ın kendi portu) yazılmış ayarlar kendiliğinden değişmez:
  Bağlantılar'da "Araç başka bir adrese ayarlı" uyarısı ve **Düzelt** düğmesi çıkar
  (`check_integration_endpoint_drift`, normal önizleme/uygulama akışı).
  Bu kayıttan önce uygulanmış ayarlar (ledger'da istek yok) dosyanın kendisinden değerlendirilir:
  aracın dosyasında Harness'ın yazdığı adres (OpenCode/Kilo/Open Design `provider.harness.options.baseURL`,
  Claude Code `env.ANTHROPIC_BASE_URL`, Codex `model_providers.harness.base_url`, Continue `apiBase`,
  VS Code, Zed, Goose, LibreChat) gateway'in geçmiş portlarından biriyse (`gateway_history` ve
  varsayılan 11435) ve şimdiki porttan farklıysa kart aynı uyarıyı gösterir. Anahtar yoksa yalnız
  adresin portu yerinde değiştirilir (`keyed::retarget`: yedek alınır, yorumlar ve diğer ayarlar
  korunur, ledger karmaları güncellenir; Geri al çalışmaya devam eder); kullanıcının başka bir
  adresine ya da başka bir porta dokunulmaz, anahtar varsa dosyaya dokunulmadan uyarı + Düzelt
  kalır. Kontrol her gateway başlangıcında bir kez çalışır; gateway varsayılan porta dönerse
  (11435 yeniden boşsa yapılandırılmış port önce denenir) 11436'ya yazılmış ayarlar geri taşınır.
  Bağlantılar/Gateway'deki yedek port bildirimi kaç istemci ayarının gerçekten taşındığını ve
  hangilerinin Düzelt beklediğini yazar (`GatewayStatus.clients_moved/clients_pending`).
- Anahtar:
  - Loopback gateway varsayılan olarak token istemez; araçlara yer tutucu `harness-local` verilir.
  - `harness launch` ve GUI'deki "Başlat", gateway istemci token'ını (`<veri>/gateway-token`,
    her başlatmada değişir) yalnız alt sürecin ortamına verir. Uzak örnekte `HARNESS_TOKEN`
    (`inference` kapsamlı isimli token) kullanılır.
  - Doğrudan uç noktada llama-server anahtarı (`--api-key` / `HARNESS_SERVER_API_KEY`) kullanılır.
- Anthropic tarzı istemciler (Claude Code, Claude Desktop, Anthropic SDK, `langchain_anthropic`)
  taban adresi **`/v1` olmadan** ister; kendileri `/v1/messages` ekler. `@ai-sdk/anthropic`
  ise `/v1` ile ister. OpenAI tarzı her şey `/v1` ile.

## 2. `harness launch`

```
harness launch claude|codex|opencode|aider|goose [--model <alias|ad|dosya.gguf>]
               [--prepare | --no-prepare] [--direct] [--dry-run] [-- <aracın argümanları>]
```

Akış: model seçimi (TTY'de liste, yoksa `--model` ya da çalışan tek model) → bağlam kontrolü →
gerekirse "Sunucuyu Hazırla" → uç nokta (gateway varsa JIT ısıtma isteği, yoksa `/load`) →
araç, yalnız süreç ortamı ve geçici dosyalarla başlar → çıkınca geçici klasör silinir. Çıkış
kodu aracınkidir. Hiçbir global yapılandırma yazılmaz.

| Araç | Asgari / önerilen bağlam | Nasıl verilir |
|---|---|---|
| Claude Code | 32.768 / 65.536 | Ortam: `ANTHROPIC_BASE_URL` (host, `/v1` yok), `ANTHROPIC_AUTH_TOKEN` (Bearer; oturum açma istemi çıkmaz), `ANTHROPIC_MODEL`, `ANTHROPIC_DEFAULT_{OPUS,SONNET,HAIKU}_MODEL`, `CLAUDE_CODE_SUBAGENT_MODEL`, `CLAUDE_CODE_MAX_CONTEXT_TOKENS`, `CLAUDE_CODE_MAX_OUTPUT_TOKENS`, `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`, `CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS=1`, `ANTHROPIC_CUSTOM_HEADERS: X-Harness-Client: claude-code`. `ANTHROPIC_API_KEY` ve Bedrock/Vertex/Foundry değişkenleri kaldırılır. Aynı değerler geçici bir dosyaya da yazılıp `--settings <dosya>` ile verilir: `~/.claude/settings.json` içindeki `env` süreç ortamını ezer, `--settings` ise onu ezer. |
| Codex | 32.768 / 65.536 | `-c model_provider=harness -c model_providers.harness.{name,base_url,wire_api=responses[,env_key=HARNESS_API_KEY]} -c web_search=disabled [-c model_context_window=N] -m <alias>`. Codex yalnız Responses API konuşur. `web_search` kapatılır: gateway, modelde "sunucu taraflı araçlar" açık değilse barındırılan aracı sessizce düşürmek yerine `400 unsupported_tool_type` ile reddeder (açıksa `web_search` gateway'de yerelde karşılanır; Ayarlar > Gateway). Anahtar yoksa `env_key` verilmez; Codex yerel sağlayıcıda oturum istemez. |
| OpenCode | 49.152 / 65.536 | `OPENCODE_CONFIG_CONTENT` (en son birleşen yapılandırma katmanı): `provider.harness` (`@ai-sdk/openai-compatible`, `baseURL`, `apiKey: {env:HARNESS_API_KEY}`, model `limit.context/output`, `tool_call`) ve `model: harness/<alias>`. Program: PATH'te `opencode`, yoksa masaüstü sürümünün getirdiği `opencode-cli` (§3a). |
| Aider | 8.192 / 32.768 | `--model openai/<alias> --openai-api-base <base> --model-metadata-file <geçici> --no-show-model-warnings`, ortamda `OPENAI_API_KEY`/`OPENAI_API_BASE`; `AIDER_*` model değişkenleri kaldırılır. |
| Goose | 32.768 / 65.536 | Ortam: `GOOSE_PROVIDER=openai`, `GOOSE_MODEL`, `OPENAI_HOST` (host), `OPENAI_BASE_PATH=v1/chat/completions`, `OPENAI_API_KEY`, `GOOSE_CONTEXT_LIMIT`, `OPENAI_CUSTOM_HEADERS`. Argüman verilmezse `goose session`. |

"Sunucuyu Hazırla" (`harness_core::prepare`) tanımın yükleme ayarlarına şunları yazar: modelin
desteklediği ve ekran kartına sığan **en büyük bağlam**, Flash Attention, K/V önbelleği ve 1 slot;
sonra tanımın çalışan örneklerini durdurur, gateway ilk istekte yeni ayarlarla yükler.

Seçim kuralı: (a) model sınırı: GGUF `context_length`; `rope.scaling.type` (yarn/linear) + factor +
`original_context_length` bildiriliyorsa `original × factor`, aksi hâlde eğitim bağlamının üstüne
çıkılmaz; (b) bellek: tam GPU'ya yükleme, Flash Attention, 1 slot, `vram` tahmini (ölçümle
düzeltilmiş) bütçenin en fazla %90'ı; (c) aracın sınırları: asgari, önerilen (Open Design 131.072,
Claude Code/Codex/OpenCode/Goose 65.536, Aider 32.768) ve üst sınır (önerilenin 2 katı; Open Design
262.144). Önce önerilen bağlam ve üstünde `q8_0` KV aranır, sığmazsa `q4_0`; önerilene ulaşılamazsa
asgariye kadar aynı sırayla denenir. Alternatifler: daha küçük bağlamda `q8_0`/`f16` KV, ya da
önerilen bağlamda kısmi CPU'ya taşma (yavaş; yalnız açık onayla). Asgari bağlam bile sığmıyorsa
hiçbir şey sessizce uygulanmaz. Terminalsiz ortamda `--prepare` ya da `--no-prepare` gerekir;
`--prepare` yalnız tam GPU seçimini onaylar.

`--dry-run` hiçbir şey başlatmadan planı yazar; token'lar `***` olarak gizlenir.

GUI'deki "Başlat" aynı planı kullanır ve aracı yeni bir terminal penceresinde açar (proje
klasörü seçilebilir):

- **Windows:** yeni konsol penceresi.
- **Linux:** 0600 izinli, kendini silen bir `/bin/sh` başlatıcı betik; `$TERMINAL` →
  `x-terminal-emulator` → bilinen terminaller (`harness_core::integrations::terminal::pick_terminal`).
- **macOS:** aynı başlatıcı betik, Terminal.app'te (`do script "exec sh '<betik>'"`) ya da
  iTerm2'de (`create window with default profile command`) `osascript` ile açılır.
  iTerm2, yalnızca kuruluysa (`/Applications` ya da `~/Applications`) ve kabuk betiklerinin
  (`public.unix-executable` / `.command`) varsayılan uygulamasıysa seçilir
  (`defaults read com.apple.LaunchServices/com.apple.launchservices.secure LSHandlers`);
  yoksa Terminal.app. iTerm2 penceresi açılamazsa Terminal.app denenir. İlk kullanımda macOS
  "Harness Studio, Terminal'i denetlemek istiyor" (Otomasyon) izni sorar. AppleScript'te yalnız
  betiğin yolu bulunur; token ve ortam değişkenleri betiğin içindedir (argv ve `ps`'te görünmez).
- Terminal açılamazsa (terminal uygulaması yok, `osascript` reddedildi) karşılık gelen
  `harness launch` komutu kopyalanır.

Kaynaklar: code.claude.com/docs/en/llm-gateway-connect, /llm-gateway-protocol, /env-vars,
/model-config, /settings; developers.openai.com/codex/config-reference, /config-advanced,
/auth, /noninteractive; opencode.ai/docs/config, /providers; aider.chat/docs/llms/openai-compat.html,
aider.chat/docs/config/adv-model-settings.html; github.com/aaif-goose/goose `documentation/docs`
(providers, environment variables).

## 3. Kalıcı entegrasyonlar (Harness dosyayı yazar)

Her birinde önce önizleme (renkli fark) gösterilir. Yalnız Harness'in anahtarları değişir,
yorumlar ve diğer ayarlar korunur (JSON/JSONC: `jsonc-parser` CST, TOML: `toml_edit`). YAML'da
yorumlar korunamaz; önizleme bunu uyarır.

| Araç | Dosya | Yazılan |
|---|---|---|
| Claude Code | `~/.claude/settings.json` (`CLAUDE_CONFIG_DIR`) | `env.*`: §2'deki Claude Code değişkenleri. |
| Codex | `~/.codex/config.toml` (`CODEX_HOME`) + `~/.codex/harness.config.toml` | `[model_providers.harness]` (`name`, `base_url`, `wire_api = "responses"`, `http_headers`). Profil ayrı dosyada (Codex 0.134+ profil dosyaları): `model_provider`, `model`, `model_context_window`, `web_search = "disabled"`. Kullanım: `codex --profile harness`. |
| OpenCode | `~/.config/opencode/opencode.json(c)` | `provider.harness`: seçilen model önce, sonra ağ geçidindeki bütün sohbet modelleri (`models.<alias>.limit.context` tanımın bağlamı), böylece OpenCode içinden model değiştirilebilir. Genel `model`: istekte `set_default_model` yoksa yalnız boşsa ya da zaten `harness/*` ise; `true` ise her zaman, `false` ise hiç. GUI'de "OpenCode varsayılan modeli bu olsun" işareti bu alanı yönetir (varsayılan işaret aynı kural). |
| Open Design | Open Design'ın `app-config.json` dosyaları | Önceki sürümle aynı. |
| Continue | `~/.continue/config.yaml` | `models[]` içinde `title: Harness Studio (llama.cpp)`. |
| VS Code Copilot (BYOK) | `%APPDATA%\Code\User\chatLanguageModels.json` (macOS `~/Library/Application Support/Code/User`, Linux `~/.config/Code/User`) | Kök dizide `name: "Harness Studio"`, `vendor: "customendpoint"`, `apiType: "chat-completions"`, model `url` = `<base>/chat/completions`, `toolCalling`, `maxInputTokens`/`maxOutputTokens`. Anahtar varsa `apiKey: "${input:harnessStudioApiKey}"` (VS Code bir kez sorar, gizli depoda tutar). Yol gerçek bir VS Code kurulumunda doğrulandı (§8). |
| Zed | `%APPDATA%\Zed\settings.json` (diğerleri `~/.config/zed/settings.json`) | `language_models.openai_compatible."Harness Studio"`: `api_url`, `available_models[]` (`max_tokens`, `max_output_tokens`, `capabilities`). Anahtarı Zed kendi arayüzünde ister Ortam değişkeni sağlayıcı adından türetilir: `HARNESS_STUDIO_API_KEY`; Harness anahtarı dosyaya yazmaz (Zed belgesi de yazmayın diyor). |
| Kilo Code | `~/.config/kilo/kilo.json(c)` | OpenCode biçiminde `provider.harness` (`npm: @ai-sdk/openai-compatible`, `options.baseURL`/`apiKey`); Kilo belgesi Windows'ta da `C:\Users\<kullanıcı>\.config\kilo\kilo.jsonc` der. Gerçek kurulumda denenmedi. |
| Goose | `custom_providers/harness.json` (Windows `%APPDATA%\Block\goose\config`, diğerleri `~/.config/goose`) | Harness'e ait bütün dosya: `engine: openai`, `base_url` = tam `<base>/chat/completions`, `models[].context_limit`, anahtar varsa `api_key_env: HARNESS_API_KEY`. |
| LibreChat | Kullanıcının verdiği `librechat.yaml` yolu | `endpoints.custom[]` içinde `name: "Harness Studio"` (`apiKey`, `baseURL`, `models.default/fetch`, `titleConvo`, `titleModel`). Dosya yoksa `version: 1.3.17` ile oluşturulur. Docker'da `host.docker.internal` gerekir. |

### 3a. OpenCode masaüstü

Masaüstü uygulaması yalnız kalıcı yapılandırmayı okur (açılışta), `OPENCODE_CONFIG_CONTENT` kullanılamaz (uygulama açıksa yeni süreç yalnız var olan pencereyi öne getirir). Başlat penceresi masaüstü kuruluysa "Yapılandır ve Masaüstünde Aç" sunar: §3'teki önizleme → uygula → uygulama ayrık bir süreç olarak açılır (ortamında Harness ayarı yok). `harness_core::integrations::opencode_desktop`:

| Platform | Konum | Durum |
|---|---|---|
| Windows | `%LOCALAPPDATA%\Programs\@opencode-aidesktop\OpenCode.exe` (Electron, 1.18.x; CLI yok) | Doğrulandı |
| Windows | `%LOCALAPPDATA%\OpenCode\OpenCode.exe` + `opencode-cli.exe` (eski Tauri sürümü, 1.2.x; CLI tam TUI) | Doğrulandı; CLI ile gerçek bir OpenCode turu çalıştı |
| Windows | Kaldırma kayıtları (HKCU/HKLM, 32 bit görünüm dahil), `DisplayName` "OpenCode" ile başlayan: `DisplayIcon` ya da `InstallLocation` + `MainBinaryName` | Doğrulandı |
| Windows | Başlat menüsü `OpenCode.lnk` | Okunmuyor (kaldırma kaydının gösterdiği exe'yi gösteriyor) |
| macOS | `/Applications/OpenCode.app`, `~/Applications/OpenCode.app` (CLI `Contents/MacOS/opencode-cli`) | Varsayım |
| Linux | `/opt/OpenCode/{opencode-desktop,OpenCode}`, `/usr/bin/opencode-desktop`, `~/Applications/OpenCode*.AppImage` | Varsayım |

Uygulamanın çalışıp çalışmadığı süreç listesinden (exe yolu) okunur; çalışıyorsa yeniden başlatılması gerektiği yazılır.

### 3b. Model seçimi, araç çağrısı ve ilk yanıt

`harness_core::integrations::agent_models` her tanım için (embedding/rerank hariç) şunları verir: bağlam, parametre sayısı, araç çağrısı kararı ve istem işleme hızı. Araç çağrısı kararı, en güçlüden başlayarak:

1. **Deneme** ("Araç Çağrısını Dene"): §7'deki araçlı Chat Completions isteği; `tool_calls` döndüyse doğrulandı, döndürmediyse başarısız (yanıt `{"name":…,"arguments":…}`, `<tool_call>`, `<tools>` gibi görünüyorsa "düz metin yazdı"). `config/tool_checks.json`'da model dosyası ve şablon geçersiz kılmasıyla saklanır; tanım başka dosyaya geçince eskir.
2. **Ağ Geçidi kaydı:** bu alias'ın `finish_reason` `tool_calls` / `tool_use` / `function_call` ile biten yanıtları.
3. **GGUF şablonu:** araç işlemeyen şablon → llama-server araç çağrısı döndüremez; işleyen şablon → yalnız "mümkün".

Şablon tek başına yetmez: Qwen2.5-Coder-14B ile Qwen3-4B'nin şablonları aynı `<tool_call>` işaretlerini taşır ve llama-server (b11284) ikisi için de `chat_template_caps.supports_tools = true` bildirir; aynı istekte Qwen3 `tool_calls` döndürür, coder modeli çağrıyı `<tools>{"name": "get_weather", …}</tools>` metni olarak yazar (RX 7900 GRE). Sabit model listesi yoktur. İlk yanıt tahmini: araç istemi (OpenCode ~29.000 token) / kayıttaki istem hızı (önbelleklenmemiş token / `prompt_ms`, en az 2.000 token ölçülmüşse).

Kaynaklar: code.visualstudio.com/docs/agent-customization/language-models;
zed.dev/docs/ai/use-api-access; kilo.ai/docs; goose `custom_providers` belgesi;
librechat.ai/docs/configuration/librechat_yaml/object_structure/custom_endpoint;
docs.continue.dev.

## 4. Adım adım kurulum (Harness dosya yazmaz)

Ekranda adımlar ve kopyalanabilir alanlar (Base URL, anahtar, model, gerekirse bağlam) var.

| Araç | Not |
|---|---|
| Claude Desktop | Developer → Configure Third-Party Inference → Gateway; URL `/v1` olmadan; model adı listeye eklenir (claude.com/docs/third-party/claude-desktop/gateway). |
| Cursor | İstekleri Cursor sunucuları üzerinden gider; `127.0.0.1`'e ulaşamaz. Yalnız internetten erişilebilir bir adresle çalışır, yerel kullanım önerilmez. |
| Cline | API Provider "OpenAI Compatible", Context Window elle (docs.cline.bot). |
| Roo Code | Proje 2026-05-15'te kapandı (docs.roocode.com/sunset); adımlar Cline ile aynı. |
| JetBrains AI Assistant | Settings → Tools → AI Assistant → Providers & API keys (2026.2). |
| Jan | OpenAI uyumlu sağlayıcı, Base URL `/v1` ile; boş anahtar kabul edilmez. |
| Open WebUI | Admin Panel → Settings → Connections ya da `OPENAI_API_BASE_URL`/`OPENAI_API_KEY`; Docker'da `host.docker.internal`. |
| Obsidian Copilot | "Add Custom Model", sağlayıcı "OpenAI Format"; CORS hatasında özel modelde CORS seçeneği. |
| Aider | `harness launch aider` ya da kopyalanan komut. |

## 5. SDK örnekleri

OpenAI (Python/JS, `base_url` `/v1` ile), Anthropic Python (`base_url` `/v1` olmadan,
`auth_token`), LangChain (`ChatOpenAI` `/v1` ile, `ChatAnthropic` `/v1` olmadan), LlamaIndex
(`OpenAILike`, `is_chat_model`, `is_function_calling_model`, `context_window`), Vercel AI SDK
(`@ai-sdk/openai-compatible`, `createOpenAICompatible({ name, baseURL })`) ve cURL. Hepsi o anki
gerçek uç noktayı ve model adını kullanır.

## 6. Geri alma ve yedekler

- **Defter:** `<veri>/config/integrations.json`. Her uygulamada dosya başına: yazılan her
  anahtarın yolu, yazılan değerin SHA-256'sı (değerin kendisi değil), önceki değer, anahtarın
  hangi üst yolu oluşturduğu, işaretçi (Harness'e ait bütün tablo/öğe) bayrağı, dosyanın son
  SHA-256'sı, dosyayı Harness'in oluşturup oluşturmadığı.
- **Geri Al:** yalnız Harness'in yazdığı ve hâlâ Harness'in yazdığı değeri taşıyan anahtarlar
  kaldırılır ya da önceki değerine döner; Harness'in oluşturduğu boş üst tablolar ve dosyalar
  silinir. Kullanıcının sonradan değiştirdiği anahtarlar yerinde kalır ve uyarı olarak
  bildirilir. Defter yoksa (eski sürüm, elle eklenmiş kayıt) yalnız işaretçi girdiler
  (`model_providers.harness`, `provider.harness`, `"Harness Studio"` öğeleri...) kaldırılır.
  Çözümlenemeyen dosyaya dokunulmaz.
- **Yedek:** her yazma ve geri almadan önce `<dosya>.harness-YYYYMMDD-HHMMSS.bak`; dosya başına
  son 5 kuşak tutulur. Ekranda yedek sayısı ve yolları görünür.
- **Sapma uyarısı:** dosyanın SHA-256'sı defterdekinden farklıysa ekranda "Elle değiştirildi"
  rozeti, uygulama ve geri almada uyarı çıkar.

### 6a. Eski model adı (alias sapması)

Bir araç Harness'in yazdığı model adını (tanım alias'ı) saklar. Alias yeniden adlandırılırsa,
tanım silinirse ya da bir profil kendi tanımını alıp sunulan ad `<dosya>` yerine
`<dosya>-<profil>` olursa araç eski adı istemeye devam eder ve "model bulunamadı" alır.
Bağlantılar ekranı bunu kendisi bulur (`integrations::drift`, Tauri `check_integration_alias_drift`):

- Salt okunur: yapılandırılmış her entegrasyonun dosyasından Harness'e verilen model adları
  okunur (OpenCode ve Kilo Code: varsayılan `model` + `provider.harness.models` anahtarları;
  diğerleri: tek model adı) ve tanım alias'ları ile o an sunulan adlarla (çalışan sunucu,
  gateway) büyük/küçük harf duyarsız karşılaştırılır. Tanım da sunulan ad da yoksa hüküm
  verilmez.
- Eksik ad varsa kartta uyarı ve tek bir "Düzelt" düğmesi çıkar. Öneri: eksik varsayılan adla
  başlayan (ya da onunla başlayan) tek bir geçerli alias; birden çoksa öneri yok ve sunulan
  model kullanılır. "Düzelt" aracın normal önizlemesini açar (yazılacak dosya farkı görünür),
  onaylanınca bu yazmanın yedeği ve defteri her zamanki gibi tutulur; OpenCode biçimli
  araçlarda sağlayıcı bloğu gateway'in sohbet modelleriyle yeniden yazıldığından eski adlar
  düşer.
- Profile kendi tanımı verilince ortaya çıkan ad değişikliği de aynı yoldan yakalanır
  (`<dosya>` → `<dosya>-<profil>` ön ek eşleşmesiyle önerilir).
- **Elle yazılmış sağlayıcı blokları:** Harness'in kendi `harness` bloğu dışında, kullanıcının
  yazdığı bir blok Harness gateway'ini hedefliyorsa (taban adres loopback ve gateway'in şimdiki
  ya da geçmiş portu: `config/ports.json` `gateway_history` + varsayılan) o bloktaki model adları
  da aynı denetime girer: OpenCode ve Kilo Code `provider.<ad>` (+ varsayılan `model`
  `<ad>/<model>`), Codex `[model_providers.<ad>]` (+ `model_provider` o ad ise üst `model`),
  Continue `apiBase`'i gateway olan `models` girdileri. Yalnız algılanır, bu bloklar hiçbir
  zaman kendiliğinden yeniden yazılmaz. Kart aynı uyarıyı bloğun adıyla gösterir (araç
  Harness'in yazdığı bir yapılandırmaya sahip olmasa da); "Düzelt" yalnız Harness'in kendi
  bloğunu yazan olağan önizlemeyi açar, kullanıcının bloğundaki adı kullanıcı kendisi
  günceller. Gateway'i göstermeyen adresler (bulut, başka yerel sunucu, Ollama 11434) sayılmaz.
  Kapsam dışı: Cursor, Cline gibi "rehber" araçlar ve bu üç biçim dışındaki elle yazılmış
  yapılandırmalar.

### 6b. Control API yedek portu ve Harness MCP

Control API 11500 başka bir programdayken yedek porta geçer (ya da yedekten varsayılana döner).
Harness MCP'yi HTTP ile ekleyen girdiler (Claude Code `mcpServers.harness`, Codex
`[mcp_servers.harness]`, OpenCode `mcp.harness`, Cursor) adresi taşıdığından kendiliğinden
güncellenir (`harness_mcp::follow_api_port`):

- Control API her açılışta dinlediği portu `config/ports.json`'a (`control_api_history`, en çok 8)
  yazar. Girdinin adresi loopback ve portu bu örneğin kendi geçmişindeyse yalnız port yeni portla
  değişir; belirteç (Bearer) ve dosyanın geri kalanı olduğu gibi kalır, yazmadan önce
  `<dosya>.harness-<tarih>.bak` yedeği alınır. Varsayılan 11500 geçmişe kendiliğinden eklenmez:
  ayrı veri klasörüyle çalışan ve 11500'de hiç dinlememiş bir örnek, asıl uygulamanın girdisini
  kendi portuna taşımaz.
- Stdio girdilerine, uzak/LAN adreslerine ve Control API'nin hiç kullanmadığı portlara dokunulmaz.
- Kapıdır: yalnız gerçek uygulama ve `harness serve` açar (`harness_api::enable_mcp_follow`) ve
  yalnız uygulamanın kendi veri klasörü için çalışır; testlerin başlattığı Control API kullanıcının
  gerçek ana dizinindeki ayarları asla yeniden yazmaz (gateway'in `enable_client_follow` kuralıyla aynı).

## 7. Bağlantı testi

"Bağlantıyı Test Et", aracın konuştuğu API'de tek bir küçük, araçlı istek gönderir:
Claude Code ve Claude Desktop için `POST /v1/messages` (Anthropic), Codex için
`POST /v1/responses`, diğerleri için `POST /v1/chat/completions`. `get_weather` aracı tanımlanır;
sonuç durum kodu, gecikme, modelin aracı çağırıp çağırmadığı ve hata ayrıntısıyla gösterilir.

### Gerçek istemci doğrulaması (Windows)

Qwen3-4B (`q4-vulkan`) gateway üzerinden, her istemcide bir gerçek tur:

| İstemci | Yol | Sonuç |
|---|---|---|
| OpenCode 1.2.17 (`opencode-cli`) | `harness launch opencode --prepare -- run "…"` (`OPENCODE_CONFIG_CONTENT`) | "pong"; bağlam 8192 → 65536 hazırlandı |
| OpenCode | Bağlantılar'ın kalıcı yazımı, geçici ana dizinde (`USERPROFILE`), kullanıcının mevcut `opencode.json`'ının kopyası | yedek `.harness-<tarih>.bak`; `opencode-cli run -m harness/q4-vulkan` "pong"; Geri al dosyayı baytı baytına eski hâline getirdi |
| Aider 0.86.2 (izole venv) | `harness launch aider --prepare -- --no-git --yes --message "…"` | "pong", 605 token. Aider kalıcı dosya yazmıyor (`guide_only`) |
| Goose 1.52.0 (resmi zip) | `harness launch goose --prepare -- run -t "…"` | "pong" |
| Goose | kalıcı `custom_providers/harness.json` (geçici `APPDATA`), `GOOSE_PROVIDER=harness` | "pong"; Geri al dosyayı sildi, yedek bıraktı |
| Claude Code 2.1.283 | "Harness MCP'yi Ekle" yazımı geçici ana dizine; dosya `claude -p --mcp-config … --strict-mcp-config --allowedTools mcp__harness__list_models` ile | `list_models` çağrıldı, yanıt "11" (o örnekteki tanım sayısı); haiku, 3 tur |

Bulgular: (1) `%LOCALAPPDATA%\OpenCode` PATH'e girerse `opencode` GUI exe'sine (`OpenCode.exe`, büyük/küçük harf duyarsız) çözülüyor; PATH'te değilken Harness `opencode-cli.exe`'yi doğru seçiyor. (2) Aider çıktısı Türkçe Windows konsolunda "Terminal does not support pretty output (UnicodeDecodeError)" uyarısı veriyor, çalışmayı etkilemiyor. (3) Goose yalnız `harness launch` ortam değişkenleriyle (`GOOSE_PROVIDER=openai`) çalışırken de, kalıcı özel sağlayıcıyla da (`harness`) aynı sonucu verdi; ilk çalıştırma `%APPDATA%\Block\goose` oluşturur.

Resmi SDK'lar (gateway :21435): `openai` Python 3.22.1 ve JS 7.25.0, `ollama` Python 0.6.3 ve JS 0.6.4; sohbet, akış, araç çağrısı, embeddings, `show`/`ps`/`list` geçti. Gerçek Ollama 0.34.4 `pull` (`hf.co/...:Q8_0`) ve vision (`images`) için ROADMAP P2 Ollama maddesine bakın.

### Ollama CLI ile model oluşturma (`ollama create`)

Gateway'in Ollama uyumlu katmanı `ollama create` / `cp` / `rm` komutlarını model **tanımları** üzerinde çalıştırır (ayrıntı: ARCHITECTURE §13.4):

```
set OLLAMA_HOST=127.0.0.1:21435
ollama create benim-modelim -f Modelfile   # FROM ./model.gguf (CLI GGUF'u /api/blobs ile yükler) ya da FROM <Harness modeli>
ollama cp benim-modelim kopya
ollama rm kopya
```

- `FROM <ad>`: gateway'deki var olan model (`ollama list` / `/api/tags` adı); `FROM ./dosya.gguf`: CLI dosyayı `ollama-blobs/` altına yükler (SHA-256 doğrulanır). Ollama kayıt defteri adları (`llama3`) çözülmez, `hf.co/...` yalnız indirilmiş dosyalar için çalışır.
- `PARAMETER`, `SYSTEM`, `MESSAGE`, `ADAPTER` (yüklenen GGUF LoRA), `TEMPLATE` Kütüphane'nin Modelfile içe aktarmasıyla aynı kurallarla eşlenir; eşlenemeyenler `warning:` satırı olarak CLI'da görünür.
- `--quantize` yok (501), `ollama push` yok (501, kayıt defteri yok), safetensors yok (yalnız GGUF).
- Silme: `ollama rm` yalnız tanımı siler; indirilen/kütüphanedeki GGUF dosyaları kalır. `ollama create` ile yüklenmiş ve başka tanımın kullanmadığı blob dosyaları silinir.
- Bu uçlar gateway'in kimlik doğrulamasından geçer (anahtar varsa gerekir); anahtarsız yerel kullanımda aynı makinedeki her program tanım oluşturabilir/silebilir.

## 8. Doğrulanmayanlar

- Kilo Code ve JetBrains AI Assistant gerçek kurulumda denenmedi; yalnız resmi belgeye dayanır.
- macOS/Linux masaüstü yolları varsayım. (Windows'ta gerçek istemcilerle denendi:
  OpenCode 1.2.17, Aider 0.86.2, Goose 1.52.0, Claude Code 2.1.283; yukarıya bakın.)
- GUI (bölümler, "Başlat", test, yedek rozetleri) gerçek pencerede elle denenmedi;
  `svelte-check` geçiyor.

### İstemci yolları ve anahtar biçimleri: doğrulama kaynakları (Eylül 2026'da kontrol edildi)

| İstemci | Doğrulanan | Kaynak |
|---|---|---|
| VS Code | Özel uç nokta `chatLanguageModels.json`; `vendor: customendpoint`; `apiType` `chat-completions`/`responses`/`messages`; model `url` tam uç nokta; `apiKey` için `${input:...}`; varsayılan başlık `Authorization: Bearer`. Yol: Windows'ta gerçek kurulumda `%APPDATA%\Code\User\chatLanguageModels.json` var (salt okunur bakıldı); belge yolu söylemez, macOS/Linux standart kullanıcı dizini varsayımı. | https://code.visualstudio.com/docs/copilot/customization/language-models, https://github.com/microsoft/vscode-docs/blob/main/docs/agent-customization/language-models.md |
| Zed | `language_models.openai_compatible.<ad>.api_url` + `available_models[]`; anahtar settings.json'a yazılmaz; ortam değişkeni `<AD>_API_KEY` (UpperSnake; kaynakta `format!("{}_API_KEY", id).to_case(Case::UpperSnake)`), yani `HARNESS_STUDIO_API_KEY`. | https://zed.dev/docs/ai/use-api-access, https://github.com/zed-industries/zed/blob/main/crates/language_models/src/provider/api_compatible.rs |
| Kilo Code | Genel yapılandırma `~/.config/kilo/kilo.json[c]`, Windows'ta `C:\Users\<kullanıcı>\.config\kilo\kilo.jsonc`; eski `opencode.json[c]` da okunur; `provider.<ad>.options.{baseURL,apiKey}`, `{env:DEĞİŞKEN}` sözdizimi. | https://kilo.ai/docs/getting-started/settings, https://kilo.ai/docs/code-with-ai/platforms/cli, https://github.com/Kilo-Org/kilocode/blob/main/packages/opencode/src/kilocode/skills/kilo-config.md |
| JetBrains AI Assistant | Settings \| Tools \| AI Assistant \| Providers & API keys (2026.2); alanlar URL, API Key, Tool calling, Test Connection. | https://www.jetbrains.com/help/ai-assistant/use-custom-models.html |
