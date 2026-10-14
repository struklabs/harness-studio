# `harness` CLI ve headless mod

[English](../en/CLI.md)

`harness`, Harness Studio'nun komut satırı arayüzüdür. GUI ile aynı çekirdeği (`harness-core`) ve aynı Control API'yi (`harness-api`) kullanır; ayrı bir iş mantığı yoktur. Kaynak: `crates/harness-cli`.

```
cargo build -p harness-cli --release     # target/release/harness(.exe)
harness --help
```

## Kurulum ve PATH

Masaüstü uygulamasının kurulumu `harness`'i uygulamanın yanına koyar (Tauri sidecar: Windows/Linux'ta `harness-studio(.exe)` ile aynı klasör, macOS'ta `Harness Studio.app/Contents/MacOS/harness`; Linux `.deb` paketinde `/usr/bin/harness`, yani zaten PATH'te). Terminalden her yerde çalışması için **Ayarlar > Geliştirici > CLI > Terminalde Kullan** bölümündeki **PATH'e Ekle** düğmesi kullanılır; yalnız tıklayınca çalışır, **Kaldır** geri alır:

- **Windows:** uygulamanın klasörü kullanıcı PATH'ine (`HKCUEnvironmentPath`, değer türü korunur, diğer girdilere dokunulmaz) eklenir ve `WM_SETTINGCHANGE` yayınlanır. Açık terminaller değişikliği görmez, yeni terminal açın.
- **macOS/Linux:** `~/.local/bin/harness` sembolik bağı oluşturulur. `~/.local/bin` PATH'te değilse arayüz `export PATH="$HOME/.local/bin:$PATH"` satırını gösterir. Kaldırırken yalnız bu CLI'a işaret eden bağ silinir; başka bir `harness` dosyası varsa ekleme reddedilir.

CLI'ın bulunma sırası (`harness_core::integrations::cli_install::locate`; Harness MCP kurulumu, tanılama ve bu bölüm aynı işlevi kullanır): çalışan uygulamanın yanı (sidecar; geliştirme derlemesinde aynı klasöre derlenen `harness`), sonra PATH'teki ilk `harness`, sonra kullanıcının girdiği yol.

## Nasıl çalışır: keşif

Her komut bir Control API ile konuşur. Hangisiyle konuşacağı sırayla şöyle belirlenir:

1. **`--host` / `HARNESS_HOST`** verildiyse o uzak API. Token `--token` / `HARNESS_TOKEN` ile verilir (isimli token; kapsamlar geçerli).
2. **Çalışan bir sahip varsa** (GUI ya da `harness serve`): veri dizinindeki `instance.json` okunur, API adresi ve oturum token dosyası oradan alınır. Komut sahibin API'si üzerinden yürür; CLI başka bir sürecin veri dizinine hiçbir zaman yazmaz.
3. **Sahip var ama Control API'si kapalıysa** (GUI'de Ayarlar > Geliştirici > Control API kapalı): çıkış kodu 4, `cli.owner_without_api`.
4. **Sahip yoksa** (ya da `instance.json` bir çökmeden kalmışsa): komut, bu süreç içinde **geçici bir örnek** açar. Geçici örnek `instance.lock`'u alır, aynı Control API'yi rastgele bir loopback portunda başlatır, komut bitince durdurduğu her şeyi kapatır ve kilidi bırakır. `--json` çıktısı bu yüzden her yolda aynı şemadadır.
   - llama-server başlatan komutlar (`load`, `run`, `chat`, `profile run`) geçici örneği yalnız **`--headless`** ile açar; bu durumda başlattıkları sunucular komut bitince durur.
   - Var olan sunucular üzerinde çalışan komutlar (`stop`, `unload`, `logs`, `shutdown`, `pull --detach`) geçici örnek açmaz; çalışan bir örnek ister (yoksa çıkış 6).

`status`, `doctor`, `search`, `speak`, `tts`, `transcribe`, `asr`, `completion` ve `service` hiçbir örneğe ihtiyaç duymaz ve kilit almaz. `status` ile `doctor` yalnız okur (yetenek sondası bile diske önbelleğe yazılmaz).

### Veri dizini

Varsayılan, GUI'ninkiyle aynıdır (Tauri `app_data_dir`):

| OS | Yol |
|---|---|
| Windows | `%APPDATA%\com.harnessstudio.app` |
| macOS | `~/Library/Application Support/com.harnessstudio.app` |
| Linux | `$XDG_DATA_HOME/com.harnessstudio.app` (varsayılan `~/.local/share/...`) |

`--data-dir` / `HARNESS_DATA_DIR` ile değiştirilir. Testler her zaman geçici bir dizin kullanır.

## Genel seçenekler

| Seçenek | Ortam değişkeni | Anlamı |
|---|---|---|
| `--json` | | Veri stdout'a JSON olarak; hata stderr'e `{"error": {"code", "message", "exit_code", "params"}}` |
| `-y`, `--yes` | | Onayları evetle geçer. Terminal yokken yıkıcı komutlar `--yes` olmadan çıkış 2 ile durur |
| `-q`, `--quiet` | | stderr'deki ilerleme satırlarını kapatır |
| `--data-dir DIR` | `HARNESS_DATA_DIR` | Veri dizini |
| `--host HOST` | `HARNESS_HOST` | Uzak Control API (`host:port` ya da URL; port yoksa 11500) |
| `--token TOKEN` | `HARNESS_TOKEN` | `--host` için token |
| `--headless` | | Çalışan örnek yoksa komut süresince geçici örnek |
| | `HARNESS_MEMORY_SECRETS=1` | İşletim sistemi anahtarlığı yerine bellekte sır deposu (testler ve yalıtılmış çalıştırmalar) |
| | `HARNESS_SERVER_API_KEY` | Anahtarlı bir llama-server'a `run`/`chat` için `--api-key` |

stdout yalnız veri taşır; ilerleme, uyarı ve hata stderr'e gider. İlerleme satırları terminalde yerinde güncellenir, terminal yoksa seyrek satırlar olarak yazılır.

## Çıkış kodları

| Kod | Anlamı | Örnek |
|---|---|---|
| 0 | Başarılı | |
| 1 | Genel hata | token reddedildi (`api.unauthorized`, `api.insufficient_scope`), sunucu başlayamadı (`cli.server_failed`, `servers.crashed`, `servers.corrupt_model`, `servers.missing_library`, `servers.unsupported_architecture`, `servers.invalid_argument`, `servers.load_timeout`...), indirme başarısız |
| 2 | Kullanım hatası | bilinmeyen komut/seçenek, geçersiz `owner/repo`, bilinmeyen kapsam, belirsiz model adı, terminalsiz onay (`cli.confirmation_required`) |
| 3 | Bulunamadı | model, profil, sunucu, benchmark, token, llama-server'ın açamadığı model dosyası (`servers.model_not_found`, `cli.not_found`, `api.not_found`, `models.not_found`, `definitions.alias_not_found`, `downloads.variant_not_found`) |
| 4 | Çakışma | veri dizini başka süreçte (`cli.lock_held`, `cli.owner_without_api`), port dolu (`api.bind_failed`, `servers.port_in_use` ve `servers.port_in_use_by` / `_at` / `_server`: başlatmadan önce ya da llama-server bağlanamadı; mesaj portu tutan programı adlandırır), aynı kimlikli profil (`servers.exists`), model kullanımda (`models.in_use`), sunucu zaten çalışıyor, anahtarsız ağ bağlanması (`network.lan_without_key`) |
| 5 | Bellek yetersiz | llama-server yüklenirken ayırma hatası bildirdi (`servers.out_of_memory`; sınıflandırma yoksa `cli.out_of_memory`: `ErrorOutOfDeviceMemory`, `failed to allocate`, `cudaMalloc failed`, `out of memory`...). `--json` hatasında `failure` ve `recovery` (kurtarma seçenekleri) alanları; metin çıktısında `harness retry ...` önerileri |
| 6 | Bağlantı hatası | API'ye ulaşılamadı (`cli.connection_failed`), çalışan örnek yok (`cli.no_instance`), llama-server uç noktasına ulaşılamadı |

HTTP durumları şöyle eşlenir: 400 → 2, 404 → 3, 409 → 4, 401/403/422/500 → 1, bağlantı hatası → 6.

## Hata mesajları ve dil

CLI çıktısı İngilizcedir. Çekirdek ve API hataları i18n anahtarı döndürür (`models.in_use`, `api.not_found`...):

- GUI'nin bildiği anahtarların (`src/lib/network.ts` → `ERROR_KEYS`) CLI metni GUI'nin İngilizce çevirisiyle **aynıdır**; bir birim testi `src/lib/locales/*.ts` ile karşılaştırır, metinler ayrışamaz.
- Yalnız API'nin ve CLI'ın kodları (`api.unauthorized`, `cli.no_instance`...) `crates/harness-cli/src/messages.rs` içinde.
- Çekirdeğin diğer hataları da anahtardır (`harness_core::error_key`: `anahtar {"ad": "değer"}`); İngilizce metin GUI'nin hata locale dosyalarından (`src/lib/locales/errors*.ts`, derlemeye gömülü) alınır, `{ad}` yerleri parametrelerle doldurulur. Parametreler teknik ayrıntıyı (yol, işletim sistemi hata metni) çevrilmeden taşır; bir parametre iç içe bir anahtarlı hata olabilir, o da çevrilir. Bir test çekirdekteki her `err("…")` anahtarının metni olduğunu denetler.
- Bağlamı olmayan ham işletim sistemi/kütüphane hataları hâlâ serbest metindir (`core.error`, `params.message`) ve olduğu gibi yazılır.
- Bilinmeyen bir anahtar hiçbir zaman boş kalmaz: anahtarın kendisi ve parametreleri yazılır.
- `--json`'da `code` her zaman makine kodudur; metne göre dallanmayın.

## Komutlar

`--json` çıktısı, aksi yazılmadıkça ilgili Control API yanıtının kendisidir; şema `GET /api/harness/v1/openapi.json`'dadır.

### Durum

| Komut | Ne yapar | `--json` |
|---|---|---|
| `status` | Veri dizini, sahip (yok / çalışıyor / API kapalı / bayat), sahibin `/status` özeti (gateway satırı: çalışıyor mu, adresi, token gerekiyor mu); sahip yoksa çözülen runtime | `{data_dir, owner: {state, info}, via, api_url, api, runtime}` (`api.gateway`: `{running, host, port, endpoint, require_token, error, ollama_port, ollama_warning}`); `--host` ile `{via: "remote", api_url, api}` |
| `doctor` | Kontroller (`id`): `data_dir` (yazılabilir mi, token dosyası yalnız kullanıcıya mı açık), `instance` (sahip), `runtime` (yol ve sürüm), `runtime_compat` (kayıtlı ile gerçek build, etkinleştirilmemiş staging, yapılandırılan runtime yerine sistemdeki binary'ye düşme), `gpu_libraries` (Windows: `vulkan-1.dll`, `nvcuda.dll`, `amdhip64*.dll`; Linux: `ldconfig -p` ve bilinen dizinler; macOS: Metal; runtime klasöründeki `ggml-*` backend'leri), `gpu_devices` (`--list-devices`), `hardware`, `ports` (Control API, varsayılan sunucu ve profil portları dolu mu, iki profil aynı portu mu kullanıyor; otomatik portlu profiller atlanır). Aynı rapor GUI'de Ayarlar > Tanılama'da | `{ok, data_dir, version, checks: [{id, status: ok\|warn\|fail, summary, message_key, params, detail}], runtime}`. `summary` İngilizce metin; `message_key` (`doctor.*`) ve `params` kararlıdır. Bir kontrol `fail` ise çıkış 1 |

### Modeller

| Komut | Ne yapar | `--json` |
|---|---|---|
| `ls` (`list`) | Yerel modeller ve üzerlerindeki model tanımları | `GET /models` |
| `show <model>` | Dosya, tür, mimari, kuant, boyut, bağlam, parametre, parçalar ve tanımlar; `memory:` satırında bu makinedeki bellek tahmini (varsayılan bağlam, tam offload; önceki bir yüklemeden ölçüm varsa `measured`/`corrected`) ve önerilen `-ngl`; `default load:` satırı varsayılan yüklemenin başlayacağı ayarlardır (bağlam, `-ngl`, `-np`, KV türleri, flash attention; `GET /models/{id}/automatic`, yüklemenin ve Dashboard formunun kullandığı kuralın aynısı). `<model>`: kütüphane kimliği/yolu, dosya adı, ad ya da tanım alias'ı | `{model: LibraryEntry\|null, definitions: [...], fit: FitResult\|null, automatic: AutomaticLaunch\|null}` (`GET /models/{id}/fit`) |
| `search <q> [--limit N] [--sort S]` | Hugging Face'te GGUF araması (örnek gerekmez; HF token'ı anahtarlıktan okunur) | `HfModelSearchResult[]` |
| `pull <owner/repo[:quant]> [--file F] [--detach]` | İndirir ve bitene kadar ilerlemeyi gösterir. `hf.co/...` ve `https://huggingface.co/...` de kabul edilir. Ctrl+C işi duraklatır (tekrar `pull` sürdürür). `--detach` kuyruğa alıp döner (çalışan örnek ister). Hazır listedeki bir görsel modeli (FLUX, SD3.5) çekilirse eksik VAE ve metin kodlayıcı dosyaları da indirilir ve beklenir (`companions`); model tanımı hepsi gelince kendiliğinden oluşur | `{job_id, variant, companions, download}`; `--detach`: `POST /models/pull` yanıtı |
| `rm <model> [--definitions] [-y]` | Modelin tüm dosyalarını siler (sunucuda kullanılıyorsa 4). `--definitions` üzerindeki tanımları da siler | `DeleteReport` |

### Hugging Face hesabı

Örnek gerekmez; token OS anahtarlığında durur ve GUI, `harness serve`, `search`, `pull` aynısını kullanır (OAuth tokenı gerekirse önce yenilenir).

| Komut | Ne yapar | `--json` |
|---|---|---|
| `hf login [--client-id ID] [--no-browser]` | Tarayıcıda huggingface.co'yu açar, onayı `127.0.0.1:<rastgele port>/callback`'te bekler (OAuth + PKCE, en çok 5 dk). Adres stderr'e de yazılır. Client ID: `--client-id`, Ayarlar ya da `HARNESS_HF_OAUTH_CLIENT_ID`; yoksa ne kaydedileceği yazılır ve 1 ile çıkar | `HfAuthStatus` (token yok) |
| `hf login --device [--client-id ID]` | Cihaz kodu akışı: kod ve adres stderr'e yazılır, huggingface.co'da onaylanana kadar beklenir (tarayıcısız makineler) | `HfAuthStatus` |
| `hf login --with-token` | Erişim tokenını stdin'den okur (`harness hf login --with-token < token.txt`; terminalde tek satır, yazılan görünür), önce `whoami-v2` ile doğrular, sonra kaydeder. OAuth oturumunun yerini alır | `{kind: "token", user}` |
| `hf logout` | Token ve OAuth oturumunu anahtarlıktan siler; grant'ın iptal edileceği sayfayı yazar | `{logged_out, revoke_at}` |
| `hf whoami` | Hesabı çevrimiçi doğrular (süresi dolmuş/iptal edilmiş token burada görünür). Giriş yoksa 1 | `{kind, user, expires_at, scope}` |

### Seslendirme (metinden konuşmaya)

Örnek gerekmez: komutlar bu süreçte çalışır ve kilit almaz. Ses `sherpa-onnx-offline-tts` ile üretilir (yönetilen sherpa-onnx runtime'ı, ); ses modelleri model dizinlerindeki klasörlerdir (Kokoro, Piper/VITS) ve kütüphanede `tts` türüyle görünür. Aynı iş gateway'de `POST /v1/audio/speech`.

| Komut | Ne yapar | `--json` |
|---|---|---|
| `speak [METİN] [-m MODEL] [--voice SES] [-s HIZ] [-f wav\|pcm\|mp3\|opus\|aac\|flac] [-o DOSYA] [--runtime ID] [--threads N]` | Metni okur ve sesi dosyaya yazar (`-o` yoksa `speech.<biçim>`; `-o -` ses verisini stdout'a yazar, terminale yazmayı reddeder). Metin argümandır; `-` ya da argümansız (boru) ise stdin'den okunur. `-m` yoksa kurulu ilk model; model adı, klasör adı ya da `tts-1` gibi OpenAI adı olabilir. `--voice` model sesinin adı (`af_heart`, `fettah`), sıra numarası ya da OpenAI adıdır (`alloy` → Kokoro `af_alloy`); verilmezse modelin varsayılan sesi (Kokoro `af_heart`). Hız 0,25-4,0. Metin en çok 4096 karakter. `wav` ve `pcm` her zaman, diğer biçimler yalnız ffmpeg varsa. Model yoksa 3 (`tts.no_model`), geçersiz biçim/hız/metin 2, runtime yoksa `runtime.family_not_installed` | `{file, model, voice, format, sample_rate, duration_secs, bytes, elapsed_ms}` (`-o -` ile `--json` kullanılamaz: çıkış 2) |
| `tts ls` | Kurulu ses modelleri: aile, diller, kuant, boyut, sesler | `[{name, id, family, languages, quantization, sample_rate, size_bytes, voices}]` |
| `tts catalog` | Önerilen indirmeler (resmi sherpa-onnx sürümü): Kokoro çok dilli v1.0 int8 (Apache-2.0), Türkçe Piper sesleri `fettah` ve `fahrettin` (CC0); boyut, lisans, kurulu mu | `[CatalogEntry + installed]` |
| `tts install <id> [-y]` | Katalogdaki modeli SHA-256 doğrulamasıyla indirip açar (`<modeller>/tts/<id>/`); ilerleme stderr'e yazılır | kurulan model |
| `tts rm <model> [-y]` | Ses modeli klasörünü siler (yalnız model dizinlerinin içindeki, ses modeli olarak tanınan klasörler) | `{removed}` |

Runtime bir kez kurulur: `harness runtime add --github k2-fsa/sherpa-onnx --family sherpa-onnx` (GUI'de Konuşma sekmesindeki "sherpa-onnx Kur", Ayarlar > Kurulu Model Motoru > "Diğer Model Motorları" ya da Gelişmiş düzeyde Ayarlar > Registry > "sherpa-onnx Deposunu Doldur").

```sh
harness tts install vits-piper-tr_TR-fettah-medium-int8
harness runtime add --github k2-fsa/sherpa-onnx --family sherpa-onnx -y
harness speak "Merhaba dünya" --model fettah -o merhaba.wav
echo "Read this aloud." | harness speak - -f mp3 -o out.mp3
```

### Konuşma tanıma (sherpa-onnx)

Örnek gerekmez: komutlar bu süreçte çalışır ve kilit almaz. Metin `sherpa-onnx-vad-with-offline-asr` ile okunur (seslendirmedeki sherpa-onnx runtime'ı); modeller model dizinlerindeki klasörlerdir (NVIDIA Parakeet, SenseVoice, Omnilingual...) ve kütüphanede "Konuşma tanıma (sherpa-onnx)" olarak görünür. Kütüphanedeki ONNX CTC modelleri (wav2vec2, HuBERT, WavLM) de `harness transcribe DOSYA -m MODEL` ile tanım kimliği/takma adı veya tam ONNX dosya yolu üzerinden seçilebilir. Yönetilen ONNX Runtime motoruyla çalışır, istem ve dil ipuçlarını yok sayar, zamanlı parçalar üretmez. Whisper (ggml `.bin`) modelleri bu komutlarla değil gateway'den (`POST /v1/audio/transcriptions`) kullanılır; aynı uç nokta kurulu sherpa modellerini de adıyla çalıştırır.

| Komut | Ne yapar | `--json` |
|---|---|---|
| `transcribe DOSYA [-m MODEL] [-l DİL] [-f text\|json\|verbose_json\|diarized_json\|srt\|vtt] [-o DOSYA] [--diarize] [--speakers N] [--runtime ID] [--threads N]` | Ses dosyasını yazıya çevirir. `--diarize` konuşmacıları ayırır: her bölüm kimin konuştuğunu söyler (A, B...; metin ve SRT'de `A: ...`, VTT'de `<v A>`, JSON biçimlerinde `speaker`); `--speakers N` sayıyı biliyorsanız verir (yoksa bulunur) ve `--diarize` anlamına gelir; iki küçük konuşmacı modeli (35 MB) ilk kullanımda iner. WAV (16 bit mono) olduğu gibi okunur, diğer biçimler (MP3, M4A, WebM...) ffmpeg gerektirir. Uzun kayıt duraklardan bölünür, her parçanın zamanı çıkar. `-m` yoksa kurulu ilk model; model adı, klasör adı, dosya yolu ya da adın benzersiz bir parçası (`parakeet`) olabilir. `-l` yalnız SenseVoice'ta etkilidir (`zh en ja ko yue`); diğer modeller dili kendileri bulur. Çıktı stdout'a (`-o` ile dosyaya) yazılır; ilerleme ve özet stderr'e. Model yoksa 1 (`stt.no_model`), runtime yoksa `runtime.family_not_installed` | `-o` yoksa tam sonuç (`verbose_json`: `text`, `duration`, `segments`); `-o` ile `{file, model, duration_secs, segments, elapsed_ms}` |
| `asr ls` | Kurulu tanıma modelleri: aile, diller (katalogdan), kuant, boyut | `[{name, id, family, languages, quantization, size_bytes}]` |
| `asr catalog` | Önerilen indirmeler (resmi sherpa-onnx `asr-models` sürümü): Parakeet TDT 0.6B v3 int8 (25 Avrupa dili, CC-BY-4.0), SenseVoice Small int8 (zh/en/ja/ko/yue), Omnilingual ASR 300M v2 int8 (1600 dil, Türkçe dahil, Apache-2.0), Nemotron 3.5 ASR streaming 0.6B int8 (Türkçe dahil 35 dil, noktalamalı), Cohere Transcribe int8 (14 dil; `--language` verin); boyut, lisans, kurulu mu | `[CatalogEntry + installed]` |
| `asr install <id> [-y]` | Katalogdaki modeli SHA-256 doğrulamasıyla indirip açar (`<modeller>/asr/<id>/`); Silero VAD (644 KB) birlikte iner; ilerleme stderr'e yazılır | kurulan model |
| `asr rm <model> [-y]` | Model klasörünü siler (yalnız model dizinlerinin içindeki, tanıma modeli olarak tanınan klasörler) | `{removed}` |

Runtime seslendirmeyle ortaktır, bir kez kurulur: `harness runtime add --github k2-fsa/sherpa-onnx --family sherpa-onnx`. Sürümde Türkçeye özel bir model yoktur: Türkçe için Omnilingual (noktalamasız, küçük harfli) ya da Whisper (Model Hub'dan whisper.cpp modeli) kullanılır.

```sh
harness asr install sherpa-onnx-omnilingual-asr-1600-languages-300M-ctc-v2-int8-2026-02-05
harness runtime add --github k2-fsa/sherpa-onnx --family sherpa-onnx -y
harness transcribe toplanti.wav -m omnilingual -f srt -o toplanti.srt
harness transcribe konusma.mp3 -m parakeet
```

### Lemonade (Ryzen AI NPU ve kendi llama.cpp yapıları)

Örnek gerekmez: komutlar bu süreçte çalışır. Lemonade isteğe bağlı bir motordur ([ARCHITECTURE.md](ARCHITECTURE.md) 14.5): Ryzen AI NPU'larında FastFlowLM ile model çalıştırır ve kendi ROCm / Vulkan llama.cpp yapılarını sunar. Kurulduktan sonra indirilen modelleri gateway'de Lemonade adlarıyla görünür; gateway `lemond`'u gerektiğinde kendisi başlatır. `pull`, `rm`, `backend` ve `models --refresh` komutları kendi `lemond`'unu komutun süresince başlatır ve sonunda durdurur (açık bir Harness penceresinin `lemond`'undan ayrıdır). Değişiklik yapan komutlar onay ister (`-y`).

| Komut | Ne yapar | `--json` |
|---|---|---|
| `lemonade status` | Kurulu sürümler, llama.cpp arka ucu ayarı, NPU durumu (algılandı / sürücüsüz / algılanmadı), `lemond` bu süreçte çalışıyor mu, son çalıştırmanın arka uçları | `LemonadeStatus` (`GET /lemonade`) |
| `lemonade install [--tag T]` | Kararlı en yeni Lemonade sürümünü (GitHub `lemonade-sdk/lemonade`, `lemonade-embeddable-*`) indirir, SHA-256 doğrular, `lemond --version` dener ve `lemonade` ailesi runtime olarak kaydeder. Yeni sürüm yeni kayıttır; eskisi geri alma yoludur | eklenen kayıt |
| `lemonade uninstall <id>` | Kurulu bir sürümü siler (çalışıyorsa `lemonade.in_use`) | `{removed}` |
| `lemonade config [--backend auto\|vulkan\|rocm\|cpu] [--runtime ID\|default]` | llama.cpp arka ucunu ve çalışacak sürümü (geri alma için eskiyi sabitleme) ayarlar | `LemonadeSettings` (`PUT /lemonade/settings`) |
| `lemonade models [--refresh] [--downloaded]` | Lemonade'in model kayıt defteri. `--refresh` olmadan son çalıştırmanın listesi; `--refresh` `lemond`'u başlatıp güncel listeyi okur | `{live, updated_at, models}` (`GET /lemonade/models`) |
| `lemonade pull <model>` | Kayıt defteri adı (`Qwen3-0.6B-GGUF`) ya da `owner/repo[:varyant]` ile modeli Lemonade'e indirtir; ilerleme stderr'e | `{pulled}` (`POST /lemonade/models/pull`) |
| `lemonade rm <model>` | İndirilmiş modeli siler | `{removed}` |
| `lemonade backend <recipe> <backend>` | Bir arka uç kurar (`llamacpp rocm`, `llamacpp vulkan`, `flm npu`...). NPU yoksa `flm npu` kurulamaz | `{installed}` (`POST /lemonade/backends/install`) |

```sh
harness lemonade install
harness lemonade config --backend vulkan
harness lemonade pull Qwen3-0.6B-GGUF
# harness serve açıkken ağ geçidi modeli Lemonade adıyla sunar:
curl http://127.0.0.1:11435/v1/chat/completions -H "content-type: application/json" \
  -d '{"model":"Qwen3-0.6B-GGUF","messages":[{"role":"user","content":"Merhaba"}]}'

### Safetensors'tan GGUF'a dönüştürme

Çalışan örnek gerekmez; komutlar bu süreçte çalışır. Hugging Face'teki bir safetensors LLM deposunu (metin üretimi ya da görüntülü sohbet, llama.cpp'nin tanıdığı mimariler) indirir, llama.cpp'nin resmi `convert_hf_to_gguf.py` betiğiyle GGUF'a çevirir, yönetilen çalışma zamanının `llama-quantize`'ıyla nicemler ve `<modeller>/<sahip>/<depo>-GGUF/` altına koyar (kitaplık onu her indirme gibi görür). Python yalnızca dönüştürme anında, ilk kullanımda Harness'in kendi kurduğu geçici ortamda çalışır (uv + Python + torch CPU, ~700 MB; ayrıntı ve lisanslar için [ARCHITECTURE.md](ARCHITECTURE.md) 14.5). Yönetilen bir llama.cpp çalışma zamanı kurulu olmalıdır; gated depolar için `harness hf login`.

| Komut | Ne yapar | `--json` |
|---|---|---|
| `convert <sahip/depo> [--quant TÜR] [--keep-original] [--plan] [-y]` | Önce planı yazar (mimari, parametre, indirme ve sonuç boyutu, gereken ve boş disk, ortam kurulacak mı), onay ister, sonra çalıştırır; aşama ve ilerleme stderr'e. `--quant`: `Q4_K_M` (varsayılan), `Q5_K_M`, `Q6_K`, `Q8_0`, `F16`, `BF16`. `--keep-original` indirilen safetensors'u `<modeller>/<sahip>/<depo>/` altında bırakır. `--plan` yalnız planı gösterip çıkar. Ctrl+C işi iptal eder (indirilenler kalır, aynı komut sürdürür). Desteklenmeyen mimari, LLM olmayan depo, disk yetmemesi, gated depo ya da dönüştürücü hatasında 1 ile çıkar | `--plan`: `ConvertPlan` (+ `quant`, `estimate`); aksi halde biten `ConvertJob` (`output`, `mmproj`, `warning`) |
| `convert env status` | Dönüştürme ortamı: kurulu mu, hangi llama.cpp etiketi için, boyut, konum | `EnvStatus` |
| `convert env install [-y]` | Ortamı şimdi kurar (yoksa ilk dönüştürme kurar) | `EnvStatus` |
| `convert env rm [-y]` | Ortamı siler (~700 MB geri gelir; sonraki dönüştürme yeniden kurar) | `{removed}` |

Control API: `GET /convert/plan?repo=sahip/depo`, `POST /convert` (`{repo, quant?, keep_original?}`), `GET /convert`, `GET /convert/{id}`, `POST /convert/{id}/cancel`, `DELETE /convert/{id}`, `GET|POST|DELETE /convert/env`. `POST /convert`, iptal ve silme `downloads:write` kapsamı ister; ortamı kurmak/silmek `admin`; okumalar `read`. İş değişiklikleri olay akışında `convert` ve `convert_removed` olarak gelir.

```sh
harness convert HuggingFaceTB/SmolLM2-135M-Instruct --plan
harness convert Qwen/Qwen3-0.6B --quant Q5_K_M -y
harness convert env status
```

### Sunucular

| Komut | Ne yapar | `--json` |
|---|---|---|
| `ps [-a]` | Çalışan sunucular; `-a` durmuşları ve profilleri de gösterir | `ServerView[]` |
| `load <model> [--server ID] [--no-wait] [--timeout S]` | Modeli yükler ve hazır olana kadar bekler. `<model>` bir tanım alias'ı, kütüphane modeli ya da `.gguf` yolu olabilir; tanımı yoksa dosya adından bir tanım oluşturulur (stderr'e yazılır). Aynı tanımı son sunmuş durmuş bir örnek varsa o yeniden kullanılır; yoksa bir sonraki boş portta yeni örnek açılır | `{server_id, definition, server}` |
| `retry <id> [--option O] [--set ID=V]... [--unload ID]... [--save] [--no-wait] [--timeout S]` | Hata veren sunucuyu yeniden başlatır. Seçeneksiz: hatayı ve kurtarma seçeneklerini listeler (`GET /servers/{id}/recovery`). `--option` bir seçeneği (ör. `lower_context`, `enable_fit`, `kv_cache_quant`, `unload_server:<id>`), `--set` tek tek katalog değerlerini bu başlatma için uygular. `--unload` önce başka sunucuyu durdurur. `--save` değerleri tanıma yazar (yoksa geçicidir) | Seçeneksiz: `RecoveryPlan {server_id, failure, options: [{id, kind, label_key, params, overrides, unload, recommended}], free_vram_bytes, estimated_need_bytes}`; başlatmada `ServerView` |
| `unload <model> \| --server ID` | Tanımı sunan tüm sunucuları (ya da birini) durdurur | `{stopped: [...]}` |
| `stop <id>... \| --all` | Sunucuları durdurur | `{stopped: [...]}` |
| `logs <id> [-f] [-n N]` | Son N satır; `-f` Ctrl+C'ye kadar yeni satırları izler | `LogsResponse`; `-f --json` her parti için bir JSON satırı |
| `lora list <id>` / `lora set <id> <adaptör> <ölçek>` | Çalışan sunucunun LoRA adaptörlerini (kimlik, ölçek, yol) listeler; `set` sunucuyu yeniden başlatmadan tek adaptörün ölçeğini değiştirir, diğerleri aynı kalır (0 kapatır). Sunucu çalışmıyorsa `lora.server_not_running` (çıkış 4). Kimlikler `--lora` sırasıdır; açılışta ölçeği 0 yapmak için `--lora-init-without-apply` | `GET`/`POST /servers/{id}/lora` (`{adapters: [{id, path, scale}]}`; POST gövdesi `{adapters: [{id, scale}]}`, kapsam `servers:control`) |
| `router models <sunucu>` | Router sunucusunun (`--models-dir`, tek süreçte çok model) modellerini ve durumlarını (`loaded`, `loading`, `unloaded`, `failed`) listeler (`GET /servers/{id}/router/models`). Sunucu çalışmıyorsa ya da router değilse çıkış 4 (`router.not_running` / `router.not_router`) | `{models: [{id, state, source, path, context_length, exit_code}]}` |
| `router load <sunucu> <model> [--no-wait] [--timeout S]` | Router'ın bir modelini yükler ve `loaded` olana kadar bekler (varsayılan 300 sn; yüklenemezse hata, çıkış kodu mesajda). Router `--models-max` aşılırsa en eski yüklü modeli kendisi boşaltır | `RouterModel` |
| `router unload <sunucu> <model>` | Router'ın bir modelini boşaltır | `RouterModel` |
| `shutdown` | `harness serve` örneğini temiz kapatır (`POST /shutdown`, `admin`). GUI kabul etmez (4) | `{shutdown: true}` |

`load`, `run`, `profile run` ve `retry` için `--timeout` varsayılanı 0'dır: sunucunun kendi yükleme zaman aşımı (`load_timeout_secs`, varsayılan 300 sn; tanımda `load_timeout`) başlatmayı bitirene kadar beklenir. Başlatma başarısız olursa hata `failure` nesnesini (`code`, `kind`, `message_key`, `params`, `phase`, `exit_code`, `evidence`) ve logun son satırlarını (`log_tail`) taşır.

### Çalıştırma ve sohbet

| Komut | Ne yapar | `--json` |
|---|---|---|
| `run <model> [prompt...]` | Model çalışmıyorsa yükler, istemi gönderir ve yanıtı stdout'a akıtır. İstem yoksa stdin okunur; stdin terminalse `chat` açılır. Akıl yürütme metni stderr'e soluk yazılır; sonda hız özeti stderr'e. Bu komut yüklediyse sunucu çalışmaya devam eder, `--unload` durdurur | `{model, server_id, endpoint, loaded, content, reasoning_content, finish_reason, interrupted, usage, timings}` |
| `chat <model>` | Etkileşimli sohbet; geçmiş tutulur. `/bye` (ya da Ctrl+D) çıkar, `/clear` sıfırlar. stdin bir dosya/boru olabilir (her satır bir mesaj) | her yanıt için bir JSON satırı |

| `image [prompt...] --model <görsel modeli>` | Görsel modeliyle gateway'in `/v1/images/generations` uç noktasından bir ya da birkaç görsel üretir ve dosyaya yazar. İstem yoksa stdin okunur. Seçenekler: `-o/--output` (varsayılan `image.png`; birden çok görselde `ad-1.png`, `ad-2.png`), `--size 512x512` (8'in katı; verilmezse modelin boyutu), `--negative`, `--steps`, `--cfg-scale`, `--seed` (-1 rastgele), `--sampler`, `-n/--count`, `--format png\|jpeg\|webp`, `--timeout` (model yükleme dahil, varsayılan 1800 sn). Model ilk istekte kendiliğinden yüklenir. Gateway çalışmıyorsa `cli.not_available` | `{model, files}` |

Ortak seçenekler: `--system`, `--max-tokens`, `--temperature`, `--api-key`, `--timeout`. İstek `/v1/chat/completions`'a akışlı gider:

- Örneğin gateway'i çalışıyorsa **managed endpoint** üzerinden (`endpoint` gateway adresidir; istek `x-harness-client: harness-cli` ile işaretlenir). Token gerektiren gateway'de yerel sahip için `<veri>/gateway-token`, uzak örnekte `HARNESS_TOKEN` (isimli, `inference` kapsamlı) kullanılır. Uzak örnekte yalnız loopback'e bağlı gateway kullanılamaz.
- Aksi halde doğrudan modelin llama-server uç noktasına. Uzak bir örnekte uç noktanın `127.0.0.1` adresi API'nin host'u ile değiştirilir.

### Headless

| Komut | Ne yapar |
|---|---|
| `serve [--listen HOST] [--port N] [--no-restore] [--gateway \| --no-gateway] [--gateway-port N]` | Veri dizininin sahibi olur (kilit alınamazsa 4), indirme kuyruğunu yükler, telemetri örnekleyicisini başlatır, Control API'yi açar (ayarlardaki adres/port, burada her zaman açık; ayarlardaki port başka bir programdaysa yedek porta geçer ve `warning:` yazar, `--port` ile verilen port dolu ise 4 ile çıkar), otomatik başlayan profilleri ve son oturumu geri yükler. Ctrl+C, SIGTERM/SIGHUP (Unix), konsol kapanması/oturum kapatma (Windows) ya da `harness shutdown` ile temiz kapanır: tüm llama-server'lar durur, API kapanır, `instance.json` ve `api-token` silinir. Gateway (tek uç nokta, JIT yükleme, kuyruk; varsayılan `127.0.0.1:11435`) ayarlarda açıksa ya da `--gateway` verilirse başlar, `--no-gateway` kapalı tutar, `--gateway-port` portu bu çalıştırma için değiştirir (ayarlardaki gateway portu doluysa yedek porta geçilir ve `warning:` satırı yazılır, `--gateway-port` ile verilen port yedeğe geçmez); başlayamazsa uyarı yazılır ve `serve` devam eder. Gateway'in Ollama portu (ayarlarda açıksa, P2) alınamazsa (ör. gerçek Ollama çalışıyor) `warning:` satırı yazılır. `--json`: hazır olunca `{"event": "ready", pid, kind, base_url, token_path, data_dir, version, gateway: {running, base_url, require_token, error, ollama_port, ollama_warning, port_fallback}}`, kapanınca `{"event": "stopped", reason}` satırı |
| `service install\|uninstall [--name N]` | `harness serve`'ü oturum açılışında başlatır. Linux: `~/.config/systemd/user/harness.service` + `systemctl --user enable --now`. macOS: `~/Library/LaunchAgents/com.harnessstudio.harness.plist` + `launchctl bootstrap gui/<uid>` (log: `<veri>/logs/harness-serve.log`). Windows: Görev Zamanlayıcı'da `HarnessStudio\harness-serve`, mevcut kullanıcının oturum açılışı tetikleyicisi, en düşük yetki, süre sınırı yok, hata olursa 3 kez yeniden deneme. Görev `harness.exe`'nin yanındaki `harness-hidden.exe`'yi başlatır (konsol penceresi açmayan launcher; `harness serve`'ü pencere olmadan çalıştırır, görev bitince onu da kapatır). Launcher yoksa görev `harness.exe`'yi çalıştırır ve konsol penceresi açık kalır. `--data-dir` verildiyse servise de geçer |
| `service print [--platform systemd\|launchd\|windows]` | Birimi / plist'i / görev XML'ini kurmadan yazdırır |

Windows notu: görev bir konsol uygulaması çalıştırdığı için oturum açılışında bir konsol penceresi görünür. Görevi "Sonlandır" süreci öldürür; llama-server'lar Job Object ile birlikte kapanır (temiz kapatma için `harness shutdown`).

### Profiller, benchmark, token'lar

`token` komutunun eski adı `api-token` da çalışır.

| Komut | Ne yapar | `--json` |
|---|---|---|
| `profile ls` | Kayıtlı sunucu profilleri | profil olan `ServerView[]` |
| `profile show <id>` | Profil ayrıntısı | `ServerView` |
| `profile run <id> [--no-wait] [--timeout S]` | Profili başlatır ve hazır olmasını bekler | `ServerView` |
| `profile export <id> [-o FILE]` | Profil ve model tanımı: `{"format": "harness-profile", "version": 1, profile, definition}`. Anahtar referansı ve binary yolu yazılmaz. Belge `--json` olmadan da stdout'a JSON olarak yazılır | belge ya da `{id, file}` |
| `profile import <FILE\|->` | Aynı alias'lı yerel tanım varsa onu kullanır (model dosyası farklıysa uyarır), yoksa tanımı oluşturur; sonra profili ekler. Aynı kimlikli profil varsa 4. Profil reddedilirse yeni oluşturulan tanım geri silinir | oluşturulan `ServerView` |
| `import modelfile <FILE\|-> [--alias AD] [--dry-run]` | Ollama Modelfile'ı model tanımı + çıkarım preset'i olarak içe aktarır (göreli yollar dosyanın klasöründen başlar). `--dry-run` yalnız neyin oluşacağını (eşlenen değerler, uyarılar) gösterir, kaydetmez. Alias verilmezse `FROM`'dan türetilir; kullanımdaysa hata (`--alias` ile başka ad verin). `MESSAGE` örnekleri preset'in sistem istemine "Example conversation" olarak eklenir (uyarı `messages_as_examples` bunu bildirir). Uyarılar (Go `TEMPLATE`, eşleşmeyen parametre, `MESSAGE` notu) stderr'e yazılır | `{definition, warnings}`; `--dry-run`: `{import, mapped, warnings, from}` |
| `import modelfile --ollama <ad[:etiket]> [--alias AD] [--dry-run]` | Ollama'da yüklü bir modeli içe aktarır: GGUF dosyası Ollama klasöründe (`OLLAMA_MODELS`, `~/.ollama/models`) yerinde, salt okunur kullanılır; modelin saklı parametreleri ve sistem istemi devralınır | aynı |
| `benchmark run <model> [--n-prompt 512] [--n-gen 128] [--ngl ...] [--repetitions N] [--threads N]` | llama-server'ın yanındaki `llama-bench` ile ölçer, bitene kadar bekler, Ctrl+C iptal eder. Listeler virgülle ayrılır (tarama) | `BenchmarkView` |
| `benchmark ls` / `benchmark show <id>` | Kayıtlı çalıştırmalar | `GET /benchmarks`, `GET /benchmarks/{id}` |
| `token create <ad> --scope read[,inference,...] [--model DESEN]... [--endpoint GRUP]... [--rpm N] [--tpm N] [--concurrency N]` | İsimli token; değer bir kez gösterilir. Kapsamlar: `read`, `inference`, `models:write`, `downloads:write`, `servers:write`, `servers:control`, `conversations` (sohbetleri okur/dışa aktarır/sıkıştırır; `read` içermez), `tools` (Harness araçlarını çalıştırır; `read` içermez), `admin`. `--model` izinli alias desenleri (`qwen3-*`; `*` = hepsi), `--endpoint` gateway grupları (`chat`, `embeddings`, `rerank`, `models`, `messages`, `ollama`, `metrics`; `all` = hepsi); ikisi de tekrarlanabilir ya da virgülle ayrılabilir. Sınırlar dakikada istek, dakikada token (istem + yanıt) ve aynı anda istek | `CreatedToken` |
| `token ls` | Token'lar, kapsamlar, MODELS / ENDPOINTS / LIMITS sütunları (değerleri asla) | `TokenInfo[]` |
| `token show <id>` | Bir token'ın kapsamları, izinli modelleri, uç noktaları ve sınırları | `TokenInfo` (`GET /tokens/{id}`) |
| `token set <id> [--name AD] [--scope ...] [--model ...] [--endpoint ...] [--rpm N] [--tpm N] [--concurrency N]` | Yalnız verilen alanları değiştirir; token değeri aynı kalır. Sınırda `0` = sınır yok | `TokenInfo` (`PATCH /tokens/{id}`) |
| `token rm <id> [-y]` | İptal (anında geçerli) | `{revoked}` |
| `otel status` | OpenTelemetry ayarları (başlıkların yalnız adları), gönderim sayaçları ve derlemedeki protokoller (`status.grpc`; varsayılan derlemede yalnız HTTP/protobuf) | `{settings, status}` (`GET /otel`) |
| `otel test [--endpoint URL] [--protocol http\|grpc]` | Kayıtlı (ya da verilen) toplayıcıya gerçek bir deneme span'i gönderir; toplayıcı reddederse çıkış 1. `grpc` yalnız `otel-grpc` ile derlenmiş örnekte; diğerlerinde çıkış 2 (`otel.grpc_not_built`) | `{ok, url, protocol, elapsed_ms, trace_id, error}` |
| `completion <bash\|zsh\|fish\|powershell\|elvish>` | Kabuk tamamlama betiği | |

### Runtime'lar

Resmi llama.cpp dışında isimli llama-server build'leri (ör. PrismML fork'u) kaydedilir; model tanımı (`load.runtime`) ya da sunucu profili (`serve_overrides.runtime`) bunlardan birini seçer. Seçilmezse Ayarlar > Model Motoru'ndaki resmi runtime kullanılır. `--family` `llama.cpp`, `whisper.cpp` ya da görsel üretimi için `stable-diffusion.cpp` olabilir (GitHub depo adı bunlardan biriyse otomatik seçilir: `runtime add --github leejet/stable-diffusion.cpp`; `--backend` verilmezse makineye uygun yapı: AMD'de Vulkan, NVIDIA'da CUDA, yoksa CPU). `official` ve (PATH/WinGet'te bir llama-server bulunduysa) `system` sanal kayıtlardır, eklenip silinemez. Değişiklik yapan komutlar `admin` kapsamı ister.

| Komut | Ne yapar | `--json` |
|---|---|---|
| `runtime ls` | Resmi runtime (gerçek kaynağı: yönetilen / özel / sistemde bulunan; yapılandırılandan farklıysa uyarı; etkinleştirilmemiş staging build), sistem llama-server'ı ve kayıtlı runtime'lar: sürüm, backend, güven düzeyi, bayrak sayısı, desteklenmeyen katalog parametreleri, kuant türü sayısı | `RuntimeView[]` (`GET /runtimes`) |
| `runtime add --github OWNER/REPO [--family F] [--tag T] [--asset A] [--backend B] [--allow-unverified] [--name N] [--id ID]` | GitHub sürümünden kurar. `--family` arşivi seçen kural ailesidir: `llama.cpp` (varsayılan), `whisper.cpp`, `sherpa-onnx`, `lemonade`. Önce arşiv adı, boyutu, SHA-256 digest'i ve indirme adresi gösterilip onay istenir; indirme ilerlemesi yazılır. `--backend` verilmezse donanım önerisi kullanılır (CUDA'da `cudart` arşivi de eklenir). Digest'i olmayan arşiv yalnız `--allow-unverified` ile kurulur. Kurulum `<veri>/bin/runtimes/<id>/` altına yapılır ve `--version` ile denenir | eklenen `RuntimeEntry` |
| `runtime add --local DIR [--family F] [--copy] [--name N] [--id ID]` | llama-server (ya da `--family` ailesinin ikilisini) içeren bir klasörü kaydeder (yerinde kullanılır, `--copy` veri dizinine kopyalar). Dosyaların SHA-256 manifesti çıkarılır. `--family mistral.rs` yalnız bu yoldan eklenir (hazır indirme yok): kendi derlediğiniz `mistralrs(.exe)` klasörü, örn. `harness runtime add --local C:\src\mistral.rs\target\release --family mistral.rs`; ekleme `mistralrs --version` çalıştırır ve sürümü kaydeder | eklenen `RuntimeEntry` |
| `runtime rm <id> [--force] [-y]` | Kaydı siler; indirilmiş/kopyalanmış dosyalar silinir, yerel klasöre dokunulmaz. Çalışan bir sunucu kullanıyorsa her zaman, bir tanım/profil seçiyorsa `--force` olmadan `runtime.in_use` (çıkış 4). `--force` ile silinen runtime'ı seçmeye devam eden tanım/profil başlatılırken `runtime.not_found` alır | `{removed}` |
| `runtime verify <id> [--accept-changes]` | Dosyaları yeniden hash'ler ve `--version` çalıştırır. Değişiklik varsa çıkış 4 (`runtime.changed`) ve değişen/eksik/yeni dosyalar listelenir. `--accept-changes` (yalnız yerel) mevcut dosyaları yeni temel yapar | `VerifyReport {ok, accepted, changed, missing, added, version, error}` |
| `runtime suggest <model>` | Modelin GGUF tensör türlerine bakıp hangi runtime'ın yükleyebileceğini söyler (ör. Bonsai `PQ2_0` → PrismML fork'u; kayıtlı değilse eklenecek depo) | `RuntimeAdvice` (`GET /runtimes/advice`) |
| `runtime managed status` | Ayarlar > Model Motoru'daki yönetilen resmi build: kurulu ve önceki sürüm, güncellemelerin koruduğu backend (kayıtlı ve dosyalardan tespit edilen), yedek yuvasındaki build | `ManagedRuntimeStatus` (`GET /runtime/managed`) |
| `runtime managed install [--backend B] [--allow-unverified] [-y]` | En yeni resmi build'i indirir, doğrular (SHA-256, `--version`, GPU build'inde cihaz kontrolü) ve etkinleştirir; eski build yedeğe geçer. `--backend` verilirse tam o backend (geçiş), verilmezse kurulu build'in backend'i (güncelleme; başka backend'e düşmez), hiç kurulu değilse donanım önerisi. Çalışan sunucu varsa `runtime.stop_servers_first` (çıkış 4) | `ManagedInstall` (`POST /runtime/managed/install`) |
| `runtime managed rollback [-y]` | Yedekteki build'e döner; kurulu build yedeğe geçer. Yedekte build yoksa `runtime.no_backup` (çıkış 4) | `ManagedRuntimeStatus` |

Başlatırken seçilen runtime modelin tensör türünü açıkça desteklemiyorsa (türler o build'in `llama-quantize --help` listesinden okunur) sunucu başlatılmaz: `runtime.quant_unsupported` (çıkış 4). Destek bilinmiyorsa başlatma engellenmez.

### Sohbetler

Playground sohbetleri (`state.db`); `conversations` kapsamı ister (`harness` oturum token'ı tam yetkilidir). Takma ad: `chats`. Çalışan örneğe bağlanır; sıkıştırma için gateway açık olmalıdır (özet isteği gateway'den geçer).

| Komut | Ne yapar | `--json` |
|---|---|---|
| `conversations ls [SORGU] [--folder ID] [--pinned] [--favorites] [-n N]` | Sohbetleri yeniden eskiye listeler; sorgu başlık ve mesaj içeriğinde arar (FTS) | `[ConversationSummary]` |
| `conversations show <id>` | Sohbetin geçerli yolu: mesajlar, işaretleyiciler (sıkıştırma) | `{conversation, messages}` |
| `conversations export <id> [-f json\|markdown] [-o DOSYA]` | `harness.conversation.v1` JSON'u (uygulamanın içe aktardığı dosya) ya da Markdown; `-o` yoksa stdout | JSON belge |
| `conversations docs <id>` | Sohbete ekli belgelerin üst verisi (ad, tür, boyut, durum); içerik yok | `[RagDocument]` |
| `conversations compact <id> [-m ALIAS] [--n-ctx N] [--keep-turns N] [--keep-tool-results] [--summary-tokens N] [-i TALİMAT]` | Eski turları modelin yazdığı özetle değiştirir (hiçbir mesaj silinmez; işaretleyici düğüm eklenir) | `{marker_id, marker, ...}` |
| `conversations undo <id> <işaretleyici> [--redo]` | Sıkıştırmayı geri alır (istekler tam geçmişi gönderir) ya da yeniden uygular | `{undone}` |

### Analitik

Gateway'den (managed endpoint) geçen isteklerin kaydı (`analytics.db`). Doğrudan llama-server portuna giden istekler görülemez. İstek içeriği yalnız model tanımında içerik kaydı açıksa saklanır.

| Komut | Ne yapar | `--json` |
|---|---|---|
| `analytics [--range 24h\|7d\|30d\|90d\|all]` | Model bazında istek, token, ortalama üretim t/s, TTFT ve gecikme P50/P95, VRAM tepe, hata ve OOM oranı, son kullanım; istemci dağılımı; son llama-bench sonucu ile gerçek hız. Varsayılan `7d`. Ham satır saklama süresinden eski günler günlük toplamlardan gelir (yüzdelik yok). Geçersiz aralık: `api.invalid_range` (çıkış 2) | `GET /analytics/summary` (`AnalyticsSummary`) |
| `requests [--model A] [--client C] [--status ok\|error\|oom\|cancelled] [--tool-issue] [--tool-category runtime\|context\|model\|client] [-n N]` | Son N istek (varsayılan 20): zaman, model, istemci, uç nokta, durum (araç teşhisi varsa `tool:<kategori>/<kod>` eklenir), token, TTFT, toplam süre, üretim t/s. `--tool-issue` yalnız araç çağrısı bulgusu olan istekleri getirir ([TOOL-DIAGNOSIS.md](TOOL-DIAGNOSIS.md)) | `GET /analytics/requests` (`RequestPage`) |
| `requests <id>` | Tek istek: tüm alanlar, zaman çizelgesi (yükleme → kuyruk → istem → üretim → diğer), hata ayrıntısı, araç teşhisi (kategori, kod, kanıt); içerik kaydedildiyse `--json` çıktısında | `GET /analytics/requests/{id}` (`RequestDetail`) |

### Araçlar

Harness'ın kendi araç motoru (`harness_core::tools`): llama-server'ın yedi aracıyla aynı ad ve şemada `read_file`, `file_glob_search`, `grep_search`, `get_info` (kısıtlı), `write_file`, `edit_file`, `exec_shell_command` (tehlikeli). Politika (`settings.json` → `tools`: `off`, `ask_every_time`, `allow_trusted`, `allow_all`; varsayılan `ask_every_time`) her çağrının çalışıp çalışmayacağını, sorulup sorulmayacağını ya da reddedileceğini belirler. Dosya araçları çalışma dizininin dışına çıkamaz.

| Komut | Ne yapar | `--json` |
|---|---|---|
| `tools list` (`tools ls`) | Araçlar: ad, sınıf (`safe` / `restricted` / `dangerous`), kaynak, politikanın şimdiki kararı (`run`, `ask`, `refuse`), açıklama; üstte politika ve güvenilir araçlar | `GET /tools` (`ToolListing`) |
| `tools mcp [--retry]` | Harness'ın kendi MCP istemcisi: yapılandırılmış her sunucu için durum (`idle`, `connecting`, `connected`, `failed` + hata, `disabled`), taşıyıcı, müzakere edilen MCP sürümü ve araç sayısı. Sunucuları ilk çağrıda başlatır; `--retry` başlamayanları yeniden dener. `AUTH` sütunu OAuth durumunu gösterir (`required`: yetkilendirme gerekli, `authorized`) | `GET /tools/mcp` |
| `tools mcp-resources <sunucu> [--read <uri>]` | Bir MCP sunucusunun kaynakları ve kaynak şablonları; `--read` bir kaynağın içeriğini yazdırır | `GET /tools/mcp/{server}/resources`, `/resource?uri=` |
| `tools mcp-authorize <sunucu>` | Uzak MCP sunucusunu OAuth ile yetkilendirir: tarayıcı `harness`'ın çalıştığı makinede açılır, onay beklenir, token anahtarlıkta saklanır (CLI istemcisi 3 dakikada zaman aşımına uğrar) | `POST /tools/mcp/{server}/authorize` |
| `tools mcp-logout <sunucu>` | Sunucunun OAuth oturumunu siler | `POST /tools/mcp/{server}/logout` |
| `tools run <ad> [--args '<json>'] [--workdir DİZİN] [--timeout SN]` | Bir aracı çalıştırır; çıktı aracın metnidir. `--workdir` yoksa geçerli dizin. Karar `ask` ise terminalde e/H sorusu; terminal yoksa `--yes` (`-y`) gerekir (çıkış 2, `cli.confirmation_required`). Politika `off` ise `--yes` ile bile `tools.disabled` (çıkış 1). Araç çalışıp hata verirse (yol dışı, eşleşmeyen düzenleme, zaman aşımı) çıkış 1, mesaj stderr'de; `--json` sonucu stdout'a da yazar | `POST /tools/{name}/run` (`RunToolResponse`) |

Aynı yüzey Control API'de: `GET /tools`, `POST /tools/{name}/run`, `GET /tools/policy`, `PUT /tools/policy`, `GET /tools/log`. Çalıştırma, politika değiştirme ve günlük `admin` kapsamı ister. API kullanıcıya soramadığı için politikanın "sor" dediği çağrıya `403 tools.approval_required` döner; soran istemci (CLI) kullanıcıdan evet aldıktan sonra gövdede `"approved": true` gönderir.

### MCP

| Komut | Ne yapar |
|---|---|
| `mcp [--scope KAPSAMLAR]` | Harness MCP sunucusu, stdin/stdout üzerinde (satır başına bir JSON-RPC mesajı). Claude Code, Codex, OpenCode, Cursor gibi istemciler bu komutu kendisi başlatır. Araçlar çalışan örneğin Control API'sinden geçer; örnek her çağrıda yeniden bulunur. Örnek yoksa araçlar `cli.no_instance` döner, `--headless` ile süreç ömrü boyunca geçici örnek açılır (stdin kapanınca sunucularıyla kapanır). `--scope` (varsayılan `read,servers:write,tools`) hangi araçların listelenip çağrılabileceğini belirler; `models:write` indirme ve silmeyi açar; `tools` yalnız Ayarlar > Araçlar'da "MCP İstemcilerine Aç" açıksa Harness'ın yerleşik araçlarını gösterir. stdout'a yalnız MCP mesajı yazılır. Araçlar ve protokol: [MCP.md](MCP.md). Geçersiz kapsam: çıkış 2 |

```sh
claude mcp add -s user harness -- harness mcp
codex mcp add harness -- harness mcp --scope read,servers:write,models:write
```

### Kodlama ajanını başlatma

| Komut | Ne yapar | `--json` |
|---|---|---|
| `launch <claude\|codex\|opencode\|aider\|goose> [--model M] [--prepare \| --no-prepare] [--direct] [--api-key K] [--timeout S] [--dry-run] [-- ARGÜMANLAR]` | Aracı Harness'e bağlı olarak başlatır; yalnız aracın süreç ortamı ve geçici dosyalar kullanılır, `~/.claude`, `~/.codex` gibi genel yapılandırmalar yazılmaz. `--model` yoksa terminalde tanım listesi sorulur (terminal yoksa çalışan tek model, o da yoksa 2). Bağlam aracın asgarisinin altındaysa ya da bilinmiyorsa "Sunucuyu Hazırla" sorulur: tanıma modelin desteklediği ve bu makinenin GPU'suna sığan en büyük bağlam (araç sınırları içinde), Flash Attention, `f16` KV (sığmazsa sırayla K `q8_0`, `q8_0`/`q8_0`, K `q8_0` + V `q4_0`; bkz. ARCHITECTURE "KV önbelleği hassasiyeti") ve 1 slot yazılır, çalışan örnekleri durdurulur; seçim, gerekçe ve alternatifler önce yazdırılır, GPU'ya tam sığmayan (asgari bağlam) durumda `--prepare` yetmez: kısmi CPU'ya taşma yalnız açık onayla/`--yes` ile kabul edilir, sığdıracak bir seçenek yoksa 5 (`cli.launch_context`); uzak örnekte donanım farklı olduğundan sabit öneri (65.536, q8_0) kullanılır; terminalsiz ortamda `--prepare` ya da `--no-prepare` gerekir (yoksa `cli.launch_context`, 2). Gateway çalışıyorsa managed endpoint ve gateway istemci token'ı kullanılır, model yüklü değilse tek token'lık ısıtma isteği gönderilir (OOM: 5); `--direct` ya da gateway yoksa model `/load` ile yüklenir. `--` sonrası argümanlar araca gider (Claude'da ör. `-p "..."`, Codex'te `exec ...`). Ctrl+C araca bırakılır; araç çıkınca geçici klasör silinir. Çıkış kodu aracınkidir; araç PATH'te yoksa 3 (`cli.launch_not_installed`). Ayrıntı: [INTEGRATIONS.md](INTEGRATIONS.md) | `--dry-run`: `{tool, model, definition_id, endpoint: {kind, base_url, token: "***"\|null}, context: {per_request, min, recommended, check}, prepare: {definition_load, plan}, load, program_path, plan: {program, args, env, env_remove, files, ...}}`; token değerleri `***` |

```sh
harness launch claude --model Qwen3-4B-Instruct-2507-Q4_K_M
harness launch codex -m qwen3-coder --prepare -- exec "README'yi özetle"
harness launch opencode --dry-run --json
```

## Örnekler

```sh
# Headless örnek, başka bir terminalden kullanım
harness serve
harness pull unsloth/Qwen3-4B-Instruct-2507-GGUF:Q4_K_M
harness run Qwen3-4B-Instruct-2507-Q4_K_M "Merhaba de."
harness ps --json | jq '.[].endpoint'
harness shutdown

# Örnek yokken tek seferlik
harness --headless run ./model.gguf "2+2?" --json

# Bellek yetmedi (çıkış 5): seçenekleri gör, önerileni dene
harness load qwen14b --json || harness retry qwen14b
harness retry qwen14b --option lower_context
harness retry qwen14b --set context_size=16384 --set cache_type_k=q8_0 --save

# Uzak örnek (isimli token)
HARNESS_HOST=192.168.1.20:11500 HARNESS_TOKEN=hs_... harness ps
```

## Testler

- Birim testleri: `cargo test -p harness-cli` (hata eşlemesi, GUI çevirisiyle metin eşitliği, servis birim/plist/görev içeriği, argüman ayrıştırma).
- Entegrasyon (`tests/cli.rs`): gerçek `harness` ikilisi geçici veri dizinleriyle çalıştırılır; `--json` şemaları ve çıkış kodları, geçici örneğin kilidi bırakması, ikinci sahibin reddi (4), dolu port (4), uzak token kapsamları, `serve` + `shutdown` yaşam döngüsü.
- `import modelfile` (`tests/cli.rs`): `--dry-run` hiçbir şey kaydetmez, gerçek içe aktarma tanımı ve preset'i oluşturur, alias çakışması ve `FILE` ile `--ollama` çakışması hata verir.
- `transcribe` / `asr` (`src/commands/asr.rs`): argüman ayrıştırma, çıktı biçimleri, `need()` = örnek gerekmez, anahtarlı hataların İngilizce metni. Gerçek tanıma `asr/e2e.rs` ve gateway uçtan uca testinde ([ARCHITECTURE.md](ARCHITECTURE.md)).
- `speak` / `tts` (`src/commands/tts.rs`): argüman ayrıştırma, stdin/argüman metni, çıktı hedefi ve biçim doğrulama, `need()` = örnek gerekmez, anahtarlı hataların İngilizce metni. Gerçek ses üretimi gateway uçtan uca testinde ([ARCHITECTURE.md](ARCHITECTURE.md)).
- MCP (`tests/mcp.rs`): gerçek `harness mcp` ikilisi satır satır JSON-RPC ile; el sıkışma, kapsamlar, araç hataları, modern/eski dönem. `#[ignore]` uçtan uca: `load_model` → `get_endpoint` → gerçek sohbet → `unload_model` → benchmark; gerçek `claude mcp list` ve `codex mcp get` ile yapılandırma denetimi (geçici `CLAUDE_CONFIG_DIR` / `CODEX_HOME`).
- `launch` (`tests/launch.rs`, `#[ignore]`, portlar 19120+): `serve --gateway` ile beş aracın `--dry-run` planı (token gizli), bağlantı testi (`/v1/messages`, `/v1/responses`, chat; araç çağrısı), `HS_TEST_LAUNCH_CLAUDE=1` ile gerçek Claude Code (geçici `CLAUDE_CONFIG_DIR`, `--prepare`, `-p`), `HS_TEST_LAUNCH_CODEX=1` ile gerçek Codex (geçici `CODEX_HOME`, `exec`). Kullanım hataları `tests/cli.rs`'de.
- Uçtan uca (`tests/e2e.rs`, `#[ignore]`): `HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<gguf> cargo test -p harness-cli --test e2e -- --ignored --test-threads=1`. `serve` → `run` (dosya yolundan tanım) → alias ile yeniden kullanım → `chat` (iki tur, geçmiş) → `logs` → `benchmark run` → `unload` → `load` → `shutdown` (portlar kapanır); `--headless run` sonrası hiçbir şey açık kalmaz. `HS_TEST_OOM=1` ile gerçek bir VRAM ayırma hatasında çıkış 5 ve `servers.out_of_memory` (aşama `kv_cache`) doğrulanır; `e2e_oom_recovery_retry` `harness retry --option lower_context` ile sunucunun gerçekten açıldığını ve `enable_fit` ile `-ngl`'siz başladığını, `e2e_crash_auto_restart` çöken llama-server'ın üstel beklemeyle yeniden başlatıldığını ve beklerken `stop`'un denemeyi iptal ettiğini doğrular. `e2e_serve_gateway_run_uses_managed_endpoint` `serve --gateway` hazır satırını, `status` gateway bilgisini ve `run`'ın gateway üzerinden gittiğini doğrular (portlar 18800+). `e2e_serve_gateway_request_log_and_analytics` `run`'ın gateway isteğinin `analytics.db`'ye yazıldığını ve `requests`, `requests <id>` ve `analytics` ile geri okunduğunu doğrular (portlar 19010+).
