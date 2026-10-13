# Harness Studio — Özellikler ve Rakip Karşılaştırması

[English](../en/FEATURES.md)

Tek doğruluk kaynağı: Harness Studio'da **bugün gerçekten ne var**, LM Studio ve Ollama'da ne var, hangi boşluğu nasıl kapatacağız. Plan ve öncelikler [ROADMAP.md](ROADMAP.md)'te, hedef mimari [ARCHITECTURE.md](ARCHITECTURE.md) içinde.

Son denetim: **Ekim 2026** (rakip sütunları resmi belgelere karşı yeniden doğrulandı, Harness sütunu `main` koduyla yeniden denetlendi; kalan eksikler [ROADMAP](ROADMAP.md)'te). Karşılaştırılan sürümler:
- Harness Studio `main` (0.1.0)
- LM Studio **0.4.26** (lmstudio.ai/download, 2026-10-12; 0.4.26 build 4)
- Ollama **0.40.3** (2026-10-11; 0.40.0 ile Apple Silicon'da MLX varsayılan)
- llama-server **0.5.0-dev build 11236** (`--help` denetimi bu build'e göre; §0 ölçümleri b11552/b11556)

Kaynaklar: repo kod denetimi, lmstudio.ai/docs ve /changelog, docs.ollama.com + github.com/ollama/ollama/releases, llama.cpp `tools/server/README.md` ve yerel `llama-server --help`.

`?` doğrulanamayan hücre demektir (resmi belgede ne var ne yok denmiyor); ayrıntı en alttaki [Dipnotlar](#dipnotlar)'da.

**Durum:** ✅ var · 🟡 kısmi · ❌ yok

**Ölçülen karşılaştırma:** bu tablo özellik varlığını gösterir. Aynı GGUF dosyalarıyla hız, araç çağrısı, bağlam, eşzamanlılık ve bellek ölçümleri aşağıda [§0](#0-ölçülen-karşılaştırma-2026-10-11) içinde, tamamı [COMPARISON.md](COMPARISON.md) belgesinde.

**Yol:**
- **L** — llama.cpp'nin bugünkü yetenekleriyle yapılabilir
- **M** — mimari değişiklik gerekir (çekirdek crate, gateway, kalıcı depo vb.)
- **X** — rakibe özgü veya Harness'a uygun değil

---

## 0. Ölçülen karşılaştırma (2026-10-11)

Windows 11, RX 7900 GRE 16 GB (Vulkan); Harness 0.1.0 (llama.cpp b11556), LM Studio (motor 2.51.0), Ollama 0.35.1, llama.cpp b11026/b11552. Modeller Qwen3-4B-Instruct-2507 ve Qwen2.5-Coder-14B-Instruct, Q4_K_M, her araçta aynı dosya. Her araç kendi varsayılanlarıyla ("kutudan çıktığı gibi"); yöntem ve ham veri [COMPARISON.md](COMPARISON.md), [benchmarks/2026-10-11](../benchmarks/2026-10-11/summary.md).

| Ölçüm | Harness | LM Studio | Ollama | llama.cpp |
|---|---|---|---|---|
| 4B üretim hızı (t/sn) | 140,9 | 142,0 | 128,9 | 114,5 |
| 14B üretim hızı (t/sn) | 48,6 | 42,0 (tekrar ölçüm 50,7) | 49,0 | 40,9 |
| 4B istem işleme, 4k token (t/sn) | 2460 | 2308 | **2997** | 1381 |
| 14B istem işleme, 4k token (t/sn) | 737 | 442 (tekrar ölçüm 822) | **1109** | 402 |
| 4 eşzamanlı istek, toplam (t/sn) | **344** | 157 | 126 | 152 |
| 14B araç çağrısı (38 vaka) | **36/38** | 5/38 | 4/38 | 5/38 |
| 4B araç çağrısı (38 vaka) | 38/38 | 38/38 | 38/38 | 38/38 |
| 20k token istem | doğru | bağlam hatası (8k) | bağlam hatası (4k) | doğru |
| Ajan testi, önbellekli 2./3. tur ilk token (sn) | **0,45 / 0,25** | bağlam hatası | bağlam hatası | 0,95 / 0,30 |
| 14B, 65k bağlam: paylaşılan belleğe taşma (MiB) | **98** | 12376 | 10326 | 5208 |
| 14B, 65k bağlam: üretim (t/sn, kısa istem) | 15,5 | **46,0** | 43,8 | 14,6 |
| Belleğe sığmayan açık ayar (14B, 32k f16) | gerekçeli 503 | yükledi, taştı | yükledi, taştı | yükledi, taştı |
| Tek uç noktada 4B → 14B geçişi, ilk token (sn) | 6,5 | yanlış model yanıtladı | **5,6** | yok (yeniden başlatma 22,9) |

Rakiplerin önde olduğu yerler: istem işleme (Ollama), sığmayan kartta 65k bağlamda kısa istem hızı (LM Studio/Ollama, paylaşılan belleğe taşarak), olgunluk ve ekosistem (aşağıdaki tablolar). Sınırlar: tek makine, yalnız AMD/Vulkan; 14B'de ölçümler arası değişim ~%20.

## 1. Model keşfi ve indirme

| Özellik | Harness | LM Studio | Ollama | Yol / not |
|---|---|---|---|---|
| HF arama, sıralama, görev/lisans/yazar filtresi | ✅ | ✅ | ❌ (kendi kataloğu) | — |
| Keşif ekranı (Trend, Yeni eklenenler, En çok beğenilen, En çok indirilen) ve aramasız, yalnızca filtreyle listeleme | ✅ | 🟡 (öne çıkanlar) | ✅ (kendi kataloğu) | İmleçli sayfalama, 5 dk oturum önbelleği, son seçimler hatırlanır |
| HF görevleri (`pipeline_tag`, 32 görev, HF gruplarıyla) ve görev başına "burada çalışır mı" işareti | ✅ | ❌ | ❌ | Çalışmayan görevlerin kartı "Burada çalışmaz" |
| Kuant / boyut filtresi, akıllı arama (`Q4_K_M` ayrıştırma) | ✅ | 🟡 | ❌ | — |
| Varyant tablosu + varyant başına donanım uyumu | ✅ | ✅ | ❌ | — |
| Yayıncı / model avatarı | ✅ | 🟡 | ✅ | **L** — HF `api/organizations/{x}/avatar` / `api/users/{x}/avatar`, harf+renk fallback |
| Kalıcı indirme kuyruğu, duraklat/sürdür/öne al, SHA-256 | ✅ | ✅ | ✅ | Harness'ta tek eşzamanlı iş |
| Gated model kontrolü, HF token (keychain) | ✅ | 🟡 | ❌ | Lisans adı, "Lisansı kabul et", "Tekrar Dene"; anahtarlı `hub.gated_*` hataları |
| "Hugging Face ile giriş" (OAuth PKCE, loopback / cihaz kodu) | ✅ | ✅ | ❌ | Kayıtlı OAuth uygulamasının client ID'si gerekir (SECURITY_MODEL.md); `harness hf login` |
| Özel URL indirme (HTTPS + SSRF koruması) | ✅ | ❌ | ❌ | — |
| Güncelleme kontrolü (LFS oid karşılaştırması) | ✅ | ❌ | ✅ (`pull`) | Hub kartında "güncelleme var" rozeti ve varyant çekmecesinde |
| Önerilen kuant / "donanımına uygun" rozeti | ✅ (kartta ve çekmecede) | 🟡 | ❌ | — |
| İndirme bitince yükle/çalıştır, kütüphane otomatik yenilensin | ✅ ("Yükle ve Sohbet Et", isteğe bağlı otomatik yükleme) | ✅ | ✅ | **L** |
| Model tanımı dosyası (model.yaml / Modelfile) | ✅ (`*.harness.toml`) | ✅ | ✅ | **M** — Harness Model Tanımı (bkz. §4) |

## 2. Yerel kütüphane (My Models)

| Özellik | Harness | LM Studio | Ollama | Yol / not |
|---|---|---|---|---|
| Ayrı kütüphane ekranı | ✅ (Kütüphanem) | ✅ | CLI `ls` | **L** |
| Türe göre ayırma: LLM / Embedding / VLM / Draft / LoRA / mmproj / Reranker | ✅ (sekmeler; tür GGUF'tan, VLM projektör eşleşmesinden, Draft tokenizer uyumundan) | 🟡 (LLM/Embedding) | ❌ | **L** — GGUF `general.type`, `pooling_type`, `adapter.type`, `clip.*` anahtarları; reranker ayrıca `cls.output.weight` tensöründen (bge-reranker-v2-m3 `pooling_type` taşımıyor) |
| Bilgi: kuant, mimari, parametre, bağlam, boyut, yol | ✅ | ✅ | ✅ (`show`) | Parametre sayısı tensör tablosundan (tahminse ≈); yetenekler şablon ve projektörden |
| Yüklü/yüklenmemiş durumu, son kullanım, kullanılan preset | ✅ | ✅ | ✅ (`ps`) | **L** — `state.db` `model_state` |
| Yükle / boşalt / yeniden yükle | ✅ (Kütüphanem, Playground seçicisi) | ✅ | ✅ | **L** |
| Klasörde göster, taşı, sil | ✅ (silmede önizleme; kullanımdayken engelli) | ✅ | 🟡 (`rm`) | **L** |
| Alias (aynı dosya, farklı ayarlar) | ✅ (Model Tanımı) | 🟡 (`--identifier`) | ✅ (`cp`/`create`) | **M** — Model Tanımı |
| Model başına varsayılan yükleme ayarı | ✅ (tanım başına, "Varsayılan Olarak Kaydet") | ✅ | ✅ (Modelfile) | **L** |
| Ollama / LM Studio'dan içe aktarma (link/kopya) | ✅ | ✅ (`lms import`) | ❌ | — |
| Çoklu model klasörü, güvenli taşıma | ✅ | 🟡 | 🟡 (`OLLAMA_MODELS`) | — |
| Eksik shard tespiti ve onarımı | ✅ | ? | ❌ | HF klasörü, kayıtlı `resolve` adresi veya GGUF kaynak üst verisi; bilinmiyorsa bir parça adresi |
| Kopya model tespiti (hash) | ✅ (ön özet + tam SHA-256) | ❌ | ✅ (digest) | **L** |
| Favoriler, son kullanılanlar, son indirilenler | ✅ | 🟡 | ❌ | **L** |

## 3. Yükleme (load) yapılandırması

llama-server build 11236'nın `--help` çıktısına göre:

| Ayar | Harness | LM Studio | Ollama | Yol / not |
|---|---|---|---|---|
| Bağlam, GPU katmanı, main GPU, tensor split, split mode | ✅ | ✅ | 🟡 (env/options) | `-ngl auto/all` desteklenmiyor |
| Flash Attention, K/V kuant | ✅ (f16/q8_0/q4_0) | ✅ | 🟡 (global env) | Runtime 9 tip destekliyor (bf16, iq4_nl, q5_x…) |
| KV offload (`-kvo`), unified KV (`-kvu`), `--swa-full` | ✅ | ✅ | ❌ | **L** |
| Batch / ubatch / threads / paralel slot | ✅ | ✅ | 🟡 | — |
| Continuous batching, cache-reuse, `--cache-ram` | ✅ (`--cache-ram` ayrı alan, Bellek bölümü, varsayılan 1024; birleşik KV havuzunda çok slot varken otomatik `--cache-ram 0`, b11552 takılmasına karşı) | 🟡 (continuous batching 0.4.0'dan beri) | ❌ | **L** |
| RoPE / YaRN | ✅ (eğitim bağlamı aşılınca YaRN ipucu) | ✅ | ❌ | **L** |
| Context shift | ✅ | 🟡 (overflow policy) | ✅ | — |
| Jinja / chat template / template kwargs | ✅ (şablon düzenleyici, `--chat-template-file`, GGUF şablonunu görüntüleme) | ✅ | ✅ (TEMPLATE) | **L** |
| Reasoning format / budget / effort | ✅ (katalog) | ✅ | ✅ (`think`) | **L** |
| Draft model + ngl | ✅ | ✅ | 🟡 | Taslak model seçilince yeni sürümlerde `--spec-type draft-simple` otomatik eklenir (b11236 onsuz taslak üretmiyor); `--spec-type`, n-max/min katalogda |
| Yerleşik MTP / DFlash / DSpark spekülatif kodlama | 🟡 (`--spec-type` değerleri `--help`'ten okunur, runtime destekliyorsa seçilir; ayrı arayüz ve ölçüm yok) | ✅ (MTP 0.4.14, DFlash/DSpark taslakçıları 0.4.22) | ? | Kaynak: lmstudio.ai/changelog |
| Uyumlu draft model önerisi | ✅ (Dashboard ve tanım düzenleyicide "Önerilen") | 🟡 | ❌ | Aynı tokenizer (`tokenizer.ggml.model`, pre, BOS/EOS, vocab ±128, token metni özeti) ve ≤%40 boyut |
| LoRA | ✅ (çoklu adaptör, çalışan sunucuda canlı ölçek) | ✅ | 🟡 (ADAPTER) | `harness lora list|set` |
| mmproj (vision/ses/video) | ✅ (yol alanı + Dashboard/tanımda otomatik öneri) | ✅ | ✅ | — |
| MoE: `--n-cpu-moe`, `--cpu-moe`, `-ot` | ✅ | ✅ | ❌ | **L** |
| mmap / mlock / direct-IO (`--load-mode`) | ✅ (`--load-mode`, eski runtime'da `--no-mmap/--mlock`) | ✅ | 🟡 | **L** — eski `--no-mmap/--mlock` kaldırıldı |
| Embedding / rerank modu, pooling | ✅ (model türünden otomatik `--embeddings` / `--rerank`; "Sunucu modu" ile değiştirilebilir; pooling GGUF'tan) | ✅ | ✅ | Embedding/rerank koşusunda `-b`/`-ub` eğitim bağlamına çıkar, en az 2048 (kullanıcı ubatch vermediyse) |
| Belleğe göre otomatik ctx/ngl/KV | ✅ (kendi sığdırma merdiveni: f16 → q8_0 KV → bağlam 16k'ya kadar → daha az katman; paylaşılan belleğe taşmayı önler, AMD/Windows sürücü payı ≥2200 MiB; açık kullanıcı ayarı sığmazsa gerekçeli 503) | ✅ (autoFit) | ✅ | Kutudan çıkış ölçümü §0 |
| Varsayılan bağlam | ✅ (otomatik: GGUF eğitim bağlamı, en çok 32k) | 🟡 (8k) | 🟡 (4k) | 20k istem/ajan testi §0 |
| Tahmini bellek gereksinimi | ✅ (Rust, SWA/mmproj/KV türü; yüklemeden sonra ölçümle düzeltilir) | ✅ (estimate-only) | ✅ (ölçüm + fit) | — |
| Runtime yeteneğine göre arayüz | ✅ (yetenek kaydı: `--help` çıktısından bayrak/değer; parametre başına `feature`, desteklenmeyen alan rozetli/gizli) | ✅ | — | ARCHITECTURE §13.2 |
| Serbest ek parametre, korumalı bayrak listesi | ✅ | ✅ | ❌ | — |

## 4. Preset, Model Tanımı, alias

| Özellik | Harness | LM Studio | Ollama | Yol / not |
|---|---|---|---|---|
| Model başına yükleme preset'i | ✅ (model başına varsayılan tanım, "Varsayılan Olarak Kaydet"; ayrıca isimli tanımlar) | ✅ | ✅ | — |
| İsimli sunucu profili (model + tüm ayarlar) | ✅ | ❌ | ❌ | Model Tanımı'nın çekirdeği olacak |
| Çıkarım preset'i (system prompt + örnekleme) | ✅ | ✅ | ✅ (PARAMETER/SYSTEM) | **L** |
| Hazır preset'ler (Coding, Reasoning, Creative, Deterministic) | ✅ | 🟡 | ❌ | **L** |
| Preset içe/dışa aktarma | ✅ (JSON) | ✅ | ✅ (Modelfile) | **L** |
| Tek yeniden kullanılabilir tanım (base + template + defaults + adapter + alias) | ✅ (`*.harness.toml`) | ✅ (model.yaml) | ✅ (Modelfile) | **M** — "Harness Model Tanımı" (TOML) |
| Ollama Modelfile içe aktarma | ✅ (FROM GGUF/Ollama model adı, PARAMETER, SYSTEM, ADAPTER; TEMPLATE ve MESSAGE → uyarı; önizleme, `harness import modelfile`) | ❌ | — | Go TEMPLATE yerine GGUF'un Jinja şablonu kullanılır |

## 5. Çıkarım / örnekleme (Playground ve API)

| Parametre | Harness Playground | llama-server | LM Studio | Ollama |
|---|---|---|---|---|
| temperature, top_p, top_k, min_p, repeat_penalty, seed, max_tokens | ✅ | ✅ | ✅ | ✅ |
| typical_p, presence/frequency penalty | ✅ | ✅ | ✅ | ✅ |
| stop dizileri | ✅ | ✅ | ✅ | ✅ |
| DRY, XTC, top-n-sigma, mirostat, dynatemp, adaptive-p | ✅ | ✅ | 🟡 (XTC) | ❌ |
| sampler zinciri sırası (`samplers`) | ✅ | ✅ | ❌ | ❌ |
| GBNF grammar, JSON Schema / `response_format` | ✅ | ✅ | ✅ | ✅ (`format`) |
| logit_bias, logprobs | ✅ (Playground'da token olasılığı görünümü) | ✅ | ✅ | 🟡 |
| reasoning açık/kapalı/effort/budget | ✅ | ✅ | ✅ | ✅ |
| tool_choice | ✅ | ✅ | ✅ | ❌ |

## 6. Playground

| Özellik | Harness | LM Studio | Ollama app | Yol / not |
|---|---|---|---|---|
| Akışlı sohbet, düşünme bloğu, istatistik (t/s, TTFT, token, cache) | ✅ | ✅ | 🟡 | — |
| Composer token sayımı ve konuşma tüketimi | ✅ (sistem, araçlar, geçmiş, taslak; yerel chat template + tokenize ile kesin, diğerlerinde ≈; yanıt rezerviyle bağlam uyarısı; mesaj/konuşma giriş-çıkış toplamı, TR/EN) | — | — | — |
| Tool calling döngüsü + onay, MCP araçları | ✅ | ✅ | ✅ | — |
| MCP istemcisi: stdio, Streamable HTTP, eski HTTP+SSE, OAuth (RFC 9728/8414/7591, PKCE), kaynaklar (listele/oku/sohbete ekle) | ✅ (dinamik istemci kaydı olmayan yetkilendirme sunucuları yok) | 🟡 | 🟡 | — |
| Çoklu sohbet: liste, arama, yeniden adlandır, sil, kopyala, sabitle, klasör | ✅ (`state.db`; sohbeti klasöre sürükle-bırak ya da menüden "Klasöre Taşı") | ✅ | 🟡 | Sürükleme işaretçi olaylarıyla; Tauri'nin dosya bırakma işleyicisiyle çakışmaz |
| Dışa/içe aktarma, otomatik başlık, son sohbeti aç | ✅ | ✅ | 🟡 | **L** |
| Herhangi bir mesajı düzenle, yeniden üret, buradan çatalla, tekrar dene | ✅ | ✅ | 🟡 | **L** |
| Tam markdown (liste, tablo, başlık, mermaid) | ✅ (```mermaid blokları diyagram: Kod/Diyagram, kaynağı kopyala, SVG kaydet; temayı izler) | ✅ | ✅ | Mermaid yalnız ilk diyagramda yüklenir (dinamik import) |
| Bağlam doluluğu göstergesi (18.420 / 32.768) | ✅ | 🟡 | ❌ | **L** — `timings` + `/props n_ctx` |
| Playground'dan model yükle/başlat/durdur, yükleme ayarı seçme | ✅ (seçicide "Yükleme Ayarları", tek seferlik ya da tanıma kaydet) | ✅ | ✅ | **L** |
| Yan yana karşılaştırma (split view) | ✅ (iki model ya da iki preset, paralel akış, TTFT/t/s/token, benzerlik ve kelime farkı, sığmazsa otomatik sıralı, "Bu yanıtı sohbete al") | ✅ | ❌ | Split turu varyant olarak `state.db`'de; JSON/Markdown dışa aktarmada iki taraf |
| Görsel ekleme (vision) | ✅ (düğme, yapıştır, sürükle-bırak; yalnız vision modelinde) | ✅ | ✅ | **L** — `image_url` / base64 |
| Metin/kod/PDF ekleme, uzun belgede RAG | ✅ (PDF sayfa sayfa, DOCX, Markdown, metin, kod; sürükle-bırak; sığan belge tam metin, sığmayan hibrit arama: embedding + BM25, isteğe bağlı reranker; yanıt altında numaralı alıntılar ve sayfa) | ✅ | 🟡 | Tamamen yerel; embedding/rerank gateway üzerinden tanım alias'ıyla (JIT). Embedding modeli yoksa anahtar kelime araması. Bölünmüş görünümde kullanılmaz |
| Preset seçimi | ✅ | ✅ | ❌ | **L** |
| Yerleşik web araması ve hava durumu | ✅ (`web_search`, `weather`, `geocode`) | ? | 🟡 (ollama.com web search/fetch API'si, bulut, ücretsiz hesap + API anahtarı) | DuckDuckGo HTML anahtarsız varsayılan (yeni/eksik ayarlar); SearXNG, Brave, Tavily seçilebilir. Konum: Open-Meteo + sonuç yoksa Nominatim; tahmin: Open-Meteo. Kayıtlı `none` korunur; araç izin politikası geçerli |
| Bağlam sıkıştırma (eski turları özetle, son turlar aynen) | ✅ (otomatik: bağlamın %'si ya da sabit token eşiği, varsayılan %80; elle "Şimdi Sıkıştır" ve `/compact [talimat]`; ayraçta özet, geri al / yeniden uygula; global ya da sohbete özel ayar, özet modeli seçimi, araç sonucu kısaltma) | 🟡 (taşmada kırpma: ortadan kes / kayan pencere) | ❌ | Özgün mesajlar silinmez (işaretleyici düğüm); token sayımı llama-server `/tokenize` |
| Embedding testi (çoklu metin, boyut, süre, kosinüs benzerlik matrisi) ve Rerank testi (sorgu + belgeler, puanlı sıralama) | ✅ | ❌ | ❌ | Seçili sunucuya gerçek `/v1/embeddings` ve `/v1/rerank` isteği; embedding/rerank sunucusunda sohbet sekmesi uyarır |
| Yanıt başına spekülatif kabul oranı | ✅ (`timings.draft_n_accepted / draft_n`) | 🟡 | ❌ | — |

## 7. Sunucu orkestrasyonu

| Özellik | Harness | LM Studio | Ollama | Yol / not |
|---|---|---|---|---|
| Çoklu eşzamanlı sunucu, port güvenliği (otomatik port 11440–11499, portu tutan program adıyla, gateway/API yedek portu), profiller, otomatik başlatma | ✅ | 🟡 | 🟡 | Harness'ın güçlü yanı |
| Router modu (`--models-dir`) | ✅ (model durumu, yükle/boşalt, model başına ön ayar, gateway yönlendirmesi; `harness router`) | — | — | **L** |
| JIT yükleme (istek gelince yükle) | ✅ (gateway; tanımı sunan çalışan sunucu varsa ona yönlendirir) | ✅ | ✅ | Yalnız gateway'den gelen isteklerde |
| Boşta TTL ile boşaltma | ✅ (tanım başına, genel varsayılan; varsayılan kapalı) | ✅ (60 dk) | ✅ (5 dk) | Yalnız gateway'in yüklediği sunucular |
| Bellek farkındalıklı tahliye, en fazla yüklü model | ✅ (boş VRAM + ölçülen süreç VRAM'i/tahmin, LRU, sabitlenmiş modeller) | ✅ | ✅ | — |
| Gateway yüklemelerinde otomatik paralel slot | ✅ (bağlam/8k istek boyuna göre; 4B 32k'da 4 slot) | ✅ (4) | 🟡 (1, env ile) | 4 akış ölçümü §0 |
| İstek kuyruğu, backpressure (503), iptal, zaman aşımı | ✅ (slot kadar eşzamanlılık, sınırlı kuyruk, 503 + `Retry-After`, kopmada yukarı akış iptali) | 🟡 | ✅ | Doğrudan portlarda llama-server'ın kendi slotları |
| Çökmede otomatik yeniden başlatma / geri çekilme | ✅ (isteğe bağlı, en çok 3 deneme, 2→60 sn üstel bekleme, arayüzde geri sayım) | ? | ✅ | Ayar hataları yeniden başlatılmaz |
| OOM kurtarma (ngl/ctx/ubatch düşür, başkasını boşalt) | ✅ (ngl/ctx/ubatch, KV kuantı, `--fit`, MoE→CPU, boşalt; tek tıkla gerçek yeniden başlatma) | 🟡 (guardrail) | ✅ (tahliye + 1 tekrar) | Gateway JIT yüklemesinde otomatik tahliye + 1 tekrar |
| Cihazlar arası uzak model erişimi (LM Link: uçtan uca şifreli, Tailscale ortaklığı; `lms link`) | ❌ (LAN açma + token ile yerel çözüm, §11) | ✅ | ❌ | Planlı: Harness Link (uzak Harness kaynağı, HTTPS + token, hesapsız ve VPN'siz); bkz. ROADMAP |
| Yükleme zaman aşımı ayarı | ✅ (ayar + model/profil başına) | ? | ✅ | — |
| Hata sınıflandırma (OOM, eksik DLL, bozuk GGUF, mimari, port) ve `harness doctor` | ✅ (sabit kodlar, TR/EN mesaj, Ayarlar > Tanılama) | 🟡 | 🟡 | — |

## 8. API uyumluluğu ve Harness API

| Özellik | Harness | LM Studio | Ollama | Yol / not |
|---|---|---|---|---|
| OpenAI `/v1/models`, chat, completions, embeddings | ✅ (llama-server doğrudan) | ✅ | ✅ | — |
| `/v1/responses` | ✅ (llama-server, chat'e çevirerek; gateway `namespace` araçlarını düzleştirir; gateway'de durumlu: `store`, `previous_response_id`) | ✅ | 🟡 (stateless) | Desteklenmeyen araç tipleri gateway'de açık 400 (bkz. §10) |
| Anthropic `/v1/messages` + `count_tokens` | ✅ (llama-server) | ✅ (`/v1/messages`; `count_tokens` ?) | 🟡 (`count_tokens`, `tool_choice`, prompt caching yok) | — |
| Konuşmadan yazıya `/v1/audio/transcriptions` ve `/translations` (whisper.cpp) | ✅ (sherpa-onnx ONNX ASR da: Parakeet TDT v3, SenseVoice, Omnilingual (Türkçe dahil), Silero VAD ile zaman damgalı bölümler; `harness transcribe`, `harness asr`; whisper-server ikinci runtime; JIT + TTL, VRAM bütçesine sayılır; WAV/MP3/FLAC/Ogg doğrudan, M4A/WebM/Opus ffmpeg ile; json/text/srt/vtt/verbose_json; Playground "Konuşma" sekmesi) | ❌¹ | ❌¹ | Resmi whisper.cpp sürümlerinde Vulkan/ROCm yok: AMD'de CPU build; CUDA build NVIDIA'da doğrulanmadı |
| Metinden sese `/v1/audio/speech` | ✅ (yönetilen sherpa-onnx, CPU; Kokoro çok dilli, Türkçe Piper sesleri fettah/fahrettin; wav/pcm, ffmpeg ile mp3/opus/aac/flac; Playground "Sesli Oku" ve Konuşma sekmesi; `harness speak`) | ❌¹ | ❌ | İstek başına süreç (Kokoro ~1,5 sn yükleme) |
| Hugging Face ONNX encoder'ları (embedding, rerank; Python'suz) | ✅ (`harness-onnx` yan süreci: ONNX Runtime + tokenizers, DirectML/CUDA/CPU; pooling ve normalizasyon sentence-transformers dosyalarından; ONNX Runtime ilk kullanımda SHA-256 doğrulamalı indirilir; all-MiniLM, bge, ms-marco cross-encoder) | 🟡 | ❌ | Yalnız safetensors içeren LLM depoları aşağıdaki dönüştürmeyle çalışır |
| ONNX sohbet modelleri (onnxruntime-genai) | ✅ (`harness-onnx` içinde `/v1/chat/completions`, akış; DirectML/CPU; Hub'da DirectML/CPU/CUDA varyantı; Phi-3-mini int4 DirectML ~100–130 t/sn) | ❌ | ❌ | Araç çağrısı yok; Linux/macOS'ta GenAI kitaplığı yok; genel dışa aktarımlar DirectML'de bozuk çıktı verebilir |
| Safetensors → GGUF dönüştürme | ✅ (Hub'da "GGUF'a Dönüştür ve İndir", `harness convert`; yönetilen uv + Python yalnız dönüştürmede, llama.cpp resmî dönüştürücüsü + `llama-quantize`; Q4_K_M/Q5_K_M/Q6_K/Q8_0/F16/BF16) | ❌ | 🟡 (`ollama create` safetensors içe aktarır) | Yalnız Windows'ta denendi; ortam ~700 MB |
| Lemonade motoru (Ryzen AI NPU/FastFlowLM, ROCm/Vulkan llama.cpp) | ✅ (isteğe bağlı `lemond`, SHA-256'lı kurulum; ağ geçidinden sohbet/embedding/rerank/yazıya dökme; Ayarlar kartı, `harness lemonade`) | ❌ | ❌ | NPU yolu donanımsız denenmedi; FastFlowLM NPU ikililerinin lisansı çelişkili, paketlenmez |
| mistral.rs motoru (safetensors LLM/VLM, dönüştürmesiz) | ✅ (isteğe bağlı; Windows CUDA yerel derleme; ağ geçidinden sohbet, akış, `/v1/responses`, Ollama `/api/chat`) | ❌ | 🟡 (Ollama safetensors'u içe aktarır) | Yalnız NVIDIA'da denendi; AMD Windows yolu yok; GUI kartı kurulu yapıda doğrulanmadı |
| Görev hatları `/v1/hf/{model}` (Hugging Face Inference API biçimi) | ✅ (ONNX: metin sınıflandırma, sıfır örnekli sınıflandırma, varlık tanıma, boşluk doldurma, soru cevaplama, görüntü sınıflandırma, nesne algılama, anlamsal bölütleme, derinlik tahmini; Playground "Görevler" sekmesi) | ❌ | ❌ | Python yok; bkz. ARCHITECTURE §14.4 |
| Rerank `/v1/rerank` | ✅ (reranker modeli `--rerank` ile başlar; Playground "Rerank" sekmesi; başlatma sonrası gerçek `/rerank` doğrulaması) | ❌ | ❌ | — |
| Tek uç noktada çok model (modele göre yönlendirme) | ✅ (gateway, `127.0.0.1:11435`; OpenAI, Responses, Anthropic, embeddings, rerank) | ✅ | ✅ | Doğrudan sunucu portları da çalışır |
| Metin olarak yazılan araç çağrılarını (`<tools>`, `<tool_call>`, düz JSON) gerçek `tool_calls`'a çevirme | ✅ (gateway, varsayılan açık, model başına kapatılır) | ❌ | ❌ | 14B araç testi 36/38 vs 4–5/38, §0 |
| Ollama native `/api/*` uyumluluğu | ✅ (P2, gateway: `/api/chat`, `/api/generate`, `/api/embed`, `/api/embeddings`, `/api/tags`, `/api/ps`, `/api/show`, `/api/version`; NDJSON akış, ns süreler, araç çağrısı, görseller, `format`, `think`, `keep_alive` → TTL; gerçek `ollama` CLI ile doğrulandı) | ❌ | — | `/api/create`, `/api/copy`, `/api/delete`, `/api/blobs` var; `suffix`/`context` ve `/api/push` açık hata |
| Ollama portunda (11434) dinleme | ✅ (isteğe bağlı, varsayılan kapalı; Ollama çalışıyor ya da kuruluysa port alınmaz, uyarı) | ❌ | ✅ | — |
| Ollama `/api/pull` | 🟡 (yalnız `hf.co/<sahip>/<depo>:<kuant>`, ayarla açılır; kayıt defteri adları açık hata) | ❌ | ✅ | Ollama kayıt defteri kullanılmaz |
| Yönetim API'si (listele, indir, yükle, boşalt, sunucu, metrik, log, benchmark) | ✅ (Control API, SSE olaylar) | ✅ (REST v1) | ✅ | — |
| OpenAPI şeması | ✅ (`openapi.json`, utoipa) | ❌ | ✅ | — |
| Karar modelleri `/v1/systemone` (Ollama) / `/v1/decisions` (LM Studio, Jev uyumlu) | ❌ | ✅ (0.4.26) | ✅ (0.35.0) | Evet/hayır olasılığı, seçim, puan soruları; planlı (GBNF + logprobs); bkz. ROADMAP |
| Görsel üretimi (`/v1/images/generations`, görsel düzenleme `/v1/images/edits`) | ✅ (stable-diffusion.cpp `sd-server`; SD1.5/SDXL/SD-Turbo/SD3.5/FLUX paketleri; Playground Görsel sekmesi, `harness image`) | ? | ❌ (kaldırıldı) | — |
| Durumlu sohbet (`previous_response_id`) | ✅ (P2, gateway: `store` varsayılan açık, `GET`/`DELETE /v1/responses/{id}`, `input_items`, `background: true` + `POST .../cancel`, `context_management` ile zincir sıkıştırma; `state.db`'de token'a ait, saklama süresi/sayı/boyut sınırlı; Ayarlar > Gateway'den kapatılır) | ✅ | ❌ | Doğrudan llama-server portunda yok (llama.cpp stateless) |

## 9. CLI, headless, ajan kontrol düzlemi

| Özellik | Harness | LM Studio | Ollama | Yol / not |
|---|---|---|---|---|
| CLI (`ls/ps/load/unload/pull/run/chat/serve/logs`) | ✅ (`harness`; [CLI.md](CLI.md)) | ✅ (`lms`) | ✅ | Çalışan örneğin Control API'si, yoksa komut süresince geçici örnek |
| `--json` çıktı, tutarlı çıkış kodları | ✅ (her komutta `--json`, sabit 0-6 kodları, OOM = 5) | 🟡 | ❌ (yalnız API) | — |
| Headless / servis modu | ✅ (`harness serve`; systemd `--user`, launchd, Görev Zamanlayıcı) | ✅ (llmster) | ✅ (`serve`) | systemd/launchd gerçek kurulumu yalnız içerik testli |
| Uzak instance (`--host`) | ✅ (`HARNESS_HOST` + `HARNESS_TOKEN`, isimli token kapsamları) | ✅ | ✅ (`OLLAMA_HOST`) | — |
| Kendi MCP sunucusu (ajanlar Harness'ı araç olarak kullanır) | ✅ (`harness mcp` stdio + Control API `/mcp`, 16 araç, token kapsamları; [MCP.md](MCP.md)) | ❌ | ❌ | Farklılaştırıcı |
| `launch <araç>` (araçla doğru ortamla başlat) | ✅ (`harness launch`) | ❌ | ✅ | **L** |
| Resmi SDK'lar (JS / Python) ve eklenti geliştirme (`lms dev`, Hub'a `lms push`/`clone`) | ❌ (OpenAPI şeması + MCP; SDK yok) | ✅ | ✅ (Python / JS istemci kitaplıkları) | Control API SDK'ları planlı (bkz. ROADMAP); eklenti çalışma zamanı ve Hub yayınlama bilerek kapsam dışı (§14) |
| Shell completion | ✅ (`harness completion <kabuk>`) | ? | ❌ | — |

## 10. Entegrasyonlar

| Araç | Harness bugün | Not |
|---|---|---|
| Claude Code | ✅ `harness launch claude` / "Başlat" (env + geçici `--settings`); kalıcı: `~/.claude/settings.json` `env`; Harness MCP ekle/kaldır | Anthropic taban adresi `/v1`'siz |
| Codex CLI / IDE | ✅ `launch` (`-c` sağlayıcı geçersiz kılmaları); kalıcı: `[model_providers.harness]` + `harness.config.toml` profili (`codex --profile harness`); Harness MCP | Responses API; `web_search` kapalı (gateway sessizce düşürmez, 400 verir) |
| OpenCode | ✅ `launch` (`OPENCODE_CONFIG_CONTENT`; PATH'te yoksa masaüstü sürümünün `opencode-cli`'ı); masaüstü uygulaması algılanır, "Yapılandır ve Masaüstünde Aç"; kalıcı uygula/geri al (bütün gateway sohbet modelleri listelenir, genel `model` yalnız istenirse); model başına araç çağrısı durumu (gerçek deneme / ağ geçidi kaydı / şablon) ve ilk yanıt tahmini; Harness MCP | Masaüstü yolları macOS/Linux'ta varsayım |
| Aider | ✅ `launch` (parametre + env + geçici model metadata); komut snippet'i | — |
| Goose | ✅ `launch` (env); kalıcı: `custom_providers/harness.json` | — |
| VS Code Copilot BYOK | ✅ `chatLanguageModels.json` (`customendpoint`) | Yol doğrulanmadı |
| Zed | ✅ `language_models.openai_compatible` | — |
| Kilo Code | ✅ `kilo.json` `provider.harness` | Windows yolu doğrulanmadı |
| LibreChat | ✅ `librechat.yaml` `endpoints.custom` (yol kullanıcıdan) | YAML yorumları korunamaz |
| Open Design, Continue | ✅ "Sunucuyu Hazırla": modelin desteklediği ve GPU'ya sığan en büyük bağlam (Open Design ≥128k), KV `q8_0`→`q4_0`, gerekçe + alternatifler önceden gösterilir | Bellek tahmini bütçenin %90'ı ile sınırlı |
| Claude Desktop (3P gateway), Cline, Roo Code, JetBrains AI, Jan, Open WebUI, Obsidian Copilot | ✅ adım adım talimat + kopyalanabilir alanlar | Roo Code kapandı (2026-05) |
| Cursor | ✅ talimat (localhost'a ulaşamadığı uyarısıyla); Harness MCP ekle/kaldır | — |
| OpenAI / Anthropic SDK, LangChain, LlamaIndex, Vercel AI SDK, cURL | ✅ snippet (gerçek uç nokta + alias) | — |

Her kalıcı entegrasyonda: önizleme (renkli fark), "Bağlantıyı Test Et" (aracın API'sinde araçlı
gerçek istek), anahtar bazlı geri alma (yalnız Harness'in yazdığı ve değişmemiş anahtarlar),
zaman damgalı 5 kuşak yedek, dosya elle değiştiyse hash uyarısı. Başvuru:
[INTEGRATIONS.md](INTEGRATIONS.md).

## 11. Güvenlik

| Özellik | Harness | LM Studio | Ollama | Yol / not |
|---|---|---|---|---|
| API anahtarı keychain'de, komut satırında görünmez | ✅ | 🟡 | ❌ | — |
| Korumalı bayraklar (`--host/--port/--api-key/--tools…`) | ✅ | ❌ | — | — |
| Varsayılan yalnız localhost | ✅ | ✅ | ✅ | — |
| LAN açma arayüzü | ✅ (Ayarlar > Güvenlik) | ✅ | 🟡 (env) | **L** |
| Anahtarsız LAN uyarısı/engeli | ✅ (anahtarsız ağ bağlama: araçlar/MCP açıkken her zaman reddedilir, aksi halde kullanıcı "yine de aç" onaylamadıkça reddedilir) | 🟡 | ❌ (belgelerde yerleşik kimlik doğrulama yok; ters vekil önerilir) | `network.rs` |
| İsimli token, yetki kapsamı, oluştur/düzenle/sil | ✅ (Control API; `inference` kapsamı gateway'e erişim verir; dar `servers:control` / `downloads:write`; token başına izinli model deseni ve uç nokta grubu) | ✅ | ❌ | — |
| CORS / izinli origin | ✅ | ✅ | ✅ | **L** (`--cors-origins`) |
| Rate limit, istek zaman aşımı | ✅ (token başına dakikada istek / token ve eşzamanlılık; 429 + `Retry-After`, OpenAI/Anthropic/Ollama biçimi; istek zaman aşımı ve kuyruk sınırı; sayaçlar `analytics.db`'de, yeniden başlatmada korunur ve aynı veri klasörünü paylaşan örnekler arasında ortak) | ❌ | 🟡 | Eşzamanlılık sınırı süreç içi |

## 12. Gözlemlenebilirlik

| Özellik | Harness | LM Studio | Ollama | Yol / not |
|---|---|---|---|---|
| Canlı t/s, prompt t/s, slot, KV, kuyruk, spekülatif kabul oranı, süreç CPU/RSS, GPU yük/ısı | ✅ | 🟡 | ❌ | Harness'ın en güçlü yanı; GPU yük/ısı yalnız NVIDIA |
| Log arama (regex), çalıştırma geçmişi, temizlenmiş dışa aktarma | ✅ | 🟡 | 🟡 | — |
| llama-bench tarama, geçmiş, karşılaştırma | ✅ | ❌ | ❌ | — |
| İstek başına metadata (model, istemci, uç nokta, token, TTFT, süre, kuyruk, finish, cache) | ✅ (gateway trafiği, `analytics.db`; bağlam doluluğu, spekülatif kabul, VRAM/RAM tepe, backend, hata/OOM da; doğrudan porttaki trafik görülemez) | ❌ | ❌ | İçerik varsayılan kaydedilmez; tanım başına, süreli açılır |
| Tarihsel analiz: model/istemci bazında P50/P95, hata ve OOM oranı | ✅ (ham satır 30 gün, günlük toplamlar kalıcı; Analitik ekranı, `harness analytics`, `/analytics/summary`) | ❌ | ❌ | — |
| İstemci bazlı giriş/çıkış/toplam token ve payı | ✅ (Analitik + Performans > İstemciler; gateway ve etiketli doğrudan Playground/üretici kullanımı, çift sayım koruması, toplam KPI, TR/EN adlar) | — | — | Genel doğrudan trafik yalnız metrics toplamı |
| İstek inceleyici (trace) | ✅ (filtreli liste, kuyruk → yükleme → istem → üretim zaman çizelgesi, hata ayrıntısı, araç çağrısı teşhisi: suçlu sunucu/şablon, bağlam, model ya da istemci; `harness requests`) | 🟡 (`log stream`) | 🟡 (debug) | — |
| Prometheus dışa aktarma | ✅ (birleşik `/metrics`: gateway sayaçları ve model başına histogramlar, zamanlayıcı, sistem belleği, her llama-server'ın `llamacpp:*` değerleri; gateway'de ve Control API'de) | ❌ | ❌ | — |
| OpenTelemetry | ✅ (OTLP HTTP/protobuf veya gRPC; istek başına span ve kuyruk/yükleme/üretim alt span'leri, isteğe bağlı korelasyonlu log, gen_ai metrikleri, W3C traceparent/tracestate; içerik yalnız açıkça; "Deneme Gönder") | ❌ | ❌ | Tümü varsayılan kapalı; log sinyali ayrıca opt-in |
| Benchmark ↔ gerçek kullanım karşılaştırması | ✅ (son llama-bench koşusunun en iyi pp/tg'si gateway ortalamasının yanında) | ❌ | ❌ | — |

## 13. Donanım, backend ve motorlar

| Özellik | Harness | LM Studio | Ollama | Yol / not |
|---|---|---|---|---|
| Windows GPU: DXGI VRAM + PDH canlı kullanım, çoklu GPU sıralama | ✅ | ✅ | ✅ | — |
| Linux: NVIDIA (nvidia-smi), AMD/Intel (sysfs/hwmon), CUDA `ldconfig`/WSL | ✅ (Linux'ta çalıştırılmadı) | ✅ | ✅ | — |
| macOS: birleşik bellek, Intel Mac ayrımı | ✅ (`hw.memsize`/`iogpu.wired_limit_mb` kuralı; macOS'ta çalıştırılmadı) | ✅ | ✅ | — |
| Otomatik backend: CUDA / ROCm-HIP / SYCL / Vulkan / CPU / Metal | ✅ (ilk kurulumda; sonra yalnız öneri) | ✅ | ✅ | — |
| Runtime'a göre cihaz listesi (`--list-devices`, boş VRAM) | ✅ | ✅ | ✅ | — |
| Tensor split / main GPU önerisi | ✅ (boş VRAM oranı) | ✅ | ✅ | — |
| Resmi llama.cpp dışı runtime (PrismML fork vb.), sunucu başına runtime | ✅ (runtime kaydı: GitHub/yerel, SHA-256; tanım/profil başına seçim; GGUF kuantından öneri) | ✅ (runtime seçimi) | ❌ | **M** — motor soyutlaması |
| MLX motoru (Apple Silicon) | ❌ | ✅ (MLX; Mac'e özel Splash motoru 0.4.25) | ✅ (0.40.0'dan beri desteklenen mimarilerde varsayılan) | **M**, llama.cpp dışı — ayrı değerlendirme (ARCHITECTURE §13.8) |
| Yetim süreç koruması | ✅ Windows, ✅ Linux (PDEATHSIG), ✅ macOS (kqueue watchdog, çalıştırılmadı) | ✅ | ✅ | — |

## 13a. Uygulama penceresi ve yerleşim

| Özellik | Harness | LM Studio | Ollama app | Yol / not |
|---|---|---|---|---|
| Kendi pencere çerçevesi: başlık çubuğunda arama, durum, pencere düğmeleri | ✅ (Windows/Linux; macOS'ta yerel trafik ışıkları) | ✅ | ? | Snap Layouts menüsü özel ekranı kapla düğmesinde yok (Tauri sınırı) |
| Sürükleyerek genişletilen, daraltılan, hatırlanan yan paneller | ✅ (gezinme, Playground sohbet listesi ve ayarları, Dashboard ayarları, Ayarlar bölümleri; klavye ve Ctrl+B) | 🟡 | ❌ | Tek `ResizablePane` bileşeni |
| Küçük pencere ve yüksek ölçek uyumu | 🟡 (900×600'e kadar; dar pencerede paneller şeride iner ya da üstte açılır; ekranda doğrulanmadı) | ✅ | ✅ | Bkz. ROADMAP |

---

## 13b. Arayüz ve erişilebilirlik

| Özellik | Harness | LM Studio | Ollama | Yol / not |
|---|---|---|---|---|
| Açık / koyu / sistem teması | ✅ | ✅ | 🟡 (uygulama) | — |
| Yüksek kontrast temaları (AAA metin, ≥3:1 kenarlık, kalın odak) | ✅ (koyu + açık) | ❌ | ❌ | Kontrast ölçümü ARCHITECTURE §11.1 |
| OLED saf siyah, Gece (az mavi) teması | ✅ | 🟡 (birkaç renk teması) | ❌ | — |
| Renk körlüğü dostu durum ve grafik renkleri | ✅ (her temanın üstüne anahtar) | ❌ | ❌ | Okabe–Ito tabanlı; döteranopi/protanopi simülasyonuyla ölçüldü |
| Durumlarda renk dışı ipucu (simge, tarama, kesikli çizgi) | ✅ | 🟡 | — | Sunucu rozeti, günlük, analitik, benchmark, bellek göstergesi, canlı grafik |
| OS "kontrastı artır" (`prefers-contrast: more`) | ✅ | ❌ | — | Sistem teması yüksek kontrasta geçer; diğer temalarda ikincil yazı/kenarlık koyulaşır |
| Windows yüksek kontrast (`forced-colors`) | 🟡 | ? | — | Sistem renkleri + görünür odak; grafikler korunur. Elle denenmedi |
| Arayüz ölçeği (%90–150) | ✅ | ✅ | 🟡 | — |
| Arayüz dilleri | 🟡 (yalnız Türkçe / İngilizce) | ✅ (30+ dil²) | ? | Yeni dil eklemek locale dosyası ister |

---

## 14. Kopyalanmayacaklar (bilerek kapsam dışı)

| Özellik | Kimde | Neden |
|---|---|---|
| Web arama / web fetch (ollama.com API) | Ollama | Bulut hizmeti. İstenirse MCP sunucusu olarak eklenir, çekirdeğe girmez |
| Hub'a preset/plugin yayınlama, hesap sistemi, `lms login` | LM Studio | Harness hesabı veya sunucusu yok (cihazlar arası erişim hesapsız Harness Link ile) |
| Model kayıt defterine `push` | Ollama | Kendi kayıt defterimiz yok; HF zaten kaynak |
| Bionic (ajan uygulaması) | LM Studio | Ürün kapsamı dışı |
| JS/Python plugin çalışma zamanı | LM Studio | MCP aynı ihtiyacı standart yoldan karşılıyor |
| Canlı sürüklenebilir widget ızgarası, bulut ROI sayacı | — | Önceden elendi, gerekçe değişmedi |

**Değişen karar:** Daha önce "proxy/gateway yok, daemon yok" ilkesi vardı. JIT yükleme, tek uç noktada çok model, Ollama uyumluluğu ve istek başına gözlem için gateway şart.

Yeni karar:
- Gateway **isteğe bağlı** ve **uygulama sürecinin içinde** çalışır (ayrı daemon yok).
- Sunucuların doğrudan portları aynen kalır.
- İstek içeriği varsayılan olarak kaydedilmez, yalnızca metadata tutulur.

Ayrıntı: ARCHITECTURE §13.

## 15. Oluştur — araç üretimi

Ana menüde **Oluştur** grubu; ortak bölüm yan panelinde **Araç Oluşturucu**, **Otomatik Araç Oluşturucu**, **Araçlarım**. Manuel sayfa Playground üreticisini paylaşır. Otomatik sayfa seçilen modelle envanter → fikir → sıra → üretim → test → kurulum akışını yürütür; bekleyen kartlar sürükleme/klavye ile önceliklendirilir. Duraklat/devam et/durdur, hata sonrası tekrar deneme, canlı günlük ve sayaçlar vardır. TR/EN, açık/koyu tema ve azaltılmış hareket desteklenir.

Rust çekirdeğindeki kalıcı işçi UI, CLI ve Control API tarafından paylaşılır. Öneriler katı JSON ile doğrulanır; ad ve açıklama token benzerliği tekrarları eler. Üretici iki düzeltme denemesi yapar. Dış erişim istemeyen ve testten geçen paketler otomatik kalıcı kurulur; ağ/dosya isteyenler kesin izinleri gösteren onayı bekler. Testler dış erişim olmadan çalışır. Sıra, taslaklar ve günlük diskte saklanır; uygulama yeniden açılınca duraklatılmış çalışma devam ettirilebilir.

Araçlarım yerleşik, kurulu/üretilmiş ve sunucu bazında MCP araçlarını işlevsel kategorilerde listeler. Manifest `category` alanı sabit altı değerle doğrulanır; eski paketler `other` olur. Kurulu araç yönetimi mevcut Ayarlar bileşenini paylaşır; paket kodu/manifest/test incelemesi, etkinleştirme, test ve kaldırma vardır. [Kullanım ve paket rehberi](TOOLS.md).

---

## Dipnotlar

1. **Ses uç noktaları (LM Studio, Ollama):** LM Studio'nun OpenAI uyumluluk sayfası (lmstudio.ai/docs/developer/openai-compat) yalnız `/v1/models`, `/responses`, `/decisions`, `/chat/completions`, `/embeddings`, `/completions` uç noktalarını listeler; Ollama'nın OpenAI uyumluluk sayfası (docs.ollama.com/api/openai-compatibility) ses uç noktası anmaz. Bu yüzden önceki ✅ değerleri ❌ yapıldı; belgesiz bir uç nokta olabilir, elle denenmedi. LM Studio'nun Bionic uygulamasındaki ses yazıya çevirme özelliği (Bionic 1.1.3) LM Studio sunucusu değildir.
2. **LM Studio arayüz dilleri:** belgelerin bir sürümü "26+ daha fazla dil", diğeri yaklaşık 14 dil sayıyor (lmstudio.ai/docs/app/user-interface/languages); "30+" yaklaşık değerdir, kesin sayı doğrulanamadı.
3. `?` işaretli hücreler (ör. Ollama arayüz dilleri, LM Studio web araması/bulutu, MTP, görsel üretimi, `count_tokens`) resmi belgelerde doğrulanamadığı için tahmin edilmedi.
