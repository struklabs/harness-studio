# Kaynak kullanımı ölçümü (Harness Studio, LM Studio, Ollama)

[English](../en/PERFORMANCE.md)

İlk bölüm Harness Studio, LM Studio ve Ollama'nın boşta ve yükte RAM/CPU kullanımını yan yana karşılaştırır ve bariz israfları düzeltir; ardından boşta ayak izi çalışması gelir, sonraki bölümler KV önbelleği hassasiyetini, eşzamanlı istekleri ve kutudan çıkış ayarlarını kapsar. Tüm sayılar her bölümde anlatılan test sistemlerinde ölçüldü; uydurma ya da başka kaynaktan alınmış değer yok.

## Makine ve sürümler

| | |
|---|---|
| CPU / RAM | AMD Ryzen 7 9700X (8 çekirdek, 16 iş parçacığı), 31,1 GB |
| GPU | AMD Radeon RX 7900 GRE (16 GB) + entegre Radeon |
| OS | Windows 11 Pro 10.0.26200, ekran ölçeği %100 (96 DPI), WebView2 154.0.4258.x |
| Harness Studio | 0.1.0, release derlemesi (`cargo build -p harness-studio --release --features tauri/custom-protocol`), kurulum paketi değil |
| LM Studio | 0.4.25+1 (llama.cpp Vulkan motoru 2.47.0) |
| Ollama | 0.34.4 |
| Harness'in llama-server'ı | WinGet `ggml.llamacpp` kurulumu (Ayarlar > Model motoru'nın bulduğu) |

## Yöntem

- **Süreç ağacı:** uygulamanın başlattığı her şey toplanır: ana süreç, WebView2 çocukları (tarayıcı, GPU, renderer, yardımcılar) ve çıkarım süreci (`llama-server`). LM Studio ve Ollama için ad eşleştirmesi de eklendi (`LM Studio`, `lms`, `ollama`).
- **Bellek:** Windows performans sayaçları `Working Set - Private` (özel çalışma kümesi) ve `Private Bytes` (commit), süreç ağacında toplanmış, 5 sn'de bir örnek, 60 sn'lik pencerenin ortalaması. Üretim ölçümünde 2 sn'de bir.
- **CPU:** pencere başında ve sonunda her sürecin `TotalProcessorTime` farkı. "Tek çekirdek %" 100 = bir mantıksal çekirdeğin tamamı; toplam makinenin yüzdesi için 16'ya bölün.
- **Boşta:** uygulama açıldıktan 25-85 sn sonra, pencere açık ve ön planda, model yok, 60 sn.
- **Yükte:** model yüklü ve boşta 60 sn; sonra akışlı (stream) bir üretim: aynı istem ("uzun bir deniz feneri bekçisi hikayesi"), Harness'te `max_tokens=1500`, LM Studio'da `lms chat`, Ollama'da `ollama run --verbose`. Hız (token/sn) sunucunun kendi bildirdiği değerdir.
- **Aynı model dosyası:** LM Studio için `gemma-4-12B-it-Q4_K_M.gguf` (LM Studio'nun kendi klasöründeki dosya, Harness aynı dosyayı yerinde açtı), bağlam 4096, tam GPU. Ollama için `hermes3:8b` (Ollama'nın kendi blob'u; Harness uzantısız dosya adını kabul etmediği için aynı dosyaya sabit bağlantı (hard link) verildi, kopya yok, ölçüm sonunda silindi), bağlam 16384 (Ollama'nın seçtiği; `ollama ps`). Qwen3-4B-Instruct-2507 Q4_K_M ile yapılan ölçüm yalnız Harness içindir (LM Studio ve Ollama deposunda bu model yok, içe aktarma yapılmadı).
- **Yalıtım:** Ölçülen Harness, geliştirme kopyasından ayrı bir veri dizininde ve ayrı portlarda çalıştı (`HARNESS_DATA_DIR`, `WEBVIEW2_USER_DATA_FOLDER`; bkz. altta). LM Studio ve Ollama ölçüm öncesinde kapalıydı; yalnız bu ölçüm için açıldı, sonra kapatıldı. Ayarları, modelleri ve verileri değiştirilmedi (LM Studio'da model `lms load/unload` ile yüklenip boşaltıldı).

## Sonuçlar

### Boşta (model yok, pencere açık)

| | Özel çalışma kümesi | Commit | CPU (tek çekirdek %) | Süreç | İlk pencere |
|---|---:|---:|---:|---:|---:|
| Harness Studio, önce | 200-205 MiB | 289-319 MiB | **22,4** (3 ölçüm: 22-27) | 7 | 1,2-2,0 sn |
| Harness Studio, sonra | **169-176 MiB** | 285-291 MiB | **2,3** (2 ölçüm) | 7 | 1,3-1,8 sn |
| LM Studio 0.4.25 | 624 MiB (tepe 680) | 753 MiB | 0,6 | 6 | 10,6 sn |
| Ollama 0.34.4 | 938 MiB (tepe 1240), ilk dakika | 1112 MiB | 5,1 | 9 | 2,3 sn |
| Ollama, yerleştikten sonra (bir model yüklenip boşaltılınca) | 79 MiB | 341 MiB | 0,07 | 9 | - |

Notlar: LM Studio ve Ollama'nın ilk boşta değerleri açılıştan hemen sonrasıdır; Windows sonradan çalışma kümelerini küçültüyor (Ollama'nın ana süreci 1,1 GiB'tan 10 MiB'a indi, LM Studio'nun uygulama süreçleri 624'ten yaklaşık 200 MiB'a). Harness'in çalışma kümesi bu şekilde küçülmedi (pencere açık kaldı). Bu yüzden asıl karşılaştırma aşağıdaki "yüklü, boşta" satırlarındaki uygulama-yalnız toplamıdır. "İlk pencere", ana pencere tutamacının ve başlığının görüldüğü an; içeriğin çizimi ayrıca sürebilir. LM Studio'nun 10,6 sn'si tek ölçüm.

### Yükte (model yüklü)

Gemma-4-12B-it Q4_K_M, bağlam 4096, tam GPU, aynı dosya:

| | Yüklü, boşta: özel çalışma kümesi | Commit | Boşta CPU | Üretim CPU | Hız | Yükleme |
|---|---:|---:|---:|---:|---:|---:|
| Harness Studio (sonra) | 406 MiB (uygulama 178 + llama-server 227) | 9,3 GiB | 4,2 % | 40,1 % (llama-server 36,3) | 49,9 t/sn | 9 sn (dosya önbellekte) |
| LM Studio | 344 MiB (uygulama 195 + llama-server 149) | 10,0 GiB | 0,4 % | 47,1 % (llama-server 42,7; `lms` istemcisi dahil) | 48,0 t/sn (1971 token, ilk token 2,9 sn) | 61 sn (soğuk disk) |

Hermes3 8B Q4, bağlam 16384, tam GPU, aynı dosya:

| | Yüklü, boşta: özel çalışma kümesi | Commit | Boşta CPU | Üretim CPU | Hız | Yükleme |
|---|---:|---:|---:|---:|---:|---:|
| Harness Studio (sonra) | 354 MiB | 6,8 GiB | 2,5 % | 31,9 % (llama-server 27,7) | 81,0 t/sn (1243 token) | 7,1 sn |
| Ollama | 315 MiB (runner 236) | 4,7 GiB | 0,2 % | 43,5 % (runner 41,9) | 82,8 t/sn (1105 token) | ölçülemedi |

Yalnız Harness, Qwen3-4B-Instruct-2507 Q4_K_M, bağlam 4096, tam GPU (1500 token, akışlı):

| | Yüklü, boşta: çalışma kümesi | Boşta CPU | Üretim CPU | Küçültülmüş pencerede CPU | Hız |
|---|---:|---:|---:|---:|---:|
| Önce | 329 MiB | **39,8 %** | **70,3 %** | **50,3 %** | 139,5 t/sn |
| Sonra | 311 MiB | **2,75 %** | 38,6 % (llama-server 37) | **0,6 %** | 143,1 t/sn |

### Okuma

- Düzeltmeden önce Harness boşta çekirdeğin beşte birini, model yüklüyken ve pencere küçültülmüşken yarısını arayüz çizimine harcıyordu; LM Studio ve Ollama boşta %1'in altındaydı (Ollama açılış dakikası hariç). Düzeltmeden sonra fark küçüldü (Harness 2,3-4,2 %, LM Studio 0,4-0,6 %, Ollama 0,07-0,2 %); Harness hâlâ biraz yüksek, çünkü saniyede bir durum ve log yoklaması yapıyor. Küçültülmüş pencerede yoklama tamamen duruyor (%0,6).
- Bellekte Harness, yalnız uygulama tarafında LM Studio ile aynı düzeyde (boşta 170 MiB; yüklüyken uygulama tarafı 178 MiB'a karşı LM Studio'da 195 MiB; LM Studio açılışta 624 MiB, Ollama yerleşince 79 MiB). WebView2 çocukları Harness'in bellek payının büyük kısmı: yaklaşık 150 MiB.
- Çıkarım hızı üç uygulamada da llama.cpp'ye bağlı; ölçülen farklar (49,9 - 48,0 ve 81,0 - 82,8 t/sn) tek çalıştırmada gürültü sınırında. Motorlar aynı sürüm değil (LM Studio kendi Vulkan build'ini, Harness WinGet build'ini, Ollama kendi runner'ını kullanıyor), bu yüzden hız ve commit karşılaştırması motor farkını da içerir. Harness'in commit değeri (Hermes3'te 6,8 GiB'a karşı 4,7 GiB) bu build'in GPU sürücüsünde ayırdığı bellekten geliyor; ayrıntısı aşağıdaki boşta ayak izi bölümünde (Ollama tarafında q8_0 KV ve ROCm).

## Bulunan israflar ve düzeltmeler

Tarayıcı tarafı CDP (`--remote-debugging-port`) ile profillendi: ana iş parçacığı görevleri, yerleşim (layout) ve stil sayıları, çalışan animasyonlar.

| # | Bulgu | Düzeltme | Ölçüm |
|---|---|---|---|
| 1 | Telemetri panelindeki VRAM/RAM çubuğu `transition-[width]` kullanıyordu; değer her saniye değişince 500 ms boyunca her karede yerleşim yeniden hesaplanıyordu (15 sn'de 1284 yerleşim, ana iş parçacığı 1,94 sn) | Çubuk `transform: scaleX` ile çiziliyor, geçiş animasyonu yok | ana iş parçacığı 15 sn'de 1,94 sn'den 0,31 sn'ye (geçişi tutarak), sonra 0,18 sn'ye (geçişi kaldırınca); yerleşim 1284'ten 12'ye |
| 2 | Kenar çubuğunda çalışan her sunucunun yanındaki `animate-ping` sonsuz animasyon; GPU sürecini sürekli çalıştırıyordu (model yüklüyken tek başına yaklaşık %15 çekirdek) | Sabit halka yapıldı | yüklü boşta CPU 20,0 %'den 2,75 %'e |
| 3 | WebView2 pencere küçültüldüğünde ya da tepsiye gizlendiğinde `document.hidden` değerini `true` yapmıyor; bu yüzden "gizliyken yoklama yapma" kontrolleri hiç işlemiyordu. Ayrıca ana sayfadaki 1 sn'lik durum/telemetri yoklamasında ve Dashboard log yoklamasında bu kontrol hiç yoktu | Rust tarafı pencere küçülünce/gizlenince/geri gelince `window-visible` olayı yolluyor; arayüz (`windowActivity.ts`) bunu ve `document.hidden`'ı birleştiriyor. Ana sayfa, Dashboard logları, Ayarlar > Gateway, Performans ve Analitik yoklamaları pencere görünmezken duruyor, görününce hemen yenileniyor | küçültülmüş pencerede CPU 50,3 %'den 0,6 %'ye (düzeltme 2 ile birlikte) |
| 4 | Yüksek arayüz ölçeğinde (aşağıya bakın) kenar çubuğu içeriği sıkıştırıyordu | Pencere çerçevesi işiyle birleşince bu ayrı kural kaldırıldı; gezinme 1024 px altında simge şeridine iniyor (`ResizablePane`). | koddan; arayüzde görülmedi |

Çıkarım hızı düzeltmelerden etkilenmedi (139,5, 142,6 ve 143,1 t/sn); bellek de anlamlı değişmedi (özel çalışma kümesi 200'den 170 MiB'a, boşta).

Açılış süresi değişmedi: düzeltme öncesi 1,25-2,03 sn (4 ölçüm), sonrası 1,32-1,85 sn (4 ölçüm); ikisi de ana pencere tutamacının göründüğü an ve makine başka yükteyken alındı.

## Yüksek DPI ve küçük pencere

GUI'ye tıklanarak doğrulanmadı; bu yüzden yalnız kod incelemesi yapıldı ve açık madde [ROADMAP](ROADMAP.md)'te duruyor.

- Pencere en az 980 x 650 (`tauri.conf.json`); arayüz ölçeği (%90 - %150) WebView yakınlaştırmasıyla uygulanıyor. %150'de 980 px'lik pencerede CSS görünüm alanı yaklaşık 653 x 433 px olur.
- Sabit genişlikler: kenar çubuğu 216 px (daralınca 56 px), model seçici açılır penceresi 420 px, benchmark tablosu sütunları 120-240 px (tablo kaydırılıyor), Hub ve Kütüphane arama kutuları en az 220-240 px (satır kırılıyor), Dashboard `max-[860px]` kırılım noktasıyla tek sütuna geçiyor, diyaloglar `max-w-[calc(100%-2rem)]`. 720 px altında kenar çubuğunu daraltma eklendi.
- Yapılmadı: %125 ve %150 Windows ölçeğiyle gerçek pencerede tarama, 1024 x 640'ta her ekranın elle kontrolü.

## Ölçüm için eklenen ortam değişkeni

`HARNESS_DATA_DIR` (CLI'nin `--data-dir`'i ile aynı değişken) artık GUI'yi de kapsıyor: ayarlıysa uygulama o veri dizinini kullanır ve tek örnek (single-instance) korumasını atlar, böylece yalıtılmış ikinci bir kopya normal olanın yanında çalışabilir. Ayrı `WEBVIEW2_USER_DATA_FOLDER` ve ayrı port ayarlarıyla birlikte kullanılmalı (aksi halde WebView2 süreçleri paylaşılır). Normal kullanımda değişken ayarlı değildir ve davranış aynıdır.

## Sınırlar

- Ölçüm sırasında uygulamanın başka bir kopyası (debug derleme) da açıktı; öncesi ve sonrası aynı koşulda ölçüldü ama makinede başka yük vardı. Her koşul için tek 60 sn'lik pencere (boşta iki-üç tekrar); istatistiksel bir güven aralığı yok.
- Ayrık GPU belleği (VRAM) ölçülmedi; commit değeri sürücünün ayırdıklarını da içeriyor.
- LM Studio yükleme süresi soğuk disk, Harness'inki sıcak dosya önbelleği: karşılaştırılamaz. Ollama yükleme süresi ölçülemedi.
- LM Studio'da üretim `lms chat` ile yapıldı (sunucu açılmadı); üretim penceresindeki bellek ve CPU'ya `lms` istemcisi de girdi (yaklaşık 190 MiB), tablodaki bellek "yüklü, boşta" satırından alındı.
- LM Studio'nun akıl yürütme (thinking) modu açıktı ve 1971 token üretti; Harness'te 1500 token. Hız karşılaştırması yaklaşık.
- Linux ve macOS ölçülmedi.

## Boşta ayak izi: yoklama yerine itme, tepside pencereyi kapatma

Amaç: model yokken, model yüklü ama boştayken ve pencere küçültülmüş ya da tepsideyken uygulamanın CPU ve RAM'ini en aza indirmek. İlk ölçümden kalan boşta CPU, arayüzün saniyede bir durum, telemetri ve log yoklamasından ve arka ucun her saniye aldığı RAM/VRAM örneğinden geliyordu.

### Yöntem

Yukarıdaki yöntem, şu ayrıntılarla:

- **Derleme:** her iki taraf da `cargo build -p harness-studio --release --features tauri/custom-protocol` (önce `npm run build`). "Önce" = değişiklik olmayan derleme, "sonra" = değişiklikli derleme. İkisi aynı yalıtılmış veri dizinini ve WebView2 profilini sırayla kullandı.
- **Yalıtım:** `HARNESS_DATA_DIR` + `WEBVIEW2_USER_DATA_FOLDER` geçici bir klasörde; ayarlar: Control API 11600, gateway 11635 (ikisi de açık, geliştirme kopyasındaki gibi), sunucu portu 8190, llama-server WinGet build'i (özel ikili), kapatma = tepsi, rehber (tur) kapalı. Geliştirme kopyası kendi veri dizini ve portlarıyla açıktı (bir modeli yüklüydü), dokunulmadı.
- **Bellek:** süreç ağacında `Working Set - Private` toplamı, 5 sn'de bir, 60 sn ortalaması. **CPU:** süreç başına `TotalProcessorTime` farkı / 60 sn ("tek çekirdek %"); app (harness-studio), WebView2 ve llama-server ayrı da toplandı.
- **Senaryolar:** açılıştan 25 sn sonra 60 sn. *Boşta*: pencere açık, Dashboard, model yok. *Küçültülmüş*: `ShowWindow(SW_MINIMIZE)`, 10 sn sonra ölçüm. *Tepsi*: pencereye `WM_CLOSE` (kapatma = tepsi), 50 sn sonra ölçüm. *Yüklü*: Control API ile Qwen3-4B-Instruct-2507 Q4_K_M (bağlam 4096, tam GPU) yüklenip 20 sn sonra ölçüm. *Yüklü + tepsi*: aynı model, pencere tepsiye, 50 sn sonra ölçüm.
- **Açılış:** süreç başlangıcından ana pencere tutamacının görünür olduğu ana ("ilk pencere") ve `main` öğesinin DOM'da belirdiği ana ("etkileşime hazır"; CDP `--remote-debugging-port` ile 25 ms'de bir yoklandı, yalnız açılış ve geri getirme ölçümlerinde açık). Önce ve sonra beşer kez, sırayla dönüşümlü.
- **Tepsiden geri getirme:** tepsi simgesine tıklama, tray-icon'un geri çağırma iletisi (`WM_USER_TRAYICON` = 6002, `WM_LBUTTONUP`) uygulamanın tepsi penceresine gönderilerek yapıldı; süre tıklamadan pencerenin görünmesine ve `main` öğesinin oluşmasına kadar.
- Her satır ayrı bir başlatma; tablolarda en küçük-en büyük aralık. Ham sonuçlar ve betikler geçiciydi (silindi).

### Sonuçlar

| Senaryo | Önce: özel çalışma kümesi | Önce: CPU | Sonra: özel çalışma kümesi | Sonra: CPU |
|---|---:|---:|---:|---:|
| Boşta, pencere açık, model yok (önce 3, sonra 10 ölçüm) | 171-175 MiB | **1,51-1,75 %** (app 0,16-0,36) | 168-179 MiB | **0,31-1,25 %**, ortanca 0,65 (app 0-0,08) |
| Küçültülmüş, model yok | 165-166 MiB | 0,26-0,44 % | **78-80 MiB** | 0,29-0,36 % |
| Tepside, model yok | 162 MiB (7 süreç) | 0,34-0,52 % | **15-16 MiB (1 süreç)** | **0,00 %** |
| Tepside, "pencereyi bellekte tut" açık | - | - | 73 MiB | 0,08 % |
| Model yüklü, boşta, pencere açık | 314-316 MiB (llama-server 141) | **2,50-2,63 %** (llama-server 0,49-0,52) | 320-331 MiB (llama-server 141) | **0,52-1,61 %**, ortanca 0,78 (llama-server 0,18-0,52) |
| Model yüklü, pencere tepside | 312 MiB | 0,65-0,75 % | **59-158 MiB** (app 16 + llama-server 42-141) | **0,16-0,31 %** |

İlk ölçüm turunda rehber (tur) penceresi açıktı; o turda "önce" boşta 1,85-3,44 % idi (ilk ölçümdeki 2,3 % ile aynı düzey). Yukarıdaki "önce" satırları rehber kapalı ikinci turdan.

Açılış (5 + 5 ölçüm, dönüşümlü):

| | İlk pencere | Etkileşime hazır | Bellek (açılıştan 5 sn sonra) |
|---|---:|---:|---:|
| Önce | 718-795 ms (ortanca 721) | 941-1020 ms (ortanca 958) | 191-194 MiB |
| Sonra | 692-771 ms (ortanca 736) | 896-980 ms (ortanca 934) | 196-199 MiB |

Fark gürültü sınırında (~25 ms). İlk yüklenen JavaScript 2,06 MB'tan 1,25 MB'a indi (sekmeler ilk açıldıklarında yükleniyor), ama sıcak WebView2 profilinde açılışı ölçülebilir kısaltmadı: bu makinede açılışın çoğu WebView2'nin kendisi.

Tepsiden geri getirme:

| | Pencere görünür | Etkileşime hazır |
|---|---:|---:|
| Önce (pencere gizli bekliyordu) | 11-16 ms | 40-104 ms |
| Sonra, varsayılan (sayfa kapatılmıştı, yeniden kuruldu) | 312-491 ms | 516-741 ms |
| Sonra, "pencereyi bellekte tut" açık | 16 ms | 22 ms |

Süreç başına (boşta, sonra; özel çalışma kümesi / CPU): harness-studio 16 MiB / 0-0,08 %, WebView2 tarayıcı 33-35 MiB / 0,08-0,18 %, GPU süreci 48-60 MiB / 0-0,08 %, renderer 47-56 MiB / 0,03-0,6 %, ağ servisi 10 MiB, depolama servisi 3 MiB, crashpad 2 MiB. Renderer'ın ana iş parçacığı boşta 20 sn'de 23 ms çalışıyor (CDP `Performance.getMetrics`: 4 yerleşim, 4 stil hesabı); kalan CPU Chromium'un görünür bir pencere için kendi işi.

llama-server'ın kendisi (uygulama dışında, aynı model, 60 sn): yoklanmazken 0,13 %, saniyede bir `/metrics` + `/slots` yoklanırken 0,29 %. Yüklü-boşta satırındaki llama-server payının yarısı yoklamadan geliyor; pencere açıkken saniyede bir yoklama sürüyor (aşağıda).

### Değişenler

| # | Bulgu | Değişiklik |
|---|---|---|
| 1 | Ana sayfa saniyede bir `list_servers` + `get_telemetry_snapshot`, gateway durumu 3 sn'de bir, Dashboard saniyede bir log, indirme kuyruğu 3 sn'de bir soruyordu (küçültülmüşken de) | Arka uç değişince olay gönderiyor: `servers-changed` (yeni listeyle), `server-logs` (yeni satırı olan sunucular, 250 ms'de birleştirilmiş), `downloads-changed`, `gateway-changed`. Sunucu durumu, hata, yeniden başlatma ve kayıt/silme `harness_core::changes` üzerinden bildiriliyor (yükü olmayan, birleşen `watch` kanalı; log satırları Control API'nin olay akışına girmiyor). Arayüzde yoklama zamanlayıcısı kalmadı; pencere yeniden görünür olunca bir kez okunuyor. |
| 2 | Arka uç her saniye RAM + VRAM örnekliyordu, sunucu yokken ve kimse bakmazken de; VRAM için PDH sorgusu her seferinde açılıp kapanıyordu | Örnekleyici çalışan sunucu yokken ve görünür bir görünüm istemezken tamamen uyuyor. Dashboard'un bellek göstergeleri görünürken 2 sn'de bir, sunucu çalışırken pencere açıksa saniyede bir, pencere kapalı/küçük ve sunucular boştaysa 5 sn'de bir örnekliyor; bir istek işlenirken saniyede bire dönüyor, gateway isteği bitince hemen bir örnek alıyor (analitikteki bellek tepesi için). PDH sorgusu açık tutuluyor (`gpu::VramMonitor`), sysinfo nesnesi yeniden kullanılıyor ve yalnız bellek yeniliyor. Değişen telemetri `telemetry` olayıyla, yalnız izlenen sunucu için ve yalnız anlamlı fark varsa (sunucu alanları; RAM/VRAM'de 64 MiB) itiliyor. Durmuş sunucunun ya da Control API'nin anlık değeri istenince bellek o an okunuyor (`TelemetryPoller::fresh_snapshot`). |
| 3 | Gateway bakım döngüsü her saniye model tanımlarını diskten okuyordu (1 sn'lik önbellek) | Gateway'in yüklediği model yoksa hiçbir şey okumuyor. |
| 4 | Küçültülmüş/tepsideki pencere WebView2 belleğini olduğu gibi tutuyordu | Pencere görünmez olunca `ICoreWebView2_19::MemoryUsageTargetLevel = Low`, görünür olunca `Normal` (`webview2-com`). Küçültülmüşken GPU süreci 50 → 3 MiB, renderer 50 → 6 MiB. |
| 5 | Tepsideki pencere 7 süreç ve ~160 MiB tutuyordu | Pencere tepsiye gizlendikten 30 sn sonra, sayfanın hiçbir isteği sürmüyorsa ve 10 sn sessizse ve hiçbir sunucu bir isteği işlemiyorsa web görünümü kapatılıyor (`release_window`); tepsi simgesi/menüsü ya da ikinci başlatma pencereyi aynı yer ve boyutta yeniden kuruyor, açık sekme geri geliyor. Sunucular, indirmeler, gateway, Control API arka uçta olduğu için etkilenmiyor. Ayarlar > Görünüm ve Başlangıç > "Tepsideyken pencereyi bellekte tut" bunu kapatır (varsayılan kapalı). |
| 6 | Açılışta ilk ekran donanım taramasını ve çevrimdışı çökme raporu kontrolünü bekliyordu; GPU listesi (DXGI) pencere oluşmadan alınıyordu; tüm sekmeler tek 2 MB'lık JS parçasındaydı | İlk ekran yalnız ayarlar, sunucular ve profilleri bekliyor; donanım geldiğinde doluyor, çökme kontrolü sonra. GPU listesi pencere açıldıktan sonra arka planda. Dashboard dışındaki sekmeler, kurulum sihirbazı ve hazır ayar yöneticisi ilk açıldıklarında yükleniyor (`LazyView`). |
| 7 | Telemetri örnekleyicisi, veri dizini (`paths::init`) ayarlanmadan tepsi ipucu için ayarları okuyup önbelleğe alabiliyordu; o zaman uygulama o oturum boyunca varsayılan ayarlarla açılıyordu (ölçümde 15 başlatmada bir görüldü) | Örnekleyici ve itme görevleri `paths::init`'ten sonra başlıyor. |

Değerlendirilip alınmayanlar (boşta, pencere açık, sonra derlemesi, ikişer ölçüm):

| WebView2 argümanı | Özel çalışma kümesi | CPU | Karar |
|---|---:|---:|---|
| Varsayılan (Tauri'nin `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`) | 171-174 MiB | 0,31-0,39 % | |
| + `--disable-gpu` | 127-131 MiB | 0,44-0,47 % | Alınmadı: GPU süreci 48 → 9 MiB, ama çizim yazılımla (CPU) yapılıyor; kullanım sırasındaki CPU (kaydırma, akan yanıt) ölçülmedi. |
| + `--enable-low-end-device-mode` | 158-165 MiB | 0,36-0,68 % | Alınmadı: ~10 MiB kazanç; düşük renk derinlikli dokular görüntüyü bozabilir. |

### Okuma

- Model yokken pencere açık boşta CPU 1,5-1,75 %'ten ortanca 0,65 %'e indi (0,31-1,25); uygulama süreci neredeyse sıfır (0-0,08 %), kalan WebView2'nin görünür bir sayfa için harcadığı. LM Studio 0,4-0,6 % idi (faz 11). 0,5 % hedefi on ölçümün üçünde tuttu; değer makinedeki diğer yüke göre oynuyor ve kalan pay Chromium'un, uygulamanın değil.
- Model yüklü, pencere açık: 2,5-2,6 %'ten ortanca 0,8 %'e. Bunun 0,2-0,5'i llama-server; pencere açıkken telemetri canlı göründüğü için saniyede bir yoklama sürüyor.
- Tepside model yokken Harness artık yalnız kendi süreci: 15-16 MiB ve 0 % CPU (Ollama yerleşince 79 MiB, 0,07 %). Model yüklüyken tepside 0,16-0,31 % (llama-server'ın kendi boşta payı 0,13 %).
- Bedeli: tepsiden geri getirmek 0,5-0,75 sn (önce ~0,1 sn). Kaydedilmemiş girdiler (Playground'da gönderilmemiş metin, Dashboard'da kaydedilmemiş form değişikliği) sayfa kapanınca kaybolur; açık sekme geri gelir. Ayarla kapatılabilir.

### Sınırlar

- Makinede geliştirme kopyası (model yüklü) ve başka işler açıktı; CPU satırları birkaç ölçümlük aralık, güven aralığı yok. Pencerenin üstünde başka pencere olup olmadığı denetlenmedi (Chromium kapalı kalan pencereyi çizmeyebilir); önce ve sonra aynı koşulda ölçüldü.
- Açılış sıcak WebView2 profiliyle; soğuk açılış (ilk kurulum, yeniden başlatma sonrası) ölçülmedi.
- Geri getirme süresi simgeye gerçek tıklamayla değil, tepsi iletisi gönderilerek ölçüldü. Pencerenin aynı yerde ve boyutta, ekranı kaplamış durumda açılması ve sekmenin geri gelmesi koddan; ekranda doğrulanmadı.
- `--disable-gpu` yalnız boşta ölçüldü.
- Yalnız Windows. Linux ve macOS'ta `MemoryUsageTargetLevel` yok (çağrı yapılmıyor); tepside sayfayı kapatma aynı kodla çalışır ama denenmedi.

### macOS

MacBook Air M1 8 GB, macOS 27.0.1, release build (`/Applications/Harness Studio.app`, ad-hoc imzalı). Bellek: uygulamanın, WebKit süreçlerinin (WebContent, GPU, Networking) ve uygulamadan sonra başlayan llama-server'ın `phys_footprint` toplamı; CPU: aynı süreç kümesinin `ps` CPU zamanı farkı / 60 sn.

| Senaryo | Bellek | CPU |
|---|---:|---:|
| Pencere açık, model yok | 174 MiB | 1,28 % |
| Küçültülmüş (Dock), model yok | 171 MiB | 0,12 % |
| Tepside, model yok (sayfa bırakıldı) | **43 MiB** | **0,00 %** |
| Model yüklü (Qwen3-4B, 4k), pencere açık | 867 MiB | 3,05 % |
| Model yüklü, tepside (Kütüphane sayfasından, sayfa bırakıldı) | uygulama 44 + llama-server 665 MiB | 0,47 + 0,68 % |

- Tepside sayfayı bırakma macOS'ta da çalışıyor (WebContent ~1 dk içinde kapanıyor).
- **Açık hata:** sunucu çalışırken **Dashboard** sayfası açıkken tepsiye gönderilirse sayfa bırakılmıyor (5 dk sonra WebContent hâlâ açık, toplam 914 MiB); aynı durumda Kütüphane sayfasından bırakılıyor. Neden bulunmadı.
- **Açık hata:** `--minimized` + "Küçültülmüş başlat" ile gizli açılışta sayfa hiç bırakılmıyor (90 sn sonra WebContent 127 MiB): başlangıçta `window-in-tray` olayı gelmediği için bırakma zamanlayıcısı kurulmuyor.
- Küçültmek (Dock) belleği düşürmüyor, yalnız CPU'yu.
- llama-server'ın 665 MiB'ı ağırlıkların `mmap` edilmiş kısmını saymıyor; gerçek bellek için aşağıdaki Metal bellek tablosuna bakın.

### Süreç başına VRAM ve çok sunuculu telemetri

Dashboard'un VRAM çubuğunu tüketiciye göre bölmek için her telemetri turunda süreç başına ayrılmış GPU belleği de okunuyor (Windows: PDH `\GPU Process Memory(*)\Dedicated Usage`). Yeni zamanlayıcı yok: sayaç, sistem VRAM'ini okuyan aynı turda, açık tutulan ikinci bir PDH sorgusundan okunuyor; tur boştayken uyuyan örnekleyiciyle birlikte uyuyor.

**Yöntem.** Bu makine (Ryzen 7 9700X, RX 7900 GRE + entegre Radeon, Windows 11). Release derlemesinde `VramMonitor` açılıp ısındıktan sonra 25 tur, aralarında 1 sn (gerçek tur aralığı), her turda `usage()` (mevcut) ve `process_usage()` (yeni) ayrı ayrı zamanlandı; ortanca ve en yüksek. Aynı sırada 37 süreç GPU belleği tutuyordu (geliştirme uygulaması + bir llama-server ve tarayıcılar). Ölçüm için geçici test dosyası kullanıldı ve silindi.

| Okuma (tur başına) | Ortanca | En yüksek |
|---|---|---|
| Sistem VRAM'i (`usage()`, mevcut) | 43 µs | 93 µs |
| Süreç başına VRAM (`process_usage()`, yeni) | 97 µs | 140 µs |
| İlk çağrı (sorguyu açma dahil, bir kez) | 2,1 ms | |

**Okuma.** Tur başına eklenen maliyet ~0,1 ms (1 Hz'de saniyede ~%0,01 bir çekirdek; ölçüm gürültüsünün altında), bellek: 37 örneklik bir `HashMap`. Süreç tablosu RAM dökümü için zaten her turda yenileniyordu; ağaç kurma aynı tablodan yapılıyor, ek okuma yok. Tüm sunucuları izleme ek örnekleme getirmiyor (örnekleyici zaten hepsini okuyor); yalnız `telemetry` olayı, değişen her sunucu için ayrı itiliyor ve yalnız Dashboard telemetri paneli açıkken. Boşta CPU'ya etkisi ayrıca ölçülmedi: bu iş yalnız örnekleyici çalışırken (sunucu varken ya da bellek göstergesi görünürken) yapılıyor.

**Sınırlar.**
- Linux (`/proc/<pid>/fdinfo`) yolu birim testli ama test makinesinde ölçülmedi; maliyeti ağaçtaki süreçlerin açık dosya sayısıyla büyür (yalnız DRM satırı olan dosyalar ayrıştırılır).
- Linux'ta NVIDIA'da süreç başına bölme, süreç içinde `libnvidia-ml.so.1`'den yüklenen NVML'den gelir (donanımda doğrulanmadı); yalnız NVML yüklenemezse çubuk bölünmez (`nvidia-smi` tur başına süreç başlatırdı). macOS'ta çubuk bölünmez.
- Yalnız izlenen (birincil) ekran kartı; Windows'ta yalnız `Dedicated Usage`.

### Tepsiden geri getirme: önceden kurma ve açılış ekranı

Sayfası kapatılmış pencerenin tepsiden açılması 0,3-0,5 sn'de görünür, 0,5-0,75 sn'de kullanılabilir oluyordu (yukarıda). Değişiklik:

- İmleç tepsi simgesine gelince (`TrayIconEvent::Enter`/`Move`; Windows ve macOS) ya da simgeye basılınca (`Click` + `Down`, sağ tık dahil) pencere arka planda gizli kuruluyor (`prewarm_main_window`); tıklama yalnız gösteriyor. 20 sn içinde gösterilmezse `release_window` ile aynı yoldan yeniden kapatılıyor; imleç simgede durdukça süre uzuyor. Linux StatusNotifierItem tepsisi yalnız tıklamayı bildiriyor; orada pencere simgeye tıklanınca kuruluyor.
- Kapatılan sayfanın WebView2 tarayıcı süreci daha kapanırken yeniden kurmak (aynı profil) yeni sayfayı yüklemiyordu; artık önce o sürecin çıkması bekleniyor (tıklamada en çok 5 sn; üstüne gelmede beklenmeden bir sonraki olaya erteleniyor).
- `src/app.html`'de betik beklemeyen açılış ekranı; pencere ve web görünümü temanın arka plan rengiyle kuruluyor.

**Yöntem:** yukarıdaki yöntem: `cargo build -p harness-studio --release --features tauri/custom-protocol`, "önce" = değişiklik olmayan derleme, "sonra" = değişiklikli derleme; aynı yalıtılmış veri dizini ve WebView2 profili (`HARNESS_DATA_DIR`, `WEBVIEW2_USER_DATA_FOLDER`), Control API 11600, gateway 11635, sunucu portu 8190, kapatma = tepsi, CDP 9351. Önce ve sonra beşer + üçer tur, her tur ayrı bir başlatma, sırayla dönüşümlü. Her ölçümden önce pencereye `WM_CLOSE` (tepsiye), sayfanın kapatılması (pencerenin yok olması) ve 4 sn beklendi. Tepsi olayları tray-icon'un penceresine `WM_USER_TRAYICON` (6002) iletisiyle: üstüne gelme `WM_MOUSEMOVE`, tıklama `WM_LBUTTONDOWN` + 90 ms + `WM_LBUTTONUP`. Süreler `WM_LBUTTONUP`'tan: **görünür** = pencere tutamacı görünür (ayrı iş parçacığında 2 ms'de bir), **hazır** = CDP ile `main` öğesi var ve açılış ekranı yok ya da kalkmakta (önce derlemesinde açılış ekranı yok, yalnız `main`). Makinede başka derlemeler ve bir llama-bench çalışıyordu.

| Senaryo (8 tekrar) | Önce: görünür | Önce: hazır | Sonra: görünür | Sonra: hazır |
|---|---:|---:|---:|---:|
| Üstüne gelip 1 sn sonra tıklama | 327-650 ms (ortanca 385) | 579-973 ms (676) | **15-20 ms (16)** | **3-6 ms (4)** |
| Üstüne gelip 0,3 sn sonra tıklama | - | - | **16-48 ms (16)** | **151-352 ms (258)** |
| Üstüne gelmeden tıklama (basma → 90 ms → bırakma) | 341-463 ms (381) | 591-731 ms (644) | 250-419 ms (305) | 488-669 ms (558) |
| Yalnız bırakma iletisi (menüden "Göster" gibi) | 339-428 ms (363) | 592-704 ms (634) | 352-500 ms (406) | 577-772 ms (662) |
| Açılış (süreç başlangıcından, 6 tekrar) | 813-1022 ms (964) | 1086-1284 ms (1196) | 771-1127 ms (964) | 1035-1414 ms (1177) |

Tıklanmayan üstüne gelme: sayfa kapalıyken 1 süreç / 16 MiB; üstüne geldikten 3 sn sonra 15 süreç / 179-192 MiB (pencere gizli, sayfa yüklü); 25 sn sonra yeniden 1 süreç / 17-18 MiB (3 ölçüm).

Okuma:
- Fareyle simgeye gidip tıklamak (üstünde gezinme her zaman olur) artık zaten yüklenmiş bir sayfayı gösteriyor: görünür ~16 ms, sayfa hazır. Üstünde 0,3 sn kalınırsa sayfa tıklamadan ~0,25 sn sonra hazır. Hover olayı olmadan basılınca basma ile bırakma arasındaki süre kadar kazanç var (~75 ms).
- Yalnız bırakma iletisi (menü yolu) gürültü sınırında (ortanca +43 ms görünür, +28 ms hazır); "sonra" eski tarayıcı süreci çıkmadan kurmuyor.
- Tıklanmayan her üstüne gelme 20 sn boyunca ~175 MiB'a mal oluyor; sonra tepsideki ayak izi aynı (16-18 MiB).

Açılış ekranı ve yazı tipi:
- `first-contentful-paint` (WebView2) iki derlemede de tıklamadan ~2,2 sn sonra raporlandı (`DOMContentLoaded` ~0,35 sn); masaüstü başka pencerelerle paylaşılırken (Chromium örtülen pencereyi çizmiyor) ilk çizim zamanı güvenilir ölçülemedi. Açılış ekranının ve önceden kurulan sayfanın ekranda ne zaman göründüğü gözle doğrulanmalı ([ROADMAP](ROADMAP.md)).
- Ekran görüntüsünden: tepsiden açılan pencere, sayfa çizilene kadar temanın arka plan rengi (koyu temada `#0f1115`), beyaz değil.
- Boyut: arayüz paketi 7 609 309 → 7 689 637 B (+80 KB; IBM Plex Sans Latin 45,7 KB + Latin Genişletilmiş 31,0 KB, değişken ağırlık; CSS +0,9 KB). `harness-studio.exe` 24 839 168 → 24 927 232 B (+86 KB, +0,35 %). Dört statik ağırlık (400/500/600/700) aynı iki alt kümede 159 KB olurdu; değişken dosya seçildi. Latin Genişletilmiş dosyası yalnız o karakterler görününce yükleniyor.

Sınırlar: tepsi olayları ileti gönderilerek üretildi, gerçek fareyle denenmedi; macOS'ta hover kodla var ama denenmedi; tek makine, yük altında, güven aralığı yok.

---

# Çıkarım belleği, llama-server varsayılanları, boyut ve disk

Amaç: aynı model dosyasında Harness'in commit değerinin (Hermes3 8B'de 6,8 GiB, Ollama 4,7 GiB) nereden geldiğini bulmak, RAM'i büyüten llama-server varsayılanlarını küçültmek, `harness serve`'ün boşta maliyetini, ikili dosya ve kurulum paketi boyutunu ve veri dizininin büyümesini ölçmek. Tüm sayılar test makinesinde ölçüldü (makine ve sürümler yukarıdaki tabloyla aynı).

## Yöntem

- **llama-server doğrudan:** yönetilen runtime'ın (b11302 Vulkan) karalama dizinine kopyası; `-ngl 999 --split-mode none --main-gpu 0 -c 16384` ve ölçülen bayraklar. `/health` hazır olunca 8 sn beklenip ölçüldü; sonra 400 token'lık 3 üretim (aynı istem, `seed`) ve tekrar ölçüm. Bellek: `Win32_PerfFormattedData_PerfProc_Process` (`WorkingSetPrivate` = özel çalışma kümesi, `PrivateBytes` = commit); VRAM: `GPU Process Memory(pid_*)\Dedicated Usage`. Hız ve istem süresi sunucunun kendi `timings` değerleri.
- **Büyüme:** aynı sunucuya (a) `docs/ARCHITECTURE.md`'nin farklı ~2000 sözcüklük dilimlerinden oluşan 6-8 ayrı istek (~4k token), (b) her turda ~400 sözcük eklenen 20 turluk tek konuşma. Her istekten sonra commit örneklendi.
- **Ollama:** 0.34.4, ayrı portta (`OLLAMA_HOST=127.0.0.1:11977`) yalnız ölçüm için `ollama serve`; `hermes3:8b` blob'u, `num_ctx=16384`; runner'ın komut satırı süreçten okundu. Ölçüm sonunda kapatıldı.
- **Harness'in kendisi:** `harness serve --data-dir <geçici> --port 18600 --gateway --gateway-port 18601 --no-restore`, açılıştan 20 sn sonra 60 sn'lik pencere (5 sn'de bir örnek), önce/sonra dönüşümlü 3'er kez.
- **Boyut:** `cargo build --release` (çalışma alanı profili: `lto = true`, `codegen-units = 1`, `panic = "abort"`, `strip = true`), GUI `--features tauri/custom-protocol` ile; kurulum paketleri `npx tauri bundle -b nsis,msi` (imzasız). Hız karşılaştırması için `harness-core` fonksiyonlarını çağıran ayrı bir ölçüm programı aynı profille derlendi (işlenmedi).

## Commit farkı: Harness 6,6 GiB, Ollama 4,5 GiB (Hermes3 8B Q4, bağlam 16384, tam GPU)

| | VRAM | Commit | Özel çalışma kümesi (boşta / 3 üretim sonrası) | Üretim |
|---|---:|---:|---:|---:|
| Harness varsayılanı (`-np` otomatik = 4, `-fa` otomatik, KV f16) | 6456 MiB | 6583-6618 MiB | 139-166 / 269 MiB | 97,5-99,9 t/sn |
| `--no-host` | 6456 | 6582 | 139 / 252 | 100,7 |
| `GGML_VK_DISABLE_HOST_VISIBLE_VIDMEM=1` | 6459 | 6997 | **550 / 663** | 96,2 |
| `-np 1 -fa on -ctk q8_0 -ctv q8_0` | **5495** | **5655** | 159 / 219 | 96,5 |
| Ollama'nın bayrakları, Harness'in Vulkan build'inde (`+ -b 1024 -ub 1024`) | 5611 | 5769 | 170 / 232 | 95,7 |
| **Ollama 0.34.4** (runner = kendi llama-server'ı, **ROCm**) | 5809 | **4534** (üretim sonrası 4618) | 243 / 308 | 84,8-86,2 |

`-np 1` ve `-fa on` tek başına VRAM ve commit'i değiştirmedi (6456 / 6581-6583 MiB; tek üretimle ölçüldükleri için hızları yazılmadı).

Fark üç şeyden geliyor, ikisi Harness'in elinde değil:

1. **KV önbelleği türü.** Ollama runner'ı `--cache-type-k q8_0 --cache-type-v q8_0 --flash-attn on -np 1 -b 1024 -ub 1024 --context-shift --keep 4` ile başlatıyor. q8_0 test makinesindeki kullanıcı ortam değişkenlerinden geliyor (`OLLAMA_KV_CACHE_TYPE=q8_0`, `OLLAMA_FLASH_ATTENTION=true`); Ollama'nın kendi varsayılanı f16. KV 2048 MiB'tan 1088 MiB'a iniyor. Aynı ayarla Harness da 5,5 GiB VRAM / 5,7 GiB commit'e iniyor.
2. **Sürücü hesabı.** AMD'nin Vulkan sürücüsü cihaz belleği ayırmalarını sürecin commit'ine neredeyse bire bir yazıyor (commit ≈ VRAM + ~150 MiB). Ollama bu makinede ROCm backend'ini seçiyor; orada 5,8 GiB VRAM 4,5 GiB commit olarak görünüyor. Bu commit fiziksel RAM değil, sistemin commit sınırından düşülen bir ayırma. Fiziksel RAM'de (özel çalışma kümesi) Harness'in llama-server'ı Ollama'nın runner'ından küçük: 139-166'ya karşı 243-308 MiB.
3. **Yüklü boşta 354'e karşı 315 MiB** (önceki ölçüm): Harness'in llama-server'ı daha küçük olduğuna göre fark uygulama tarafında (WebView2). O kısım ayrı bir iş kolunda.

**KV varsayılanı f16 kaldı:** q8_0'da istem işleme bu Vulkan build'inde %10-25 yavaş (aşağıdaki tablo: 1609-1918'e karşı 2080-2147 t/sn), üretim hızı aynı. "Sunucuyu Hazırla" (ajan araçları) zaten q8_0 seçiyor; Dashboard'da seçilebiliyor.

**`-np` otomatik (4 slot, birleşik KV) bu aşamada değişmedi:** kayan pencereli olmayan modellerde maliyeti yok (VRAM aynı). Kayan pencereli modellerde (Gemma) SWA önbelleği slot sayısıyla büyüyor: Gemma-4-12B'de `-np 1` boşta commit'i 9287'den 8316 MiB'a indiriyor (~1 GiB VRAM) ve her bağlam kontrol noktasını küçültüyor. Bedeli: aynı anda gelen istekler sıraya girer (ajanlar, alt ajanlar, gateway'e bağlı birden çok araç). Bu bir ödünleşimdir. VRAM tahmini (`vram.rs`) otomatik slotu zaten 4 birleşik slot olarak hesaplıyor.

## Gerçek RAM büyümesi: istem önbelleği ve bağlam kontrol noktaları

llama-server'ın iki varsayılanı, sunucu kullanıldıkça fiziksel RAM'i GiB'larca büyütüyor:

- `--cache-ram 8192`: bitmiş konuşmaların durumu sistem belleğinde saklanıyor (8 GiB'a kadar).
- `--ctx-checkpoints 32` (slot başına): kayan pencereli ve hibrit modellerde (Gemma, gpt-oss, Qwen3.5) konuşmanın kopyaları; her istekte yaklaşık iki tane. Gemma-4-12B'de (4 slot) biri 320 MiB. `--cache-ram` bunları sınırlamıyor.

Hermes3 8B, 8 farklı ~4k token'lık istek (KV f16):

| | Özel çalışma kümesi: boşta → son | Commit: boşta → son | İstem işleme |
|---|---:|---:|---:|
| llama-server varsayılanı | 152 → **4741 MiB** | 6595 → 11259 MiB | 2106 t/sn |
| `-np 1` | 150 → 4731 | 6594 → 11243 | 2084 |
| **`--cache-ram 1024`** | 151 → **835** | 6594 → 7325 | 2147 |
| `--cache-ram 0` | 150 → 164 | 6593 → 6653 | 2080 |
| q8_0 KV, `-fa on`, `--cache-ram 1024` | 152 → 871 | 5634 → 6443 | 1918 |
| q8_0 KV, `-fa on`, `--cache-ram 0` | 152 → 172 | 5634 → 5766 | 1609 |
| f16 KV, `-fa on`, `--cache-ram 0` | 151 → 170 | 6595 → 6659 | 2124 |

Gemma-4-12B Q4 (kayan pencere 1024), bağlam 32768, 20 turluk tek konuşma:

| | Özel çalışma kümesi: boşta → son | Commit: boşta → son | İstem işleme |
|---|---:|---:|---:|
| llama-server varsayılanı | 217 → **7582 MiB** | 9577 → 19851 MiB | 293 t/sn |
| `--cache-ram 1024` | 217 → 8728 | 9577 → 19820 | 320 |
| **`--cache-ram 1024 --ctx-checkpoints 4`** | 217 → **1438** | 9577 → 11480 | 320 |

Gemma-4-12B, bağlam 16384, 6 farklı istek: varsayılan 230 → 6083 MiB, `--cache-ram 1024` 200 → 805 MiB, `-np 1 --cache-ram 1024` 194 → 763 MiB.

**Yeni varsayılanlar (kullanıcı bir değer seçmediyse):** `--cache-ram 1024` ve `--ctx-checkpoints 4`. Yalnız runtime bayrağı tanıyorsa gönderiliyor (eski build'e bilinmeyen bayrak gitmez, uyarı da yazılmaz). Katalog değeri ya da özel argümanlarda bayrağın herhangi bir yazımı (`-cram`, `--swa-checkpoints=8` …) varsa Harness kendi değerini eklemiyor; kullanıcı llama-server'ın değerine (`8192`, `32`) ya da `-1`'e dönebilir. Kataloğa `ctx_checkpoints` parametresi eklendi (Gelişmiş > Bellek); iki alanın ipucu Harness'in değerini gösteriyor. CLI'dan yüklenen bir sunucunun komut satırında doğrulandı (`… --metrics --split-mode none --main-gpu 0 --cache-ram 1024 --ctx-checkpoints 4`).

Bedelleri (Gemma-4-12B, 8 tur, ~8k token, `--cache-ram 1024`):

| | 4 kontrol noktası | 32 kontrol noktası |
|---|---:|---:|
| Son yanıtı yeniden üret | 1 token, 97 ms | 1 token, 100 ms |
| Son kullanıcı mesajını düzenle | 12 token, 189 ms | 12 token, 259 ms |
| 4 tur önceki mesajı düzenle | **3655 token, 4,6 sn** | 13 token, 0,5 sn |

- Kayan pencereli modelde birkaç tur geriye gidip mesaj düzenlemek istemi baştan işletiyor (bu GPU'da Gemma-4-12B ~800 t/sn). Yeniden üretme ve son mesajı düzenleme etkilenmiyor.
- 1 GiB'tan fazla konuşma arasında gidip gelince, geri dönülen konuşmanın istemi yeniden işleniyor (Hermes3'te ~4k token ≈ 2 sn).
- Bellek tahmini (`vram.rs`) değişmedi: VRAM aynı. İstem önbelleği ve kontrol noktaları çalışırken büyüyen sistem belleği; yükleme tahminine girmiyor ve artık bu iki sınırla tavanlı (Gemma-4-12B'de ~1 GiB + etkin slot başına 4 × 320 MiB).

## `harness serve` boşta

| | Özel çalışma kümesi | Commit | CPU (tek çekirdek %) | İş parçacığı | Veri dizini (80 sn) |
|---|---:|---:|---:|---:|---:|
| Önce (3 ölçüm) | 9,8-10,2 MiB | 11,3-11,9 MiB | 0,05-0,15 | 20 | 36 KiB |
| Sonra (3 ölçüm) | 9,8-10,0 MiB | 11,4-11,6 MiB | 0,05-0,17 | 20-21 | 36 KiB |

Headless mod zaten küçük; fark gürültü içinde. Yapılan: gateway zamanlayıcısının saniyelik TTL turu, gateway'in yüklediği bir model yokken tanımları artık okumuyor (önceden her saniye `state.db` sorgusu). sysinfo'nun `multithread` özelliği kapandı (rayon havuzu yok; süreç listesi yenilemesi ek iş parçacığı açmıyor). 1 Hz telemetri örnekleyicisi başka bir iş kolunda.

## Boyut

| | Önce | Sonra | Fark |
|---|---:|---:|---:|
| `harness-studio.exe` | 29 875 712 B (28,5 MiB) | 24 898 048 B (23,7 MiB) | **-16,7 %** |
| `harness.exe` | 18 441 216 B (17,6 MiB) | 15 131 136 B (14,4 MiB) | **-17,9 %** |
| `harness-hidden.exe` | 214 016 B | 203 264 B | -5,0 % |
| NSIS kurulum paketi | 15,40 MiB | 13,58 MiB | **-11,8 %** |
| MSI | 22,76 MiB | 19,36 MiB | **-14,9 %** |
| Arayüz paketi (`build/`) | 7,55 MB, 128 dosya | değişmedi | |

Yapılanlar:

1. **Bağımlılıklar** (`harness.exe` 18,44 → 18,19 MB): `zip` yalnız Deflate/Deflate64 (zstd, bzip2, xz, AES gitti; llama.cpp arşivleri ve DOCX Deflate), `sysinfo` 0.33 → 0.38 ve yalnız `system` + `disk` (rayon, ağ, bileşen, kullanıcı modülleri gitti), `windows` 0.61 → 0.62 (CLI'da tek `windows` sürümü kaldı, önce 0.57 + 0.61), `toml` 0.8 → 1 (Tauri ile aynı; toml 0.8, toml_edit 0.20 ve eşleri gitti). Kalan çift sürümler Tauri, updater ve OTel'den geliyor (`reqwest` 0.12 + 0.13, `png`, `infer`, `rand`, `getrandom`, `windows` 0.61 `tauri-plugin-opener`'dan).
2. **Paket başına `opt-level = "s"`** (`harness.exe` 18,19 → 15,13 MB): sıcak yolda olmayan kod: HTTP yönlendirme katmanı (axum, tower), CLI ayrıştırma (clap), OpenAPI (utoipa), OTLP (opentelemetry*, tonic, prost, rustls), `harness-api`/`-mcp`/`-cli`/`-gateway`/`harness-studio` ve Tauri/WebView kabuğu. `harness-core` (hash, GGUF, RAG), sha2, PDF/zip çözücüler, serde_json, SQLite, tokio ve hyper 3'te kaldı. Paket başına ayarın tam LTO altında da geçerli olduğu ölçüldü (ikili `s`, `harness-core` + `sha2` 3: SHA-256 1079 ms, GGUF 3,3 ms, parçalama 1,9 ms).

Her şeyi `s` yapmak `harness.exe`'yi 11,85 MB'a indiriyor ama hızı düşürüyor; bu yüzden yapılmadı:

| (ölçüm programı, 2 tekrar) | `opt-level = 3` | her şey `s` |
|---|---:|---:|
| SHA-256, 2,4 GB model dosyası (indirme doğrulaması) | 1212-1227 ms | **4777-4800 ms** |
| PDF metni (66 KB) | 4,7-4,9 ms | 7,4-7,5 ms |
| Parçalama (4 × ARCHITECTURE.md) | 1,7-1,8 ms | 2,4-2,5 ms |
| GGUF üst verisi | 3,5 ms | 4,6 ms |
| En yakın 8 (50k × 768 vektör) | 19,1-19,6 ms | 21,0-21,4 ms |
| JSON okuma / yazma (41 MB) | 41,6-43,0 / 44,6-46,1 ms | 44,9-46,4 / 39,7-39,9 ms |

`panic = "abort"` zaten açıktı; RAG PDF çocuk süreci (`src-tauri/src/main.rs`) buna göre yazılmış, değişmedi. `lto = true` (fat) ve `codegen-units = 1` de zaten açıktı.

**OpenTelemetry'nin boyut maliyeti** (`harness.exe`, yukarıdaki 1. ve 2. adımdan önce ölçüldü):

| `otel` | `harness.exe` | Fark |
|---|---:|---:|
| Kapalı | 15 975 424 B | - |
| Yalnız HTTP/protobuf, reqwest 0.12 üzerinden (~20 satırlık `HttpClient` uyarlaması; deneme, işlenmedi) | 16 595 456 B | +0,62 MB |
| Yalnız HTTP/protobuf, reqwest 0.13 (`reqwest-client`) | 16 935 424 B | +0,96 MB |
| Şimdiki: HTTP + gRPC (tonic, rustls/ring, kök sertifikalar) + reqwest 0.13 | 18 439 680 B | **+2,46 MB (+15,4 %)** |

gRPC tek başına 1,50 MB, ikinci reqwest 0,34 MB, OTel SDK + HTTP dışa aktarıcı 0,62 MB. Seçenekler: gRPC'yi ayrı bir özelliğe almak (`otel-grpc`, varsayılan derlemede yok), OTLP/HTTP'yi reqwest 0.12 üzerinden göndermek (ikinci reqwest gider), ya da `otel`'i varsayılan derlemeden çıkarmak. İlk ikisi yapıldı (aşağıda).

### OpenTelemetry inceltme

OTLP/HTTP çalışma alanının reqwest 0.12'siyle gidiyor (`otel/export.rs`, `opentelemetry_http::HttpClient` uyarlaması); `reqwest013` bağımlılığı kalktı. gRPC (tonic, rustls/ring, kök sertifikalar) `otel-grpc` özelliğine taşındı; varsayılan derlemelerde yok. Aynı makine, aynı profil (yukarıdaki `opt-level = "s"` ayarları dahil), aynı arayüz paketi; `cargo build --release -p harness-cli` ve `-p harness-studio --features tauri/custom-protocol` ayrı ayrı:

| | Önce (`otel`: HTTP + gRPC + reqwest 0.13) | Sonra (`otel`: yalnız HTTP, reqwest 0.12) | Fark |
|---|---:|---:|---:|
| `harness.exe` | 15 219 200 B | 14 641 152 B | **-578 048 B (-3,8 %)** |
| `harness-studio.exe` | 25 006 080 B | 24 652 800 B | **-353 280 B (-1,4 %)** |
| `harness.exe --features otel-grpc` | | 14 913 024 B | gRPC = +271 872 B |

`opt-level = "s"` sonrası gRPC'nin payı 1,50 MB değil 0,27 MB; ikinci reqwest (ve onun hyper-tls/native-tls kopyası) 0,31 MB. GUI'de reqwest 0.13 `tauri-plugin-updater`'dan geldiği için kalıyor; GUI'deki kazanç gRPC + OTel'in reqwest 0.13 özellikleri. Varsayılan `harness.exe`'de artık `tonic`, `rustls` ve `reqwest` 0.13 yok (`cargo tree -i`, ikilide `grpc-status`/`rustls` dizgesi yok).

## Disk: veri dizini büyümesi

| Ne | Sınır |
|---|---|
| Sunucu logları `logs/servers/<id>/` | Önce: sunucu başına son 20 çalıştırma, ama **tek çalıştırmanın dosyası sınırsız** (haftalarca açık, yoğun kullanılan bir sunucu GB'larca yazabilir). Sonra: dosya 16 MiB'ı geçince aynı çalıştırma yeni dosyada sürüyor; sunucu başına en yeni dosyalar 20 dosya ve toplam 64 MiB sınırına kadar tutuluyor (en yeni dosya her zaman kalır). Aynı milisaniyede açılan dosya var olanı ezmiyor. |
| `logs/diagnostics` | 50 dosya (değişmedi) |
| `analytics.db` | Ham istekler 30 gün, günlük özetler kalıcı (küçük), içerik 7 gün; bakım saatlik, sonunda `wal_checkpoint(TRUNCATE)` (değişmedi) |
| `bin/staging` | Kurulumdan sonra siliniyor (değişmedi) |
| `bin/backup` | Önceki tek runtime (geri alma için; test makinesinde 91 MB). Tek kopya, her güncellemede yenisiyle değişiyor |
| Model indirmelerinin `.part` dosyaları | Sürdürmek için kalıyor; indirme listesinden kısmi veriyle birlikte kaldırılınca gidiyor (tasarım gereği) |

## Sınırlar

- Tek makine, AMD Vulkan sürücüsü. Commit ≈ VRAM ilişkisi bu sürücüye özgü olabilir; NVIDIA/CUDA, Linux ve macOS ölçülmedi.
- Hız ölçümleri 3 üretimin ortalaması (ilk tablo) ya da tek seri (büyüme tabloları); istatistiksel güven aralığı yok. İstem işleme hızı farkı (f16 / q8_0) iki seride tutarlı.
- Ollama ROCm, Harness Vulkan kullandığı için hız ve commit karşılaştırması backend farkını da içeriyor.
- Arayüzün (WebView2) boşta belleği, telemetri örnekleyicisi ve pencere/tepsi kodu bu çalışmanın dışında (ayrı iş kolu).
- Kurulum paketleri imzasız ve güncelleyici yapıtları olmadan üretildi; WebView2 paket içinde değil (varsayılan indirme önyükleyicisi).

## Düşük donanım (Debian, 4 GB RAM, Intel HD 6000)

Debian 13 XFCE/X11, Intel Core i5-5250U (2 çekirdek / 4 iş parçacığı), 3,7 GiB kullanılabilir RAM + 5,8 GiB takas, Intel HD Graphics 6000 (Broadwell GT3, Mesa Vulkan, `uma: 1`, Vulkan0'da 1,9 GiB görünür), yönetilen runtime b11316 (Vulkan). Ölçümler arayüz kapalıyken.

### Yöntem
- `llama-bench -t 2 -p 512 -n 128 -r 3`, her koşu `timeout 600` ile sınırlı (aşağıdaki askı yüzünden). 4 iş parçacığı üretimde 2'den yavaştı (hiper iş parçacığı), seri 2 ile.
- Üç yerleşim: **CPU** = `-dev none -ngl 0` (Vulkan yok), **ngl 0** = Vulkan yüklü, ağırlıklar işlemcide (istem işlemenin bir kısmı GPU'da), **ngl 99** = her şey tümleşik GPU'da.
- Tepe RSS `/usr/bin/time %M`. Vulkan yerleşimlerinde ağırlıklar GPU'ya eşlenmiş paylaşılan bellekte durduğu için RSS'te görünmüyor (sistem RAM'inden yine düşüyor); CPU yerleşiminde ağırlıklar yeniden paketlendiği için RSS dosyadan büyük.
- Bütün modeller Q4_K_M, bağlam llama-bench varsayılanı (pp512 / tg128).

### Sonuçlar (t/s, istem işleme pp512 / üretim tg128)

| Model | Dosya | CPU | ngl 0 | ngl 99 | CPU tepe RSS |
|---|---|---|---|---|---|
| Qwen2.5-0.5B-Instruct | 469 MB | 55 / 21,1 | 130 / **28,2** | 146 / 20,3 | 657 MB |
| Qwen3-0.6B | 378 MB | 93 / **30,1** | 91 / 26,0 | 96 / 16,0 | 783 MB |
| Llama-3.2-1B-Instruct | 770 MB | 44 / **15,9** | 52 / 13,7 | 54 / 9,9 | 1419 MB |
| gemma-3-1b-it | 769 MB | 21 / 8,7 | 72 / 7,5 | 77 / **9,8** | 1021 MB |
| phi-1_5 | 852 MB | 21 / 10,0 | 42 / 9,4 | 43 / 10,0 | 1592 MB |
| Qwen2.5-Coder-1.5B-Instruct | 1066 MB | 23 / **9,8** | 42 / 8,6 | 43 / 8,8 | 1861 MB |
| SmolLM2-1.7B-Instruct | 1007 MB | 19 / **9,0** | 32 / 8,0 | 33 / 7,8 | 1990 MB |
| Qwen3-1.7B | 1056 MB | 21 / **8,3** | 36 / 7,3 | 38 / 6,9 | 1921 MB |

Standart sapmalar üretimde ≤ 0,5 t/s, `ngl 0` istem işlemede 1-15 t/s (en küçük modellerde yüksek).

### Okuma
- **Varsayılan `-ngl 0` doğru:** ağırlıklar işlemcide ama Vulkan istem işlemeyi hızlandırıyor (CPU'ya göre 1,5-3,4 kat); üretim en iyiye yakın. Tam GPU (`ngl 99`) yedi modelin altısında üretimde en yavaş (Qwen3-0.6B'de 30 → 16 t/s). İstisna gemma-3-1b (9,8 / 8,7 t/s); benchmark geçmişi varsa uyum önerisi ölçülen yönü kullanıyor.
- **Kullanılabilir boyut:** 0,5-0,6B modeller 20-30 t/s ile sohbet için rahat; 1-1,7B modeller 7-16 t/s.
- **Kararsızlık:** Qwen3-1.7B `ngl 99` önceki oturumda (b11299) llama-bench'i bir saatten uzun `drm_syncobj_array_wait_timeout`'ta askıda bıraktı; b11316'da tamamlandı. OpenCode ile Llama-3.2-1B `-ngl 999`, 65k bağlam, ~7,6k token istemde llama-server `vk::DeviceLostError` (GPU sıfırlandı) ile çöktü. Bu GPU'da uzun istemle tam yükleme güvenilir değil; araç hazırlığı artık `-ngl 0` kullanıyor.

### OpenCode (Llama-3.2-1B, `-ngl 0`, 65 536 bağlam, q4_0 KV, FA, 1 slot)
`harness launch opencode --prepare --headless -- run "a.txt dosyasının içeriğini oku…"`:

| Adım | Süre |
|---|---|
| Model yükleme + başlık isteği (564 token) | ~21 sn (istem 61 t/s) |
| Ana istem 7 582 token (OpenCode 1.18 `run`; ~29k değil) | 225 sn (33,7 t/s; 2k'da 68, 6k'da 41 t/s) |
| İlk yanıt üretimi | 41 token, 21 sn (1,9 t/s) |
| Araç sonrası tur (önbellekten, 108 token) | 3,6 sn istem + 12 sn üretim |
| Toplam | 286 sn |

Model `read` aracını çağırdı (argümanları kısmen bozuk) ama dosya içeriği yerine araç açıklaması yazdı. **Değerlendirme:** bu donanımda OpenCode çalışıyor, ama her yeni oturumun ilk yanıtı ~4 dk ve 1B model ajan işi için yetersiz. Önerilen bir uyarı [ROADMAP](ROADMAP.md)'te.

### Harness Studio'nun kendisi
| Durum | RAM | CPU |
|---|---|---|
| `harness serve`, model yok (debug) | 47 MB, 5 iş parçacığı | %0 (60 sn) |
| Arayüz açık, sunucu kapalı (debug + `tauri dev`) | uygulama 250 MB + WebKitWebProcess 369 MB + WebKitNetworkProcess 81 MB | %1,6 (60 sn, tüm süreçler) |
| Tepside, sayfa bırakıldıktan sonra (debug) | uygulama 217 MB + WebKitNetworkProcess 64 MB | — |
| llama-server, Qwen2.5-0.5B, 4k, `-ngl 33` | 143 MB (RSS) | — |

Sürüm derlemesi ölçülmedi; debug uygulama süreci sürümden belirgin büyük (Windows'ta sürüm ~170 MiB toplam).

# Eşzamanlı istekler (slotlar)

Karar ve gerekçe: [ARCHITECTURE §5.4d](ARCHITECTURE.md). Soru: llama-server'ın varsayılanı (`-np` otomatik = 4 slot, birleşik KV) mı, 1 slot mu?

## Yöntem
- RX 7900 GRE 16 GB, Vulkan, managed runtime b11312 (geçici bir dizine kopyalandı), `-dev Vulkan0 -ngl 99 -c 32768 -fa on -fit off -cram 1024 -ctxcp 4` (Harness'in bellek varsayılanları), KV f16. Yapılandırmalar: `-np` yok (otomatik: 4 slot, birleşik), `-np 1`, `-np 2 --kv-unified`.
- İstekler `/completion`, sıcaklık 0, `ignore_eos`, 200 token üretim, her istekte farklı ~1,2-1,4k token istem (önek önbelleği yardım etmesin). Tek istek: 2 ölçümün ortalaması. Eşzamanlı: 4 istek aynı anda; toplam hız = 800 token / duvar saati, gecikme = isteğin toplam süresi.
- VRAM: sürecin ayrılmış GPU belleği (PDH `GPU Process Memory`), yüklendikten sonra ve eşzamanlı turdan sonra. KV ve kontrol noktası boyutları sunucu günlüğünden (`-lv 4`).
- Makine başka bir oturumla paylaşılıyordu: bir ölçüm yalnız başka sürecin GPU hesap motoru kullanımı (PDH `GPU Engine ... engtype_compute`) sıfıra yakınken başlatıldı; ölçüm sırasında başka bir llama süreci ya da GPU kullanımı görülürse sonuç atılıp tekrarlandı.

## Sonuçlar

| Model (mimari) | Slot | VRAM yüklü → tur sonu | KV (tam + pencere) | Tek istek üretim / istem t/sn | 4 eşzamanlı: toplam t/sn | 4 eşzamanlı: gecikme en az / ort. / en çok |
|---|---:|---:|---|---:|---:|---|
| Qwen3-4B Q4_K_M (tam dikkat) | 4 (otomatik) | 7229 → 7274 MiB | 4608 MiB | 133,3 / 3099 | **184,2** | 4,3 / 4,3 / 4,3 sn |
| | 2 (birleşik) | 7229 → 7274 | 4608 | 133,9 / 3174 | 120,4 | 2,6 / 4,9 / 6,6 |
| | 1 | 7229 → 7274 | 4608 | 134,5 / 3158 | 102,6 | 1,9 / 4,9 / 7,8 |
| gpt-oss-20b MXFP4 (pencere 128) | 4 (otomatik) | 12010 → 12146 | 768 + 24 | 140,1 / 3205 | **175,9** | 4,5 / 4,5 / 4,5 |
| | 2 (birleşik) | 12004 → 12141 | 768 + 18 | 131,1 / 3067 | 119,1 | 2,9 / 4,8 / 6,7 |
| | 1 | 12004 → 12040 | 768 + 18 | 129,9 / 3038 | 95,9 | 2,0 / 5,3 / 8,3 |
| Gemma-4-12B Q4_K_M (pencere 1024, 40/48 katman) | 4 (otomatik) | **9303** → 9894 | 512 + **1440** | 46,7 / 881 | **54,4** | 14,6 / 14,7 / 14,7 |
| | 2 (birleşik) | 8660 → 8892 | 512 + 800 | 46,9 / 875 | 40,9 | 8,6 / 14,1 / 19,6 |
| | 1 | **8338** → 8473 | 512 + 480 | 46,7 / 860 | 32,6 | 6,2 / 15,3 / 24,5 |

- **Tek istek hızı slot sayısından bağımsız** (fark ölçüm gürültüsü içinde): fazladan slot boşta bir şey yavaşlatmıyor.
- **4 eşzamanlı istekte toplam hız** 4 slotla 1 slota göre Qwen3-4B'de 1,8×, gpt-oss'ta 1,8×, Gemma-4-12B'de 1,7×; en uzun bekleyen isteğin süresi 7,8 → 4,3 sn, 8,3 → 4,5 sn, 24,5 → 14,7 sn. Ortalama gecikme hemen hemen aynı (sıra beklemek ile birlikte yavaş üretmek dengeleniyor); 1 slotta ilk istek en hızlı biter.
- **Bellek:** tam dikkatli modelde (Qwen3-4B) ve küçük pencereli gpt-oss'ta slotların VRAM farkı yok ya da birkaç MiB. Gemma-4-12B'de pencere önbelleği slotla büyüyor: 4 slot 1 slota göre **+960 MiB** KV (ölçülen süreç: +965 MiB), 2 slot +320 MiB. Kontrol noktası başına boyut slottan bağımsız (Gemma 256,6 MiB, gpt-oss 3,0 MiB); toplam kontrol noktası belleği kullanılan slot başına en çok `--ctx-checkpoints` (Harness 4) kadar, yani 4 slotta 4 kat.
- Bellek tahmini (`vram.rs`) bu ölçümlere göre düzeltildi: Gemma 4'ün pencere katmanları 8 KV başı × 256, tam dikkat katmanları 1 × 512; tahmin 4 slotta 512 + 1440, 1 slotta 512 + 480 MiB veriyor (`slots::tests::gemma4_swa_cache_matches_real_loads`, günlükler `tests/data/b11312-vulkan-gemma4-12b-c32k-np{1,4}.load-log.txt`).

## Paylaşılan havuz taşınca

Qwen3-4B, `-c 8192`, iki eşzamanlı ~4,4k ve ~5,4k token'lık istem (toplam bağlamdan büyük):

| | 1. istek | 2. istek |
|---|---|---|
| Otomatik (4 slot, birleşik 8192) | **500 "Context size has been exceeded."** (3,2 sn) | **500, aynı hata** (3,2 sn) |
| `-np 1` | tamam, 4,3 sn | tamam (sırada bekledi), 9,3 sn |

Her biri tek başına sığan iki istek birlikte havuzu aşınca llama-server ikisini birden düşürüyor; 1 slotta ikincisi yalnız bekliyor. Otomatik kuralın "her eşzamanlı isteğe en az 8k / ajanlarda 32k token" koşulu bu yüzden var.

## Okuma
- Ucuz slot (tam dikkat, küçük pencere): 4 slot bedava ve eşzamanlı istekte ~1,8× toplam hız; sınırlayan bellek değil bağlam havuzu.
- Pahalı slot (Gemma'nın 1024'lük penceresi, hibrit modellerin durumu): her slot yüzlerce MiB VRAM + kontrol noktası RAM'i. Tek kişilik sohbette 1 slot ~1 GiB kazandırıyor; ajanların paralel istekleri için 2 slot (+320 MiB) en uzun bekleyen isteği 24,5'ten 19,6 sn'ye indiriyor.

## Sınırlar
- Tek makine, Vulkan; CUDA için [aşağıdaki bölüm](#eşzamanlı-istekler-cuda), Metal'de toplu üretim kazancı farklı olabilir (llama.cpp #29335'te Metal'de 32k bağlamda ters etki bildirildi).
- Qwen3.5/3.6 hibrit modeller ölçülmedi: 27B Q4 bu kartta tam sığmıyor; slot başına durum (~150 MiB) GGUF başlığından hesaplandı.
- 4 eşzamanlı istek tek tur; kısa istem (1,2-1,4k). Uzun ajan istemlerinde istem işleme payı büyür, eşzamanlı istem işleme de batch'i paylaşır.

---

# KV önbelleği hassasiyeti

**Politika güncellemesi:** CUDA, Metal ve ROCm’da otomatik merdiven yalnız eşit çiftlerden oluşur ve q8_0/q8_0’da biter: q4_0/q4_0, CUDA’da (b11342) Qwen3-0.6B’nin 8k ve 32k çıktısını bozdu ve bir Qwen3-4B araç çağrısını karıştırdı; q8_0/q8_0 sığmazsa ağ geçidi bunun yerine bağlamı küçültür. q4_0/q4_0 bu backend’lerde elle seçilen Tasarruf olarak kalır. Vulkan’ın ölçülmüş q8_0/q4_0 basamağı korunur. Ayrıntı backend bölümlerinde.

Soru: konuşma hafızası (KV önbelleği, `--cache-type-k/-v`) hangi türde saklanmalı, ne zaman düşürülmeli? Hedef: kaliteyi bozmadan en az kaynak. Uygulanan politika: [ARCHITECTURE.md §5.4c](ARCHITECTURE.md#54c-kv-önbelleği-hassasiyeti-kv_policyrs). Tüm sayılar bu makinede (Windows 11, RX 7900 GRE 16 GB, 32 GB RAM) yönetilen runtime'ın kopyasıyla ölçüldü: llama.cpp **b11312** (0c1e57098) Vulkan, `-dev Vulkan0` (iGPU dışarıda), `-ngl 99 -fa on`. Bu build K/V nicemlenince Hadamard döndürmesini (PR #21038) kendiliğinden açıyor (logda `attn_rot_k = 1`).

## Araştırma özeti

- **Türler:** `f32, f16, bf16, q8_0, q4_0, q4_1, iq4_nl, q5_0, q5_1`; varsayılan f16 ([common/arg.cpp](https://github.com/ggml-org/llama.cpp/blob/master/common/arg.cpp); runtime `--help` ile doğrulandı).
- **Flash attention:** nicemlenmiş **V** için zorunlu, K için değil. FA olmadan V devrik saklanıyor, bu yüzden blok nicemleme yapılamıyor. `-fa auto` (PR #15434'ten beri varsayılan) aygıt desteklemiyorsa FA'yı kapatıyor. V nicemliyse llama.cpp "quantized V cache was requested, but this requires Flash Attention" hatasıyla duruyor ([llama-context.cpp](https://github.com/ggml-org/llama.cpp/blob/master/src/llama-context.cpp)).
- **K, V'den hassas:** [PR #7412](https://github.com/ggml-org/llama.cpp/pull/7412) PPL farkları: q8_0/q8_0 +0,005; f16/q4_0 (yalnız V) +0,022; q4_0/q4_0 +0,21. Yazar K'nin çok daha hassas olduğunu belirtiyor. [Tartışma #5932](https://github.com/ggml-org/llama.cpp/discussions/5932): Qwen2 q4_0'da bozuluyor, K q8_0 + V q4_0 bunu düzeltiyor. Ollama'ya KV nicemlemeyi ekleyen yazarın ölçümü: q8_0 +0,002-0,05, q4_0 +0,2-0,25 PPL ([smcleod.net](https://smcleod.net/2024/12/bringing-k/v-context-quantisation-to-ollama/)).
- **Döndürme:** [PR #21038](https://github.com/ggml-org/llama.cpp/pull/21038) (2026-04) nicemlemede Q/K/V'yi döndürerek düşük bitli KV kaybını büyük ölçüde azaltıyor (gpt-oss-20b AIME25'te q4_0 KV ile %2 → %21,7). Bu PR'dan önceki ölçümler bugünkü kaybı abartıyor.
- **Backend:**
  - CUDA varsayılan derlemesinde FA çekirdeği yalnız eşleşen çiftler için var (`q4_0-q4_0;q8_0-q8_0;f16-f16;bf16-bf16`). `GGML_CUDA_FA_QUANTS=all` olmadan karışık çift (q8_0/q4_0) her token'da GPU'da f16'ya çevriliyor (CPU'ya düşmüyor; üretim ölçümde %27-38 yavaş, bkz. [CUDA bölümü](#cuda)) ([docs/build.md](https://github.com/ggml-org/llama.cpp/blob/master/docs/build.md), [#28633](https://github.com/ggml-org/llama.cpp/issues/28633)).
  - ROCm'da da karışık çiftler yavaş yola düşüyor ([#22411](https://github.com/ggml-org/llama.cpp/discussions/22411)).
  - Vulkan'da aşağıdaki ölçüme göre karışık çift GPU'da kalıyor; hızı eşleşen çiftlerle aynı sınıfta.
- **Model sınıfları:**
  - Qwen2/2.5 K'de aykırı kanallar taşıyor. KIVI'nin anahtarı kanal başına nicemleme önerisi bundan geliyor ([arXiv 2402.02750](https://arxiv.org/abs/2402.02750)).
  - gpt-oss (attention sink) KV nicemlemeye duyarlı.
  - Kayan pencereli modellerde (Gemma, gpt-oss) SWA önbelleği zaten küçük; kazanç tam katmanlardan geliyor.
  - MLA modellerinde (DeepSeek) V ayrı tutulmuyor; tek etkili ayar `-ctk`.
- **Ollama:** `OLLAMA_KV_CACHE_TYPE` `f16` (varsayılan), `q8_0` ya da `q4_0` alıyor ve tüm modellere uygulanıyor. FA gerekiyor; FA desteklenmeyen mimaride sessizce f16'ya dönüyor ([docs.ollama.com/faq](https://docs.ollama.com/faq), [#13337](https://github.com/ollama/ollama/issues/13337)). Bu makinede kullanıcı q8_0 seçmiş.
- **LM Studio:** yükleme ayarlarında K ve V türü ayrı (`llamaKCacheQuantizationType`, `llamaVCacheQuantizationType`); V için FA gerekiyor ([lmstudio.ai docs](https://lmstudio.ai/docs/typescript/api-reference/llm-load-model-config)).
- Ollama da LM Studio da model ve belleğe göre otomatik seçim yapmıyor.
- **Uzun bağlamda bulma:** akıllı nicemleme yöntemleri (KIVI, [KVQuant](https://www.stat.berkeley.edu/~mmahoney/pubs/neurips-2024-kvquant.pdf)) NIAH'ı koruyor. llama.cpp'nin düz blok nicemlemesi için yayımlanmış bir NIAH/RULER ölçümü bulunamadı; bu yüzden aşağıda ölçüldü.

## Yöntem

- **Kalite:** `llama-perplexity` ile wikitext-2 raw test, `-c 4096 --chunks 20` (40 960 puanlanan token).
  - Önce f16/f16 ile `--kl-divergence-base`, sonra her yapılandırma `--kl-divergence`.
  - Ortalama KLD, %99 KLD ve "aynı en olası token" oranı raporlandı.
  - f16/f16 tekrarı gürültü tabanı olarak alındı (KLD 0).
- **Uzun bağlamda bulma:**
  - Haystack: wikitext-2 test metninden ~16k ve ~31k token'lık gerçek parçalar.
  - Dört "special magic number" iğnesi (RULER MK-NIAH biçimi). Sorulan iğne %10/30/50/70/90 derinlikte, diğer üçü başka derinliklerde çeldirici.
  - Doğru sayılmak için 7 haneli sayı yanıtta tam olarak geçmeli.
  - Ayarlar: `llama-server -c 33792 -np 1`, sıcaklık 0, istem önbelleği kapalı. Her yapılandırmada aynı metin ve iğneler.
  - Deneme sayısı (derinlik ve uzunluk başına): Qwen3-4B 3, gpt-oss-20b 2 (`reasoning_effort: low`).
- **Hız:** `llama-bench -p 2048 -n 128 -d 0,16384 -r 5`. GPU başka işlerle paylaşıldığı için ikinci bir geçiş yapıldı; f16 hem başta hem sonda koştu. Tabloda ikinci geçiş, f16 satırında iki koşunun aralığı var.
- **Bellek:** KV boyutu GGUF şeklinden hesaplandı; load loguyla birebir aynı çıkıyor. Süreç GPU belleği Windows PDH `GPU Process Memory\Dedicated Usage` ile, 31k istekten sonra okundu.
- **Modeller:**
  - Qwen3-4B-Instruct-2507 Q4_K_M: yoğun, 8 KV başı × 128.
  - Qwen2.5-Coder-14B-Instruct Q4_K_M: yoğun, Qwen2.5 K aykırıları.
  - gpt-oss-20b MXFP4: MoE, kayan pencere + sink, 8 KV başı × 64.

## Kalite (KL ıraksaması, f16 KV'ye göre)

| Model | K / V | PPL | Ortalama KLD | %99 KLD | Aynı en olası token |
|---|---|---|---|---|---|
| Qwen3-4B | f16 / f16 | 8,705 | 0 | 0,00004 | %100 |
| | q8_0 / f16 | 8,706 | 0,00075 | 0,0052 | %98,7 |
| | f16 / q8_0 | 8,704 | 0,00063 | 0,0045 | %98,8 |
| | **q8_0 / q8_0** | 8,706 | **0,00071** | 0,0051 | %98,7 |
| | **q8_0 / q4_0** | 8,699 | **0,0028** | 0,022 | %97,6 |
| | q4_0 / q4_0 | 8,836 | 0,0236 | 0,226 | %93,3 |
| Qwen2.5-Coder-14B | f16 / f16 | 6,859 | 0 | 0,00004 | %100 |
| | q8_0 / f16 | 6,859 | 0,00028 | 0,0019 | %99,2 |
| | f16 / q8_0 | 6,858 | 0,00026 | 0,0018 | %99,2 |
| | **q8_0 / q8_0** | 6,860 | **0,00028** | 0,0020 | %99,1 |
| | **q8_0 / q4_0** | 6,862 | **0,0013** | 0,011 | %98,3 |
| | q4_0 / q4_0 | 6,907 | 0,0071 | 0,054 | %96,1 |
| gpt-oss-20b | f16 / f16 | 1769* | 0 | 0,00003 | %100 |
| | q8_0 / f16 | 1759 | 0,046 | 0,72 | %88,2 |
| | f16 / q8_0 | 1772 | 0,038 | 0,62 | %89,8 |
| | **q8_0 / q8_0** | 1758 | **0,046** | 0,75 | %88,5 |
| | **q8_0 / q4_0** | 1643 | **0,083** | 1,16 | %83,1 |
| | q4_0 / q4_0 | 1986 | 0,299 | 2,34 | %63,2 |

\* gpt-oss ham metinde (sohbet şablonu olmadan) eğitim dağılımının çok dışında kalıyor (PPL 1769). Bu yüzden KLD mutlak değer olarak abartılı; yapılandırmalar arasındaki sıralama yine anlamlı.

## Uzun bağlamda bulma (doğru / deneme)

| Model | K / V | ~16k (16 536 / 16 351 token) | ~31k (31 505 / 30 643 token) |
|---|---|---|---|
| Qwen3-4B | f16 / f16 | 15/15 | 15/15 |
| | q8_0 / q8_0 | 15/15 | 15/15 |
| | q8_0 / q4_0 | 15/15 | 15/15 |
| | q4_0 / q4_0 | 15/15 | 15/15 |
| gpt-oss-20b | f16 / f16 | 9/10 | 10/10 |
| | q8_0 / q8_0 | 9/10 | 10/10 |
| | q8_0 / q4_0 | 9/10 | 10/10 |
| | q4_0 / q4_0 | 8/10 | 10/10 |

- gpt-oss'un kaçırdığı deneme (%30 derinlik, aynı metin) f16 dahil her türde aynı. q4_0/q4_0 bir deneme daha kaçırıyor.
- Tek iğneli bulma bu bağlam uzunluklarında türleri birbirinden ayırmıyor; kalite farkını KLD gösteriyor.

## Hız (t/sn, `llama-bench`, Vulkan)

| Model | K / V | istem 2048 | istem 2048 @16k | üretim 128 | üretim @16k |
|---|---|---|---|---|---|
| Qwen3-4B | f16 / f16 | 2439-3041 | 699-701 | 87-149 | 85 |
| | q8_0 / f16 | 2842 | 871 | 144 | 85 |
| | q8_0 / q8_0 | 2843 | 856 | 143 | 91 |
| | q8_0 / q4_0 | 2894 | 817 | 140 | 92 |
| | q4_0 / q4_0 | 2978 | 840 | 142 | 93 |
| | f16 / q8_0 | 2899 | 957 | 146 | 74 |
| Qwen2.5-Coder-14B | f16 / f16 | 917-923 | 284 | 51 | 35 |
| | q8_0 / f16 | 909 | 412 | 50 | 39 |
| | q8_0 / q8_0 | 899 | 413 | 50 | 37 |
| | q8_0 / q4_0 | 900 | 390 | 49 | 37 |
| | q4_0 / q4_0 | 900 | 392 | 49 | 38 |
| | f16 / q8_0 | 919 | 433 | 50 | 33 |
| gpt-oss-20b | f16 / f16 | 3449-3453 | 1861-1864 | 147 | 129 |
| | q8_0 / f16 | 3398 | 1684 | 143 | 127 |
| | q8_0 / q8_0 | 3396 | 1749 | 141 | 127 |
| | q8_0 / q4_0 | 3339 | 1595 | 141 | 128 |
| | q4_0 / q4_0 | 3327 | 1599 | 142 | 128 |
| | f16 / q8_0 | 3407 | 1770 | 143 | 129 |

- **Kısa bağlamda istem işleme:** her türde f16'nın %1-3'ü içinde. Qwen3'ün iki f16 koşusu kendi arasında 2439 ile 3041 arasında oynadı (GPU paylaşımlıydı).
- **16k derinlikte, yoğun modeller:** nicemlenmiş KV istem işlemede **daha hızlı**. Qwen2.5-14B 284 → 413 (+%45), Qwen3-4B 700 → 856 (+%22). Bu build'de f16 FA yolu uzun KV'de yavaş kalıyor.
- **16k derinlikte, gpt-oss** (64 boyutlu baş, kayan pencere): nicemlenmiş KV yavaş. q8_0 −%6, K q8_0 + V q4_0 −%14, K-only −%10.
- **Üretim:** kısa bağlamda aynı. 16k'da çoğu türde aynı ya da daha hızlı; yalnız f16 K + q8_0 V tutarlı olarak yavaş (−%6 ile −%13).
- Önceki "q8_0 istem işlemeyi %10-25 yavaşlatıyor" bulgusu (b11236, Hermes3) bu build'de yoğun modellerde tekrarlanmadı.
- Karışık çift (q8_0 / q4_0) Vulkan'da GPU'da çalışıyor; CPU'ya düşme yok.

## Bellek

KV önbelleği (1 slot, MiB). Hesap b11312 load loguyla birebir aynı: Qwen3-4B 128k q8_0'da log 9792, hesap 9792.

| Model | Bağlam | f16 | q8_0 / f16 | q8_0 / q8_0 | q8_0 / q4_0 | q4_0 / q4_0 |
|---|---|---|---|---|---|---|
| Qwen3-4B | 32k | 4608 | 3528 | 2448 | 1872 | 1296 |
| | 128k | 18 432 | 14 112 | 9792 | 7488 | 5184 |
| Qwen2.5-Coder-14B | 32k | 6144 | 4704 | 3264 | 2496 | 1728 |
| | 128k | 24 576 | 18 816 | 13 056 | 9984 | 6912 |
| gpt-oss-20b | 32k | 786 | 602 | 418 | 320 | 221 |
| | 128k | 3090 | 2366 | 1642 | 1256 | 869 |

Süreç belleği (`-c 33792`, MiB):

| Ölçüm | Model | f16 | q8_0/q8_0 | q8_0/q4_0 | q4_0/q4_0 |
|---|---|---|---|---|---|
| GPU belleği, 31k istekten sonra | Qwen3-4B | 7437 | 5337 | 4614 | 4018 |
| | gpt-oss-20b | 12 065 | 11 746 | 11 584 | 11 482 |
| Özel bellek, yükleme anında | Qwen3-4B | 7514 | 5282 | 4687 | 4121 |

Özel bellek GPU belleğini izliyor çünkü AMD Vulkan sürücüsü VRAM'i commit'e de yazıyor (bkz. yukarıdaki "Çıkarım belleği" bölümü).

**Gerçek uçtan uca test** (`kv_policy_e2e_qwen3_4b_128k`): Qwen3-4B, 131 072 bağlam. Otomatik politika q8_0/q8_0 ve `--flash-attn on` seçti. KV 9792 MiB oldu (f16'da 18 432 MiB sığmazdı); süreç VRAM'i 12 519 / 16 368 MiB.

## Karar

1. **Varsayılan f16 kalıyor; otomatik düşürme yalnız sığmadığında.** Bellek yetiyorsa f16 en iyi kalite; kısa bağlamda hız farkı yok, gpt-oss'ta uzun bağlamda f16 daha hızlı. Ayrılan KV bütçenin %90'ı içinde kaldıkça, küçültülerek kazanılacak bellek başka bir işe gitmiyor.
2. **İlk basamak q8_0 / q8_0.**
   - Yoğun modellerde KLD 0,0003-0,0007; aynı en olası token %98,7-99,1.
   - Bulmada kayıp yok; uzun bağlamda istem işleme daha hızlı.
   - gpt-oss'ta KLD ölçülebilir düzeyde (0,046) ama bulma f16 ile aynı.
3. **İkinci basamak K q8_0 + V q4_0 ("Tasarruf"), q4_0 / q4_0 değil.**
   - Aynı modellerde KLD 3,5-8,5 kat düşük: Qwen3 0,0028 / 0,0236; Qwen2.5 0,0013 / 0,0071; gpt-oss 0,083 / 0,299.
   - İkisinin bellek farkı f16'nın %13'ü kadar.
   - Karışık çift Vulkan'da hızlı. CUDA varsayılan derlemesinde karışık çiftler her token'da f16'ya çevrilip %27-38 yavaşlıyor ([CUDA bölümü](#cuda)): CUDA'da otomatik merdiven yalnız eşit çiftlerden oluşuyor (f16 → q8_0/q8_0, sonra daha küçük bağlam; q4_0/q4_0 yalnız elle seçilince). Metal'de karışık çiftler istemi de %87-90 yavaşlatıyor ([Metal bölümü](#metal)); Metal'de de merdiven aynı.
4. **K q8_0 + V f16 (K-only), FA varken kullanılmıyor.**
   - KLD q8_0 / q8_0 ile aynı (0,00075 / 0,00071; gpt-oss 0,046 / 0,046).
   - Tasarrufu yarısı kadar (%23 / %47) ve gpt-oss'ta daha yavaş.
   - Yalnız flash attention açılamadığında (V nicemlenemezken) tek basamak olarak kalıyor.
5. **Sınırlar:** anahtar otomatik olarak hiçbir zaman 4 bite inmiyor; V hiçbir zaman FA olmadan nicemlenmiyor; kullanıcı seçimi ve özel argümanlar her zaman önce geliyor.

## Sınırlar

- Tek makine, tek GPU (RDNA3, Vulkan). ROCm, CUDA ve Metal yalnız aşağıdaki bölümlerde ([ROCm](#rocm), [CUDA](#cuda), [Metal](#metal)); Dashboard'daki hız notu Vulkan, ROCm, CUDA ve Metal'de sayı gösteriyor ("en fazla" değer, gpt-oss 16k).
- GPU ölçüm sırasında başka ajanların ve geliştirme uygulamasının işleriyle paylaşımlıydı. Hız tablosunda en fazla ~%20'lik dalgalanma görüldü (Qwen3 f16 kısa istem 2439-3041). Kalite ve bulma sonuçları bundan etkilenmez.
- KLD 40 960 token ve 4096 bağlamla ölçüldü; 64k ve üstünde ölçülmedi.
- Bulma testi tek sorulu ve 31k'ya kadar. Çok adımlı akıl yürütme ve araç çağrısı doğruluğu ölçülmedi.
- gpt-oss'un KLD'si sohbet şablonu olmadan ham metinde ölçüldü; mutlak değer abartılı.
- Bu makinede Gemma ailesi yoktu; kayan pencere davranışı yalnız gpt-oss ile ölçüldü.


## ROCm

Soru: resmi ROCm build'i bu makinede ne kadar hızlı, KV politikası (karışık K/V çiftleri) ROCm'da da geçerli mi? Aynı kart (RX 7900 GRE 16 GB), llama.cpp **b11320** (b8f96c3e8), `-ngl 99 -fa 1`, `-dev ROCm1` / `-dev Vulkan0` (iGPU dışarıda). ROCm: resmi `win-rocm-10.0-x64` arşivi; arşivde `hipblas.dll` yok, makinedeki HIP SDK 7.1'in `libhipblas.dll` dosyası `hipblas.dll` adıyla kopyalandı ve SDK `bin` klasörü PATH'e girdi (Harness'in yönetilen kurulumu bunu yapamıyor, bkz. [ROADMAP](ROADMAP.md)). Vulkan: Harness'in yönetilen b11320 Vulkan kurulumu. GPU başka işlerle paylaşımlıydı.

**Backend karşılaştırması** (derinlik 0, f16 KV, 3 tekrar, t/sn):

| Model | ROCm pp512 | Vulkan pp512 | ROCm tg128 | Vulkan tg128 |
|---|---|---|---|---|
| Qwen3-4B Q4_K_M | 4568 | 3522 | 127 | 143 |
| gpt-oss-20b MXFP4 | 2294 | 2627 | 98 | 143 |
| Qwen2.5-Coder-14B Q4_K_M | 1347 | 992 | 46 | 51 |

ROCm yoğun modellerde istem işlemede %35-36 hızlı, üretimde %10-11 yavaş; gpt-oss'ta (MoE, MXFP4) iki yönde de geride. Harness'ten (tanım `load.runtime`) başlatılan Qwen3-4B 124 t/sn üretti.

**KV türü, Qwen3-4B** (`-ctk/-ctv`, 2 tekrar, t/sn; yüzde f16'ya göre):

| Ölçüm | f16 | q8_0/f16 | q8_0/q8_0 | q8_0/q4_0 | q4_0/q4_0 |
|---|---|---|---|---|---|
| pp512 | 4608 | 4432 (−%4) | 4389 (−%5) | 4396 (−%5) | 4492 (−%3) |
| pp512 @ 16k | 1707 | 1676 (−%2) | 1642 (−%4) | 1676 (−%2) | 1672 (−%2) |
| tg @ 16k (5 tekrar, ayrı koşu) | 70 | 56 (−%20) | 76 (+%9) | 47 (−%33) | 73 (+%4) |
| aynı koşu, Vulkan tg @ 16k | 84 | 82 | 89 | 91 | 92 |

**gpt-oss-20b** (2 tekrar): pp512 @ 16k f16 2109, q8_0/q8_0 2377, q8_0/q4_0 2331, q4_0/q4_0 2362 (f16'dan hızlı); tg @ 16k f16 105, q8_0/f16 83 (−%21), q8_0/q8_0 98 (−%7), q8_0/q4_0 81 (−%22), q4_0/q4_0 96 (−%8). Derinlik 0'daki f16 pp512 (2328) bir aykırı değer (diğer türler ~3500); paylaşımlı GPU.

Sonuçlar:
- ROCm'da **istem işleme** her KV basamağında f16'nın en fazla %5 gerisinde (`kv_policy::prompt_slowdown_pct` ROCm için 5).
- ROCm'da **üretim**, karışık çiftlerde (q8_0/f16, q8_0/q4_0) uzun bağlamda yavaş yola düşüyor: 16k'da −%20 ile −%33. Eşleşen çiftler (q8_0/q8_0, q4_0/q4_0) f16 hızında. Bu, upstream'in karışık çift notlarıyla uyumlu ([#22411](https://github.com/ggml-org/llama.cpp/discussions/22411)); Vulkan'da etkisi yok.
- **Karar: eşit çift.** ROCm otomatik merdiveni artık CUDA/Metal ile aynı: f16/f16 → q8_0/q8_0, sonra daha küçük bağlam (q4_0/q4_0 sonradan otomatik merdivenden çıkarıldı, bkz. [CUDA bölümü](#cuda)). Dashboard’daki Tasarruf seçimi de bu üç backend’de q4_0/q4_0’dır (KV yaklaşık %28); K’da daha düşük hassasiyet uzun bağlam doğruluğunu azaltabilir. ROCm Compact istem yavaşlama tahmini en fazla %5; elle karışık çiftte üretim için %20-33 uyarısı var. Açık K/V ayarları korunur. Vulkan’da f16/f16 → q8_0/q8_0 → q8_0/q4_0 değişmedi; flash attention kapalıysa tüm backend’lerde yalnız q8_0/f16 basamağı kullanılabilir. Yeni GPU ölçümü yapılmadı.
- CUDA ve Metal de ölçüldü (aşağıda).


## CUDA

Soru: varsayılan CUDA build'inde KV politikası (karışık K/V çiftleri) nasıl davranıyor? Daha önceki bir varsayım "karışık çift FA'yı CPU'ya düşürebilir" idi.

**Ortam:** NVIDIA GeForce RTX 3050 Ti Laptop GPU 4 GB (cc 8.6) + Intel Iris Xe iGPU (Optimus; dGPU ekran sürmüyor, boşta 12 MiB), sürücü 616.92, CUDA 13.4, Windows. llama.cpp **b11327**, resmi `llama-b11327-bin-win-cuda-13.4-x64.zip` + cudart zip'i (`cudart64_13`, `cublas64_13`, `cublasLt64_13` yalnız cudart zip'inden). Model Qwen3-4B-Instruct-2507-Q4_K_M (2,32 GiB). Komut: `llama-bench -fa 1 -ngl 99 -p 512 -n 128 -d 0,8192,16384 -r 3`.

> **Sürücü notu:** 616.92 Studio sürücüsü bu Optimus laptopta GPU boştayken TDR ile çöktü (nvlddmkm Event ID 153, `LKD_0x141_Tdr:6_IMAGE_nvlddmkm.sys_Ampere`, ekran kararması ve bir kez sistem kilitlenmesi). Ölçümler bu sürücüyle sorunsuz tamamlandı; çökmeler llama.cpp yükünden bağımsız. Sonuçlar daha eski bir sürücü sürümüyle tekrarlanmadı.

**KV türü** (pp512 / tg128, t/sn):

| K/V | d=0 | d=8192 | d=16384 |
|---|---|---|---|
| f16/f16 | 2278 / 62,06 | 1534 / 44,27 | 135 / 9,86 (VRAM taştı) |
| q8_0/f16 | 2300 / 60,63 | 1488 / 32,47 | 97 / 1,74 (VRAM taştı) |
| q8_0/q8_0 | 2236 / 60,26 | 1441 / 39,81 | 1058 / 29,88 |
| q8_0/q4_0 | 2230 / 59,09 | 1450 / 28,24 | 1066 / 18,48 |
| q4_0/q4_0 | 2240 / 60,36 | 1459 / 39,53 | 1071 / 29,68 |

- f16/f16 16k'da: model 2,32 GiB + KV ~2,3 GiB 4 GB'ı aşıyor; WDDM taşanı sistem belleğine yazıyor (CPU'ya düşmek değil).
- q8_0/f16 16k'da: f16'ya dönüştürme geçici belleği VRAM'i taşırıyor (tg 1,74 t/sn).

**Bulgular:**
- **Karışık çiftler CPU'ya düşmüyor.** Varsayılan CUDA build'inde karışık çiftler için FA vektör çekirdeği derlenmemiş: llama.cpp her token'da K ve V'yi f16'ya çevirip öyle hesaplıyor. Bu yavaş ve geçici bir f16 arabelleği istiyor. stderr'e şu satır basılıyor (q8_0-f16 ve q8_0-q4_0 için; eşit çiftlerde yok):
  `W ggml_cuda_flash_attn_ext_vec: no FlashAttention vector kernel compiled for K/V types q8_0-f16, converting K and V to f16 instead (slow). Add "q8_0-f16" to GGML_CUDA_FA_QUANTS to compile it.`
- Derleme seçeneği artık `GGML_CUDA_FA_QUANTS` (liste alır); resmi Windows arşivleri karışık çiftleri içermiyor.
- **İstem işleme etkilenmiyor** (derinlik 8192'de f16'dan en fazla %6 geri: 1441-1488 / 1534). **Üretim** karışık çiftlerde derinlikte %27-38 yavaş (d=8192: q8_0/f16 32,5 ve q8_0/q4_0 28,2 t/sn, f16 44,3; d=16384: q8_0/q4_0 18,5, q8_0/q8_0 29,9). Eşit çiftler d=8192'de f16'nın ~%10 gerisinde (39,5-39,8 / 44,3 t/sn) ve f16'nın taştığı 16k'da tek hızlı yol (29,7-29,9 t/sn).

**Sonuç ve uygulama:**
- CUDA'da "Otomatik" merdiveni yalnız eşit çiftler: f16/f16 → q8_0/q8_0 (`kv_policy::CUDA_LADDER`); q8_0/q8_0 sığmazsa bağlam küçültülür. q4_0/q4_0 (`KvLevel::Compact`) hiçbir zaman otomatik seçilmez: b11342'de Qwen3-0.6B'nin 8k ve 32k çıktısını bozdu (f16 ve q8_0/q8_0 doğru yanıtladı) ve bir Qwen3-4B araç çağrısını karıştırdı; elle Tasarruf seçimi olarak kalır. ROCm da aynı eşit çift merdivenine geçti; Vulkan değişmedi. Karışık çift yalnız kullanıcı açıkça seçerse kullanılıyor; bellek tahmini bu durumda dönüştürme arabelleğini ekliyor (`kv_policy::cuda_convert_buffer_bytes`: bir katmanın nicemli taraflarının f16 boyutu; Qwen3-4B 16k'da q8_0/q4_0 için 64 MiB, q8_0/f16 için 32 MiB).
- Sunucu günlüğündeki yukarıdaki satır tanınıyor (`capabilities::verify::kv_fa_fallback`): sunucu günlüğüne çalıştırma başına bir kez yerelleştirilmiş uyarı düşüyor ("Eşit bir çift kullanın"), Dashboard'ın "Etkin Değerler" şeridinde "Yavaş KV çifti" gösteriliyor.
- `prompt_slowdown_pct` CUDA için: q8_0/f16 %3, q8_0/q8_0 %6, q8_0/q4_0 %6, q4_0/q4_0 %5 (d=8192, en kötü).
- Bellek tahmini bu karta uyuyor: q8_0 16k için 3847 MiB (ölçülen kullanım 3813), q4_0 32k için 3935 MiB (3905); f16 16k 4927 MiB ve q8_0 32k 5087 MiB tahmin ediliyor, yani 4 GB'ı aşan iki durum önceden görülüyor (`cuda_4gb_card_avoids_the_overflowing_combinations`). Otomatik seçim bütçenin %90'ını (3686 MiB) aşmamaya çalıştığı için o zamanki merdivenle 16k'da q8_0 (3847 MiB) yerine q4_0/q4_0 seçiyordu (bugün q8_0/q8_0'da kalıp bağlamı kısaltıyor); ölçüm q8_0'ın 16k'da da f16 hızında çalıştığını gösteriyor, yani bu kart için temkinli. İsteyen "Dengeli"yi elle seçebilir.

## Metal

Soru: KV politikası (karışık K/V çiftleri) Apple Silicon'da Metal ile nasıl davranıyor? Roadmap'te "KV politikası Metal'de ölçülmedi" açıktı.

**Ortam:** MacBook Air M1 (8 çekirdek, 8 GB birleşik bellek; Metal payı `--list-devices` ile MTL0 5461 MiB), macOS 27.0.1. llama.cpp **b11344**, resmi `llama-b11344-bin-macos-arm64.tar.gz` (Harness'ın yönettiği sürüm). Model Qwen3-4B-Instruct-2507-Q4_K_M (2,32 GiB, unsloth). Komut: `llama-bench -ngl 99 -fa 1 -d 4096 -p 512 -n 128 -r 1 -ctk <K> -ctv <V>`. 8 GB'ta d=8192 tek çift ~10 dakika sürdüğü için ölçüm 4096 derinlikte ve tek tekrarla yapıldı; Harness açık, sunucu kapalı.

**KV türü** (pp512 / tg128 @ d=4096, t/sn):

| K/V | pp512 | tg128 | f16'ya göre |
|---|---|---|---|
| f16/f16 | 53,31 | 7,56 | — |
| q8_0/f16 | 7,02 | 3,25 | istem −%87, üretim −%57 |
| q8_0/q8_0 | 59,23 | 6,81 | istem +%11, üretim −%10 |
| q8_0/q4_0 | 5,43 | 3,01 | istem −%90, üretim −%60 |
| q4_0/q4_0 | 60,45 | 5,91 | istem +%13, üretim −%22 |

Ek: f16/f16 d=8192'de pp512 44,84 ± 3,08, tg128 6,50 ± 0,12 (r=2).

**Bulgular:**
- **Karışık çiftler Metal'de yavaş yola düşüyor:** hem istem işleme (%87-90) hem üretim (%57-60) çok geride. CUDA'dakinden farklı olarak istem işleme de etkileniyor; stderr'de CUDA'daki gibi bir uyarı satırı yok.
- **Eşit çiftler hızlı:** q8_0/q8_0 ve q4_0/q4_0 istem işlemede f16'dan hızlı (daha az bellek okuması), üretimde %10-22 geride.

**Sonuç ve uygulama:**
- Metal'de (Apple Silicon: birleşik bellek bütçesi, tümleşik GPU payı değil) "Otomatik" merdiveni CUDA ile aynı: f16/f16 → q8_0/q8_0, sonra daha küçük bağlam (`kv_policy::CUDA_LADDER`). Intel/AMD tümleşik GPU'lar (Vulkan, paylaşılan RAM) eski merdivende kalıyor.
- `prompt_slowdown_pct` Metal için: q8_0/f16 %87, q8_0/q8_0 %0, q8_0/q4_0 %90, q4_0/q4_0 %0; üretim kaybı `METAL_MIXED_GENERATION_SLOWDOWN_PCT` (57, 60). Dashboard Tasarruf seçimi CUDA/Metal/ROCm için q4_0/q4_0 oldu; 4 bit ipucu anahtar hassasiyeti kaybını açıklar.
- Sınır: tek makine (M1, 8 GB), tek model, tek tekrar, 4k derinlik. Daha büyük Apple Silicon'larda (M2/M3/M4 Pro/Max) mutlak sayılar farklı olur; karışık çiftin yavaş yola düşmesi build'in çekirdek desteğine bağlı olduğu için yön aynı beklenir.

## Eşzamanlı istekler ve bellek tahmini, Metal

Aynı M1 8 GB, managed runtime b11347 (macos-arm64), Qwen3-4B-Instruct-2507 Q4_K_M. Komut: `llama-server -ngl 99 -fa on -c 16384 -np <slot>` (KV f16). Her istek `/completion`, 898 token istem (isteğe göre farklı önek), `n_predict 256`, `ignore_eos`, `cache_prompt false`. "Toplam" = üretilen tüm token / duvar saati (istem işleme dahil); "istek başına" = `timings.predicted_per_second` ortalaması.

| Slot | İstek | Toplam t/sn | İstek başına t/sn | Duvar saati |
|---:|---:|---:|---:|---:|
| 1 | 1 | 4,34 | 5,95 | 58,9 sn |
| 1 | 4 (sıraya girdi) | 4,17 | 5,73 | 245,4 sn |
| 2 | 2 | **5,49** | 3,83 | 93,3 sn |
| 4 | 2 | 4,02 | 2,56 | 127,4 sn |
| 4 | 4 | 5,02 | 1,86 | 204,0 sn |

- **M1'de slot kazancı küçük:** en iyi durumda 2 slot tek slota göre 1,26× (CUDA'da 4 slot 2,3×, Vulkan'da 1,8×). 8 çekirdekli GPU hesapta doyuyor; birlikte üretilen her istek orantılı yavaşlıyor. 4 slot 2 istekte 2 slottan yavaş çıktı (tek ölçüm; ısıl kısıtlama dışlanmadı).
- Tek istek üretimi 16k f16 KV'de 5,95 t/sn (4k'da Playground'da 9,3 t/sn ölçülmüştü): uzun KV ve 898 tokenlık istem üretimi yavaşlatıyor.
- Varsayılan (`-np` otomatik, birleşik KV) M1'de zarar vermiyor ama kazanç da az; büyük Apple Silicon'larda (daha çok GPU çekirdeği) ölçülmedi.

**Bellek tahmini ve gerçek** (`-lv 4` günlüğündeki tampon boyutları; Metal = model + KV + hesap tamponu, toplam = Metal + CPU tamponları (304 MiB CPU_Mapped gömme, çıktı, CPU hesap)):

| Bağlam / KV | Dashboard tahmini (VRAM) | Gerçek Metal | Gerçek toplam |
|---|---:|---:|---:|
| 4k f16 | 3,11 GB | 3028 MiB (2,96 GiB) | 3349 MiB |
| 8k f16 | 3,68 GB | 3624 MiB (3,54 GiB) | 3945 MiB |
| 16k q8_0/q8_0 (otomatik seçim) | 3,76 GB | 3722 MiB (3,63 GiB) | 4043 MiB |
| 16k f16 (elle) | — | 4816 MiB | 5137 MiB |

- Tahmin her bağlamda ~%4 ihtiyatlı (gerçeğin 0,13-0,15 GiB üstünde); taşma riski yok. 16k'da Dashboard KV'yi kendiliğinden q8_0/q8_0 seçiyor (f16 5,3 GB bütçeye sığardı ama pay bırakmıyor).
- `phys_footprint` (4k: 671 MB) `mmap` edilmiş ağırlıkları saymadığı için gerçek kullanımı göstermiyor; ölçüt olarak günlük kullanıldı.

## Eşzamanlı istekler, CUDA

Aynı makine ve build (b11327). Komut: `llama-server -ngl 99 -fa on -c <bağlam> -np <slot> -ctk/-ctv <tür>`. Her istek `/completion`, 796 token istem, `n_predict 256`, `ignore_eos`, `cache_prompt false`. "Toplam" = üretilen tüm token / duvar saati; "istek başına" = `timings.predicted_per_second` ortalaması. VRAM: `nvidia-smi` (WDDM'de ~3,9 GB'ta kalıyor; taşma yalnız yavaşlama olarak görünür).

| Bağlam / KV | Slot | İstek | Toplam t/sn | İstek başına t/sn | VRAM MiB |
|---|---:|---:|---:|---:|---:|
| 32k q4_0/q4_0 | 1 | 1 | 51,08 | 55,57 | 3905 |
| | 1 | 4 (sıraya girdi) | 51,45 | 55,39 | |
| | 2 | 2 | 88,15 | 48,93 | 3901 |
| | 4 | 2 | 88,42 | 49,08 | |
| | 4 | 4 | **117,69** | 34,52 | 3893 |
| 32k q8_0/q8_0 (taşıyor) | 1 | 1 | 24,94 | 31,31 | 3921 |
| | 2 | 2 | 31,10 | 19,16 | |
| | 4 | 4 | 36,17 | 11,64 | |
| 16k q8_0/q8_0 | 1 | 1 | 51,05 | 55,25 | 3813 |
| | 2 | 2 | 88,76 | 49,34 | |
| | 4 | 2 | 88,73 | 49,28 | |
| | 4 | 4 | **117,53** | 34,54 | 3801 |
| 16k f16/f16 (taşıyor) | 1 | 1 | 35,54 | 45,02 | |
| | 2 | 2 | 49,29 | 32,35 | |
| | 4 | 4 | 60,73 | 20,99 | 3921 |

- **Kazanç KV VRAM'e sığınca gerçek:** 2 slotta 1,73×, 4 slotta 2,3× toplam hız (51 → 118 t/sn). Taşan yapılandırmalarda (32k q8_0, 16k f16) slot kazancı yarıya iniyor ve mutlak hız da düşüyor (4 slotta 36 ve 61 t/sn).
- Slottan fazla istek (np 1, 4 istek) sıraya giriyor; toplam hız tek isteğinkiyle aynı.
- Bu denemede sunucu `kv_unified=false` başladı: `n_ctx_slot = bağlam / slot` (32k ve 4 slot: istek başına 8k). Harness `-np N > 1` için `--kv-unified` gönderiyor (`slots::plan_for_launch`), yani istekler tüm havuzu kullanabilir; bu tablo birleşik havuzla ayrıca ölçülmedi (toplam KV belleği aynı).
- 4 GB kartta öneri: 32k için q4_0/q4_0, 16k için q8_0/q8_0.

## Tam GPU yüklemesi ve `-ngl` önerisi, CUDA

Aynı makine, sürücü 616.92 (Studio), uygulamanın kurduğu yönetilen runtime (b11327 **win-cuda-12.4** + cudart 12.4). Uygulamadan başlatıldı: Qwen3-4B Q4_K_M, 16k, `-np 2 --kv-unified`, `--flash-attn on`. 2 eşzamanlı istek (`/completion`, `n_predict` 400-800, `ignore_eos`). VRAM: PDH `GPU Process Memory` (süreç).

| `-ngl` | KV | Ayrılmış VRAM MiB | Paylaşılan MiB | İstek başına t/sn | Toplam t/sn |
|---:|---|---:|---:|---:|---:|
| 33/37 (eski öneri) | q8_0/q8_0 | 3475 | – | 35,1 | 70,7 |
| 37/37 | q8_0/q8_0 | 3801 | 106 | **54,9** | ~110 |
| 37/37 (Otomatik KV) | q4_0/q4_0 | 3225 | 106 | 57,6 (tek istek) | – |

- Tam yükleme kartın %96'sını (3801 / 3962 MiB, DXGI) kullanıyor ama paylaşılan belleğe taşmıyor: paylaşılan kullanım -ngl 33'tekiyle aynı (106 MB). Hız istek başına %56 yüksek.
- Öneri `vram::recommend_gpu_layers` ile kartın **%90**'ında kesiliyordu (CUDA'nın `--list-devices` "free" değeri, 3295 MiB, öneriye girmiyor; yalnız cihaz listesinde ve gateway bellek kontrolünde). Ölçüm kaydı çalışıyor (`vram_measurements`, varsayılan günlük düzeyinde yalnız süreç toplamı) ve öneriyi düzeltiyor: Otomatik KV'de tam yükleme ölçülünce öneri 33 → 37 (o zamanki merdivenle q4_0/q4_0 seçerek). Elle q8_0'da ölçülmüş 3801 MiB yine %90'ı aştığı için öneri 36'da kalıyordu.
- **Değişiklik:** aynı model, bağlam ve KV tipleriyle tam GPU'da alınmış bir ölçüm varsa, onunla düzeltilmiş tam yükleme %97'ye kadar kabul (`vram::measured_full_offload`; slot sayısı farklı olabilir). Formda q8_0, 16k, -ngl 33 iken artık "Önerilen -ngl: 37". Ölçüm yoksa ya da %97'yi aşıyorsa eski kural; körlemesine yükseltme yok (WDDM sessizce paylaşılan belleğe taşar).

## Spekülatif çözümleme (taslak model), CUDA

Aynı makine (RTX 3050 Ti Laptop 4 GB, sürücü 616.92), yönetilen runtime b11333 win-cuda-12.4. Hedef Qwen3-4B-Instruct-2507 Q4_K_M, taslak Qwen3-0.6B Q8_0 (resmi `Qwen/Qwen3-0.6B-GGUF`, 610 MiB). Aynı tokenizer ailesi; `library::draft` önerisine uyuyor. llama-server doğrudan: `-ngl 99 -np 1 -fa on --spec-type draft-simple`, taslak `-md … -ngld 99`. Üç istem (kod, açıklama, çeviri + liste), her biri 400 token, `temperature 0`. VRAM: PDH `GPU Process Memory` (süreç). Kabul: `timings.draft_n_accepted / draft_n`.

| Bağlam | Yapılandırma | Ayrılmış / paylaşılan MiB | t/sn (ort.) | Kabul |
|---|---|---:|---:|---:|
| 2k | taslaksız | 2833 / 92 | 59,7 | – |
| 2k | taslak GPU, `--spec-draft-n-max 3` (varsayılan) | 3695 / 100 | 56,4 | %51 |
| 2k | taslak GPU, n-max 2 | 3695 / 100 | 54,9 | %57 |
| 4k | taslaksız | 3123 / 94 | 59,8 | – |
| 4k | taslak GPU, n-max 3 | 3923 / **392** | 46,2 | %51 |
| 4k | taslak GPU, n-max 8 | 3923 / 392 | 34,2 | %29 |
| 8k | taslaksız | 3703 / 98 | 59,4 | – |
| 8k | taslak GPU | açılmıyor | – | – |
| 8k | taslak CPU (`-ngld 0`) | 3775 / 114 | 18,0 | %51 |

- **Bu kartta taslak kazandırmıyor.** 4B zaten 60 t/sn. 0.6B taslağın her adımı (3 token) neredeyse bir hedef adımı kadar sürüyor, kabul %51 (Qwen3-0.6B düşünen model, hedef 2507 Instruct; ortalama kabul uzunluğu 2,4-2,7). Taşma yokken (2k) başa baş (−%6), taşınca (4k) −%23, taslak CPU'da −%70. Uzun taslak (n-max 8) kabulü %29'a düşürüyor. Kazanç beklenen durum, hedefin yavaş olduğu büyük model + küçük taslak: Windows/Vulkan makinesinde Qwen2.5-Coder-14B + 0.5B ile 46 → 89 t/sn ölçülmüştü (daha önceki bir ölçüm). Harness'in tanımıyla (`draft_model_path`, `draft_gpu_layers` 99) kısa ve tahmin edilebilir yanıtta ("1'den 10'a say") 21/21 taslak kabul, 61,8 t/sn.
- **Bellek:** taslak = ağırlıkları + hedefin bağlamında kendi KV önbelleği (b11333'te ayrı taslak bağlamı yok, `-cd` kaldırılmış) + hesap tamponu. Ölçülen ek: 2k'da 862 MiB, 4k'da 1098 MiB. 8k'da hedef tam GPU'dayken taslak (KV q8_0'la bile) sığmıyor ve llama.cpp bunu bellek hatası olarak değil `error loading model: invalid vector subscript` / `failed to load draft model` olarak bildiriyor. `-ngld 0` ya da hedefte `-ngl 30` ile açılıyor.
- **Tahmin düzeltildi:** VRAM tahmini taslağı hiç saymıyordu (`FitInput` taslaksızdı). Artık `FitInput.draft` (yol, boyut, `-ngld`, KV tipleri) var. Taslak, hedefin bağlam hücreleri ve slotlarıyla ayrı bir model gibi hesaplanıyor (`vram::add_draft`); `MemoryEstimate.draft_gpu_bytes` / `draft_host_bytes` sonuçta. Önerilen `-ngl` taslağın payı düşülmüş bütçeyle hesaplanıyor. Gerçek dosyalarla: tahmin 2k'da 860 MiB (ölçülen 862), 4k'da 1086 MiB (ölçülen 1098). Önerilen hedef `-ngl` 2k'da 36, 4k'da 32 (taslaksız 37).

## `--disable-gpu` kullanımda CPU

Ayrık debug örneği (`tauri build --debug --no-bundle`, ayrı `HARNESS_DATA_DIR` ve `WEBVIEW2_USER_DATA_FOLDER`), iki llama-server (qwen2.5 0.5B, 4k bağlam, Vulkan), pencere 1000×661 CSS px, ekran ölçeği %115. CPU, uygulama süreci ve tüm WebView2 alt süreçlerinin (llama-server hariç) işlemci zamanı farkından, bir çekirdeğe göre. Kaydırma: CDP ile 50 ms'de bir 120 px tekerlek olayı (5 adımda yön değişiyor), 15 sn. Akan yanıt: Playground'da yeni sohbet, ~1460 token, 6 sn pencere; 1000 token başına CPU-sn. GPU açık tur iki kez yapıldı; ilki aynı anda süren bir cargo derlemesiyle çakıştı, tabloda temiz tur var (çakışan turun akış değerleri 2,13-2,24).

| Senaryo | GPU açık | `--disable-gpu` |
|---|---|---|
| Dashboard, iki sunucu, canlı grafikler, boşta (20 sn) | %1,3 | %0,9 |
| Kaydırma (2 × 15 sn) | %19,7 / %15,5 | %22,3 / %20,3 |
| Akan yanıt, CPU-sn / 1000 token (3 tur) | 2,17 / 1,68 / 1,77 | 2,64 / 2,03 / 1,83 |
| GPU süreci | 112 MiB | 52 MiB |
| Çizici (renderer) | 134 MiB | 187 MiB |

- **Karar: açılmadı.** Boşta kazanç (önceki ölçümde 42 MiB) kullanımda çizicinin büyümesiyle kapanıyor; kaydırma ve akan yanıtta CPU %10-20 artıyor. Fark grafikli boşta gürültü içinde.
- Sınır: tek makine (RX 7900 GRE, Windows 11), debug derleme, kısa pencereler; akan yanıt ölçümü model hızına (~370 t/sn) bağlı.

## Gateway JIT/TTL/LRU, CUDA

Makine: RTX 3050 Ti Laptop 4 GB (WDDM, sürücü 617.42) + Iris Xe, llama.cpp b11456 CUDA 12 (`cudart64_12`), Harness debug derlemesi, ayrık veri dizini; gateway 11635. İstemci Node `fetch` (OpenCode gibi dış istemci), her model tanımı varsayılanlarla (bağlam 4096, `-ngl` otomatik).; koşular arası t/sn değişkenliği yüksek (dizüstü güç/saat durumu; 1,5B, 3B ve Llama değerleri 8 token'lık kısa yanıtların `timings` alanından, 0,5B 120 token'dan), aralıklar 5 koşudan.

| Model (tanım) | JIT yükleme (`x-harness-load-ms`) | Üretim (t/sn) | Yüklüyken VRAM (nvidia-smi, toplam) | Tahliye (`x-harness-evicted`) |
|---|---|---|---|---|
| Qwen2.5-0.5B Q4_K_M | 3,1-7,0 sn | 215-242 (akış 232) | 553 MiB | Llama Q6 isteğiyle: `llama-3b-q6` (en eski) |
| Qwen2.5-1.5B Q4_K_M (0,5B yüklüyken) | 3,9-5,0 sn | 39-74 | 1762 MiB (0,5B ile) | Llama Q6 isteğiyle: `qwen-1-5b,qwen-3b` |
| Qwen2.5-3B Q4_K_M (0,5B+1,5B yüklüyken) | 3,3-8,0 sn | 38-56 | 3368 MiB (1,5B ile) | `qwen-0-5b` (en eski, 3B için yer açtı) |
| Llama-3.2-3B Q6_K (hepsi yüklüyken) | 8,9-12,2 sn | 44-52 | 3131 MiB (tek) | `qwen-1-5b,qwen-3b` (iki model, sırayla en eski) |
| 0,5B + 3B birlikte (düzeltmeli) | 3,2 sn (3B) | 51 | 2712 MiB | yok |

- **Tahmin ölçüme çok yakın:** Llama Q6_K için gateway "yaklaşık 3130 MiB" dedi; yüklü ölçüm 3131 MiB. 3B için 2158 MiB tahmin, tek başına 2159 MiB ölçüm.
- **Sabitlenmiş 3B** yüklüyken Llama Q6 isteği `503 insufficient_memory`: "needs about 3130 MiB, 1932 MiB free; loaded and not evictable: qwen-3b" (`Retry-After: 30`). Boşta 0,5B bu denemede tahliye edildi ama yine de yetmiyordu.
- **TTL:** tanımda 15 sn; son istekten 16,1 sn sonra boşaldı, VRAM 553 → 0 MiB. "Boşalt" düğmesi 1,5B'yi anında boşalttı (durum "Yüklü değil").
- **e2e süreleri (3B, ctx 4096-8192):** `e2e_jit_stream_ttl_embed_rerank` JIT 4,2 sn, akış 172 parça (ilk 25 ms, toplam 2,24 sn ≈ 77 t/sn), bellek baskısı testinde 3B → 1,5B yükü 5,6 sn ve `gw-big-a` tahliyesi; `admission_e2e` 2,06 sn ve kuyrukta 2049 ms bekleyen 4,23 sn; `split_e2e` iki akış 160 token 2,32 sn (~69 t/sn); `serve_mode::e2e` taslak (0,5B → 3B) 22 taslak token/21 kabul, 86,7 t/sn; embedding 768 boyut. Test boyunca azami VRAM: gateway `e2e` 3908 MiB (oom testi, `--fit` ile spill), diğerleri 2159-2627 MiB.
- **RAG (nomic-embed Q4_K_M + bge-reranker-v2-m3 Q4_K_M):** 138 parça indeksleme (embed JIT dahil) 6,1 sn, rerank (JIT dahil) 7,0 sn. 3B yanıtı 5,1 sn, doğru içerik.
- **Sığmadı (ölçülen):** Qwen3-4B-Instruct-2507 Q4_K_M sohbet modeli 8192 bağlamda embed+rerank tahliye edildikten sonra bile `503 insufficient_memory` (tahmin ~4,1 GB). Llama Q6_K 8192 bağlam, 3B yüklüyken: "needs about 3639 MiB, 4091 MiB free" (bellek farkındalığı açıkken 512 MiB pay dahil).
- **Playground (3B, gateway SSE):** ~75 t/sn arayüz sayacı; ilk token 60-680 ms (istem önbelleği); `exec_shell_command` onayı sonrası ikinci tur 0,7 sn.
- **Bulgu (düzeltmesiz uygulama):** 3 sn'lik cihaz raporu önbelleği nedeniyle 0,5B yüklüyken 3B isteği yanlış `503 (1512 MiB free)` verdi ve 0,5B'yi tahliye etti; düzeltmeyle  200 ve tahliyesiz.
## whisper.cpp CUDA ve aynı kartta LLM

RTX 3050 Ti Laptop 4 GB, sürücü 617.42, Windows 11, whisper.cpp b5454 CUDA 12.4 (uygulamanın indiricisi), 11 sn'lik gerçek konuşma (`samples/jfk.wav`), `whisper-server` `/inference`, tek istek, 4 ardışık istek (ilk / sıcak). VRAM: süreç başına ayrılmış bellek (nvidia-smi toplam, boşta 0 MiB). Ölçüm sırasında makinede başka ajanların derlemeleri çalışıyordu (CPU süreleri bundan etkilenmiş olabilir; model yükleme süreleri dosya önbelleğine bağlı: 3 sn ile 114 sn arası).

| Model | CUDA ilk / sıcak (sn) | CPU sıcak (sn) | Hız | VRAM (MiB) | `whisper_vram_need` önce / sonra (MiB) |
|---|---|---|---|---|---|
| ggml-base.en | 0,50 / 0,32 | 1,10 | 3,5x | 411 | 333 / 461 |
| ggml-small.en | 0,71 / 0,41 | 3,55 | 8,7x | 805 | 657 / 785 |
| ggml-medium.en | 0,75 / 0,63 | 10,8 | 17x | 1945 | 1828 / 1956 |
| ggml-large-v3-turbo-q5_0 | 3,45 / 0,48 | 14-15 | 30x | 893 | 739 / 867 |

- **CUDA 11.8 arşivi GPU kullanmıyor:** `whisper-bin-win-cuda-11.8.0-x64.zip` içinde `cublas64_11.dll` yok; VRAM 0 MiB, CPU hızı. 12.4 arşivi (675 MB) cuBLAS dahil çalışıyor. Seçici artık 12.4'ü alıyor.
- **Aynı kartta (Qwen2.5-3B Q4_K_M 16k = 2602 MiB, 0.5B = 552 MiB, whisper medium.en = 1944 MiB / turbo = 892 MiB):** medium.en ile 3B + 0.5B + whisper = 5,1 GB sığmıyor. Sıra 3B, 0.5B, whisper isteği: gateway önce en eski modeli (3B) çıkardı, whisper yüklendi (2498 MiB kullanım). Ardından 3B isteği: 0.5B ve whisper çıkarıldı (2603 MiB), ardından whisper isteği 3B'yi çıkardı (1945 MiB). turbo ile (düzeltmeden sonra): 3B isteği yalnız 0.5B'yi çıkardı, whisper kaldı (3496 / 4096 MiB), whisper isteği 325 ms.
- **Sabitlenmiş 3B:** whisper isteği 503 `insufficient_memory` ("needs about 1828 MiB, 1488 MiB free", nvidia-smi 2603 MiB), 3B yerinde, 503 506 ms.
- **Uygulamada (Playground > Konuşma, dosya bırakma):** medium.en ilk yükleme + yazıya döküm 4,7 sn (dosya önbellekte; soğuk 52 sn), sıcak 0,59 sn; small.en geçişi 7,6 sn; iki model birlikte 2750 MiB.

## NVIDIA GPU seçimi ve VRAM telemetrisi

RTX 3050 Ti Laptop 4 GB (WDDM, sürücü 617.42) + Intel Iris Xe (Optimus), Windows 11. Uygulamanın kendi indirdiği llama.cpp b11445 (win-cuda-13.4 + cudart, win-vulkan; o zamanki seçiciyle, bugün ikisi de varsa 12.4 derlemesi seçilir), ayrık veri klasörü. Tüm sunucular uygulamadan başlatıldı, f16 KV, FA açık, `-np 1`; hız `/completion` 128 token.

**Cihaz listesi.** CUDA: yalnız `CUDA0` RTX (Iris Xe görünmüyor). Vulkan: `Vulkan0` Iris Xe (7,9 GB paylaşılan), `Vulkan1` RTX. Otomatik seçim iki model motorunda da RTX, `--main-gpu 0` (ayrık cihazlar arasındaki sıra; DXGI 1 → 0). Iris Xe: CUDA'da seçenek devre dışı, elle seçimde "Seçilen GPU bu model motorunda kullanılamıyor: Intel(R) Iris(R) Xe Graphics (adaptör 0)"; Vulkan'da da aynı (ayrık GPU varken llama.cpp iGPU'yu yüklemiyor).

**VRAM: sunucu payı (PDH `GPU Process Memory`) ve nvidia-smi `memory.used` (MiB).**

| Model | Bağlam | -ngl | nvidia-smi | PDH sunucu payı | PDH kart toplamı | t/sn |
|---|---:|---:|---:|---:|---:|---:|
| Qwen2.5-0.5B Q4_K_M | 8k | 99 | 605 | 605 | 609 | 199-229 |
| Qwen2.5-0.5B Q4_K_M | 16k | 99 | 709 | 709 | 713 | 229 |
| Qwen2.5-1.5B Q4_K_M | 8k | 99 | 1325 | 1325 | 1329 | 85-123 |
| Qwen2.5-1.5B Q4_K_M | 16k | 99 | 1557 | 1557 | 1561 | 118-125 |
| Qwen2.5-3B Q4_K_M | 8k | 99 | 2307 | 2307 | 2311 | 75-77 |
| Qwen2.5-3B Q4_K_M | 16k | 99 | 2603 | 2603 | 2607 | 75-77 |
| Llama-3.2-3B Q6_K | 4k | 99 (29/29) | 3131 | 3131 | 3135 | 60-61 |
| Llama-3.2-3B Q6_K, `-np 2` | 16k | 99 | 3901 | 3902 | 3906 | 56 (kart dolu: tahmin 4393-4487 MiB, öneri -ngl 23-24) |
| Qwen2.5-0.5B, Vulkan | 8k | 99 | 522 | 521 | 525 | 191-195 |

- Sunucu payı nvidia-smi ile aynı MiB; kart toplamı +4 MiB (+%0,1-0,6, başka süreçler). Hız dalgalanması aynı anda derleme yapan başka süreçlerden.
- **NVML (WDDM, 617.42):** `nvmlDeviceGetComputeRunningProcesses_v3` başarılı, ama llama-server girişi `usedGpuMemory = NVML_VALUE_NOT_AVAILABLE`; nvidia-smi `[N/A]`. Windows'ta süreç payı PDH'den.
- **İki sunucu:** iki Qwen2.5-0.5B 8k (11792 ve 11793) birlikte, RTX çubuğunda 605 + 605 MiB ayrı pay; Iris Xe 0. İkisi durdurulunca tüm paylar 0.
- **-ngl önerisi (soğuk tahmin):** Llama-3.2-3B Q6_K 4k: tahmin 3187, ölçülen 3131 MiB, tam yükleme önerisi 29/29 ("full_gpu"). Qwen2.5-0.5B 8k: tahmin 521, ölçülen 605 MiB (küçük modelde CUDA bağlamı eksik, zararsız). Llama `-np 2` 16k kartı doldurdu (3901 / 3962 MiB, "oom_risk"), yine de 56 t/sn ile çalıştı; WDDM fazlasını paylaşılan belleğe taşıyor.
- **Yanlış pozitif:** "boş cihaz belleğine sığdıramadı" uyarısı `-ngl` verilince llama-server'ın her başlatmada yazdığı "already set by user" satırından geliyordu (605 MiB'lik modelde de); düzeltildi.

**14B (Qwen2.5-14B Q4_K_M, 8k) yalnız kısmi offload** (tam yükleme 4 GB'a sığmaz; Dashboard uyarısı "Tahmini 5,75-6,9 GB gerekiyor, ayrılmış VRAM 3,87 GB", önerilen -ngl 17-29, öneri her ölçümden sonra düşüyor):

| -ngl (/48) | nvidia-smi MiB | t/sn |
|---:|---:|---:|
| 29 | 3899 | 4,0 |
| 25 | 3883 | 4,5 |
| 21 | 3747 | 4,2 |
| 17 | 3815 | 4,6 |
| 13 | 3171 | 4,1 |
| 9 | 2429 | 3,9 |

- 17 ve üzeri katmanda kart dolu (3,75-3,9 GB, tavan 3962) ve hız kısmi offload'la ~4 t/sn'de kalıyor; katman sayısı hızı belirgin değiştirmiyor. İstem işleme 8-12 t/sn.


## Hibrit modeller slot başı durum, CUDA

Soru: Qwen3.5 gibi hibrit modellerde (gated delta net katmanları + her 4. katmanda dikkat) `vram.rs`'nin özyineli durum (`recurrent_bytes_per_seq`) ve KV tahmini gerçek CUDA yüklemesiyle uyuşuyor mu; slot başına maliyet ve eşzamanlı kazanç ne?

**Ortam:** RTX 3050 Ti Laptop 4 GB (WDDM, sürücü 617.42, CUDA 13.4) + Intel Iris Xe (Optimus), 15,8 GB RAM, Windows 11. llama.cpp **b11456** (resmi CUDA 12 arşivi, uygulamanın kendi indiricisiyle kurulmuş yönetilen runtime), `-ngl 99 -fa on -lv 4`, KV f16. Modeller unsloth Q4_K_M (0.8B 508 MB, 2B 1,19 GB, 4B 2,55 GB) ve 4B Q8_0 (4,17 GB). Tahmin: `vram::estimate_raw` (GGUF başlığından, ölçüm düzeltmesi yok); gerçek: llama-server `llama_kv_cache` / `llama_memory_recurrent` / `sched_reserve` tampon satırları ve `nvidia-smi` toplamı (boştaki 0 MiB'a göre). GGUF başlıkları: 0.8B 24 katman (6 dikkat + 18 özyineli), 2B 24 (6 + 18), 4B 32 (8 + 24); `ssm.conv_kernel 4`, `state_size 128`, `inner_size` 2048 / 2048 / 4096, `group_count 16`, `full_attention_interval 4`, dikkat başı genişliği 256.

### Tahmin ve ölçülen (MiB)

| Model | -np | Bağlam | KV tahmin / ölçülen | Durum (RS) tahmin / ölçülen | Compute tahmin / ölçülen | Toplam VRAM tahmin / nvidia-smi | Fark |
|---|---:|---:|---:|---:|---:|---:|---:|
| 0.8B | 1 | 4k | 48 / 48 | 19,3 / 19,27 | 32 / 37 | 757 / 695 | +8,9% |
| 0.8B | 1 | 16k | 192 / 192 | 19,3 / 19,27 | 44 / 49 | 913 / 851 | +7,3% |
| 0.8B | 1 | 32k | 384 / 384 | 19,3 / 19,27 | 60 / 65 | 1121 / 1059 | +5,9% |
| 0.8B | 2 | 16k | 192 / 192 | 38,5 / 38,53 | 44 / 50 | 932 / 873 | +6,8% |
| 0.8B | 4 | 4k | 48 / 48 | 77,1 / 77,06 | 32 / 48 | 815 / 765 | +6,5% |
| 0.8B | 4 | 16k | 192 / 192 | 77,1 / 77,06 | 44 / 60 | 971 / 921 | +5,4% |
| 0.8B | 4 | 32k | 384 / 384 | 77,1 / 77,06 | 60 / 76 | 1179 / 1129 | +4,4% |
| 2B | 1 | 4k | 48 / 48 | 19,3 / 19,27 | 52 / 56 | 1491 / 1409 | +5,8% |
| 2B | 1 | 16k | 192 / 192 | 19,3 / 19,27 | 64 / 68 | 1647 / 1585 | +3,9% |
| 2B | 1 | 32k | 384 / 384 | 19,3 / 19,27 | 80 / 84 | 1855 / 1793 | +3,4% |
| 2B | 2 | 16k | 192 / 192 | 38,5 / 38,53 | 64 / 68 | 1666 / 1605 | +3,8% |
| 2B | 4 | 4k | 48 / 48 | 77,1 / 77,06 | 52 / 56 | 1549 / 1487 | +4,1% |
| 2B | 4 | 16k | 192 / 192 | 77,1 / 77,06 | 64 / 68 | 1705 / 1643 | +3,7% |
| 2B | 4 | 32k | 384 / 384 | 77,1 / 77,06 | 80 / 84 | 1913 / 1851 | +3,3% |
| 4B | 1 | 4k | 128 / 128 | 50,2 / 50,25 | 76 / 78 | 3018 / 2957 | +2,1% |
| 4B | 1 | 16k | 512 / 512 | 50,2 / 50,25 | 88 / 90 | 3414 / 3353 | +1,8% |
| 4B | 1 | 32k | 1024 / 1024 | 50,2 / 50,25 | 104 / 106 | 3942 / 3881 | +1,6% |
| 4B | 2 | 16k | 512 / 512 | 100,5 / 100,50 | 88 / 90 | 3465 / 3403 | +1,8% |
| 4B | 4 | 4k | 128 / 128 | 201,0 / 201,00 | 76 / 78 | 3169 / 3107 | +2,0% |
| 4B | 4 | 16k | 512 / 512 | 201,0 / 201,00 | 88 / 90 | 3565 / 3503 | +1,8% |
| 4B | 4 | 32k | 1024 / 1024 | 201,0 / 201,00 | 104 / 106 | 4093 / 3919 | +4,4% (kart doldu) |

- **Özyineli durum ve KV tahmini doğru:** KV ölçülen tüm yapılandırmalarda (np 1/2/4, 4k/16k/32k) bayt bayt aynı; durum (RS) ±%0,2 içinde (slot başına 19,27 / 19,27 / 50,25 MiB; `-np` ile doğrusal). Birleşik havuz (`--kv-unified`) ile ayrı havuz aynı KV ve RS'yi veriyor (np 4, 16k); tek fark compute tamponu (0.8B'de 4 slotta birleşik +12 MiB).
- **Toplam VRAM tahmini %1,6-9 yüksek (güvenli yönde):** sabit ~150 MiB model motoru payı küçük modellerde oransal olarak daha fazla. Compute tamponu en çok 16 MiB eksik (0.8B, 4 slot); tahmini etkilemeyecek kadar küçük.
- **Bağlam kontrol noktası** (ana bellekte): bir kontrol noktası tam bir slotun özyineli durumu kadar (günlük: 2B 19,266 MiB, 4B 50,251 MiB; her istemde ~2 tane, `--ctx-checkpoints 4` ile slot başına en fazla 4). Tahmin (`checkpoint_bytes` = durum x slot x 4) bununla aynı. Dashboard'daki "her ek istek ~96 MB" 2B için 19,27 (durum) + 4 x 19,27 (kontrol noktaları) = 96,3 MiB ile örtüşüyor.

### Eşzamanlı istekler (toplam t/sn)

Komut `-ngl 99 -fa on -c 16384 -np N [--kv-unified]`. Her istek `/completion`, 492 token istem, `n_predict 256`, `ignore_eos`, `cache_prompt false`; ilk istek ısınma olarak ayrı gönderilmiş. "Toplam" = üretilen token / duvar saati (istem işleme dahil). Kazanç = aynı modelin `-np 1`, tek istek değerine oranı.

| Model | -np | Havuz | Akış | Toplam t/sn | İstek başına t/sn | Kazanç |
|---|---:|---|---:|---:|---:|---:|
| 0.8B | 1 | ayrı | 1 / 4 (sıraya girdi) | 156,3 / 149,1 | 166,2 / 160,0 | 1,00× / 0,95× |
| | 2 | birleşik | 2 | 239,0 | 134,8 | 1,53× |
| | 4 | birleşik | 2 / 4 | 232,0 / 256,8 | 127,0 / 74,3 | 1,48× / **1,64×** |
| | 4 | ayrı | 4 | 273,1 | 76,9 | 1,75× |
| 2B | 1 | ayrı | 1 / 4 (sıraya girdi) | 95,8 / 94,2 | 101,2 / 100,0 | 1,00× / 0,98× |
| | 2 | birleşik | 2 | 155,9 | 85,1 | 1,63× |
| | 4 | birleşik | 2 / 4 | 152,8 / 198,2 | 83,9 / 56,8 | 1,60× / **2,07×** |
| | 4 | ayrı | 4 | 206,2 | 57,8 | 2,15× |
| 4B | 1 | ayrı | 1 / 4 (sıraya girdi) | 46,7 / 46,2 | 49,7 / 49,7 | 1,00× / 0,99× |
| | 2 | birleşik | 2 | 75,8 | 43,1 | 1,62× |
| | 4 | birleşik | 2 / 4 | 78,1 / 100,3 | 43,1 / 29,4 | 1,67× / **2,15×** |
| | 4 | ayrı | 4 | 103,3 | 29,7 | 2,21× |

- Kazanç Qwen2.5-3B'deki ("Eşzamanlı istekler, CUDA": 2 slotta 1,73×, 4 slotta 2,3×) ile aynı mertebede: 2 slotta 1,5-1,6×, 4 slotta 1,6-2,2×. Model küçüldükçe kazanç azalıyor (0.8B'de istek başı çekirdek başlatma yükü baskın).
- Özyineli durum ve KV bu kartta hep VRAM'e sığdı (4B np 4 16k: 3503 MiB), bu yüzden slot sayısı taşma kaynaklı yavaşlama yaratmadı. 4B np 4 32k kartı doldurdu (3919 MiB).
- Birleşik havuz ayrı havuza göre %3-6 yavaş (toplam KV aynı). Harness `-np N > 1` için birleşik havuzu seçmeye devam ediyor: istekler `-c / N` ile sınırlanmıyor.

### Sığmayan yapılandırmalar

| Yapılandırma | Ölçülen | Not |
|---|---|---|
| 4B Q8_0, `-c 4096`, `-ngl 99` | `CUDA0 model buffer` 4264 MiB (kartın 4096 MiB'ından büyük); nvidia-smi 3901 MiB'ta takılı; **11,2 t/sn** (Q4_K_M: 46,7) | **Sığmadı:** WDDM fazlalığı paylaşılan belleğe yazıyor, çökmeden çalışıyor ama 4 kat yavaş. Tahmin: model 4275 (ölçülen 4264, +%0,2), toplam 4679 MiB. 16k'da da nvidia-smi 3885 MiB. |
| 4B Q8_0, `-c 4096`, `-ngl 24` | nvidia-smi 3549 MiB, **15,0 t/sn** | Kısmi yük hızı %34 artırıyor, hâlâ Q4_K_M'nin üçte biri. |
| 2B Q4_K_M, `-c 262144`, f16 KV | KV 3072 MiB + model 1211 + compute 308; nvidia-smi 3899 MiB; 43,4 t/sn (tek istek, toplam) | Sığmadı (spill). Dashboard "Bellek Yetmeyebilir: tahmini 5,08 GB gerekiyor, ayrılmış VRAM 3,87 GB" ve "Yine de Başlat" soruyor. Tampon toplamı (4,5 GiB) + ~150 MiB ≈ 5,0 GB: tahminle uyumlu. |
| 2B Q4_K_M, `-c 262144`, f16, `-ngl 16` (Dashboard'un önerisi) | nvidia-smi 3901 MiB; KV'nin 1024 MiB'ı CPU'da, **compute tamponu 1052 MiB'a şişti**; 44,7 t/sn | Öneri kartı yine dolduruyor (kısmi yük tek başına bu bağlamda sığdırmıyor). |
| Qwen3.6-35B-A3B (MoE) kısmi offload | **Denenemedi** | En küçük uygun dosya `UD-Q4_K_M` 22,13 GB (20,6 GiB) > makinenin 15,8 GB RAM'i; ölçüm sırasında boş RAM 0,7-1,1 GB (başka süreçler). 32 MiB'lık başlıktan tahmin: 40 katman (10 dikkat + 30 özyineli), np 1 durum 62,8 MiB, np 4 251 MiB, KV 16k 320 MiB (np 1) / 571 MiB (np 4 birleşik). Ölçülmedi. |

### Bulunan hata: uzun bağlamda 256 genişlikli başlarla nicemli KV'nin dönüşüm tamponu

Ölçüm tahminin tek gerçek sapmasını gösterdi. Qwen3.5 (başlık genişliği 256) ile `q8_0`/`q4_0` KV'de flash attention çekirdeği, nicemli K ve V'yi katman katman f16'ya çeviriyor (CUDA'nın vektör çekirdeği 128'e kadar). Compute tamponu bir katmanın K + V'si (f16) kadar büyüyor:

| Model, KV | Bağlam | Compute tamponu (CUDA0 MiB) | Eski tahmin |
|---|---:|---:|---:|
| 2B f16 | 65536 | 116 | 112 |
| 2B q8_0 | 40960 / 49152 / 57344 / 65536 | 152 / 176 / 200 / 224 | 92 / 100 / 108 / 112 |
| 2B q8_0 | 131072 / 262144 | 416 / 800 | 176 / 304 |
| 2B q4_0 | 262144 | 800 | 304 |
| 4B q4_0 | 65536 / 131072 | 380 / 700 | 140 / 204 |
| Qwen2.5-3B (başlık 128) q8_0 / q4_0 | 65536 | 148 / 148 (f16: 145) | 150 |

- Sonuç: "Otomatik" KV, 262144 bağlamda 2B için q8_0 seçip "VRAM'e sığıyor, 3,19 GB" diyordu; gerçekte nvidia-smi 3757 MiB (3,67 GiB, %15 eksik). Kart sınırındaki yapılandırmalar yanlış "sığıyor" etiketi alabilirdi. Başlık genişliği 128 olan modellerde sapma yok.
- **Düzeltme** (`kv_policy::cuda_convert_buffer_bytes`): anahtar başı genişliği 128'den büyük modellerde eşit nicemli çift için de bir katmanın nicemli K/V'sinin f16 karşılığı (`n_ctx x (K + V) x 2` bayt) tahmine ekleniyor (flash attention kapalıysa ya da CUDA değilse yok). Ölçülenden ~16-20 MiB fazla, güvenli yönde. Düzeltmeden sonra uygulamada 262144 / Otomatik: KV `q4_0` seçiliyor (KV 3091 yerine 883 MiB), tahmin 2,94 GB, nvidia-smi 2989 MiB (%0,7); 65536 / Otomatik: f16, 2,16 GB tahmin / 2209 MiB ölçülen.
- Gemma gibi 256 başlı başka aileler aynı kuralı alıyor ama ölçülmedi; ROCm/Vulkan'da tahmin değişmedi (yalnız CUDA'da ölçüldü).
- Ana bellekte sabitlenmiş `CUDA_Host compute buffer` (bağlam kadar KiB: 262144'te 272 MiB) tahmine girmiyor; `HOST_OVERHEAD` içinde kalıyor.

### Dashboard (CDP, gizli pencere, ayrık veri dizini)

Qwen3.5-2B Q4_K_M, Otomatik ayarlar (uygulamadan "Sunucuyu Başlat"):

| Ayar | Dashboard | Gerçek (log / nvidia-smi) |
|---|---|---|
| Varsayılan: bağlam 4096, `-np` Otomatik | **1 slot** ("her ek istek ~96 MB; tek kullanıcı için 1 yeterli"), GPU katmanı 25/25, KV f16, "VRAM'e sığıyor, 1,4 GB" | `n_slots = 1`, 25/25, nvidia-smi 1429 MiB (1,40 GiB) |
| 16k, `-np 4` | Slot 4, birleşik KV açık, "VRAM'e sığıyor, 1,6 GB" | `n_slots = 4, kv_unified = true`, 1643 MiB |
| 32k, `-np 4` | "1,81 GB" | 1851 MiB (1,81 GiB) |
| 64k, Otomatik | f16, "2,16 GB" | 2209 MiB (2,16 GiB) |
| 262144, Otomatik (düzeltmeden önce) | KV q8_0, "3,19 GB" sığıyor | 3757 MiB: sığmıyor (%15 eksik) |
| 262144, Otomatik (düzeltmeden sonra) | KV q4_0, "2,94 GB", log "KV 3091 yerine 883 MiB" | 2989 MiB |
| 262144, KV Tam (f16) | "Bellek Yetmeyebilir: tahmini 5,08 GB, ayrılmış VRAM 3,87 GB", önerilen -ngl 16 | tampon toplamı ~5,0 GB; nvidia-smi 3899 MiB'ta takılı (spill) |

- Önerilen `-np`/`-ngl`/KV ölçümle tutarlı. Kalan not: bağlam 262144'te Etkin değerlerde "Belleğe sığmadı" etiketi çıkıyor (llama-server `failed to fit params to free device memory` satırı: kendi 1 GiB payı yüzünden); yapılandırma gerçekte çalışıyor (2989 MiB). Etiket llama-server'ın görüşünü aktarıyor, Harness tahminiyle çelişmiyor.

### Sınırlar

- Tek makine (RTX 3050 Ti Laptop 4 GB, WDDM); Qwen3.5-0.8B/2B/4B, tek build (b11456). Qwen3.6-35B-A3B ölçülemedi (RAM).
- nvidia-smi WDDM'de ~3,9 GB'ta doyuyor; taşan yapılandırmalar yalnız yavaşlama olarak görünür.
- Eşzamanlı ölçüm kısa istemle (492 token); uzun istemlerde bağlam kontrol noktaları (slot başına en fazla 4) ana belleği büyütür (bkz. "Gerçek RAM büyümesi"). 4 slot x ~7,5k token'lık istem 16k havuza sığmadığı için kontrol noktası denemesinde 4 slotlu koşu yanıtsız kaldı (token bütçeli kabul gateway'de).

- **Taze cihaz raporu maliyeti (CUDA):** `make_room` artık her bellek kararında `llama-server --list-devices` çalıştırıyor; bu kartta ~1,0 sn (ilk çağrı 2,7 sn). Birleşik kodla JIT yükleri: 0,5B 4,3 sn (önceki koşular 4,9-7,0), 1,5B 4,2 sn, 3B 6,3 sn (yalnız yük) / 3,1 sn (0,5B yüklüyken), Llama Q6_K 9,2 sn; koşular arası değişkenlik ek ~1 sn'yi aşıyor, ayrı bir artış ölçülemedi. 16 adımlık gateway testi birleşik kodla 16/16 geçti.

## Kutudan çıkış ayarları ve paralel istekler

Makine: RX 7900 GRE 16 GB (Vulkan, b11552), masaüstü (dwm vb.) boştayken ~4,2 GiB tutuyor. Kutudan çıkış = ayarsız tanım, gateway üzerinden (`bench/compare`, ayrık veri dizini, derleme). Önceki bir karşılaştırma ölçümündeki kurulu sürümle (tam yük düzeltmesinden önce: 4B 29/37 katman, 14B 33/49 + taşma) ve bu düzeltmeyi içeren ara bir derlemeyle karşılaştırıldı.

| | Referans (kurulu, düzeltme öncesi) | Ara derleme | Güncel |
|---|---|---|---|
| 4B çözme, kutudan çıkış | 47,9 t/sn (29/37 katman, 32k) | 145 t/sn (37/37, 32k) | 139-145 t/sn (37/37, 32k) |
| 14B çözme, kutudan çıkış | 14,2 t/sn, katmanlar 33/49 + paylaşılan bellek yavaşlaması | 46,0 t/sn, 49/49, 32k q8_0/q4_0, **1,1 GiB paylaşılan belleğe taştı** | 48,7 t/sn, 49/49, 16k q8_0/q4_0, taşma yok (61 MiB) |
| 14B araç testi (38 vaka), kutudan çıkış | 5/38 | 5/38 | 35-36/38 |
| 4 eşzamanlı akış, toplam t/sn, kutudan çıkış | 53 (1 slot) | 139 (1 slot) | **385** (4 slot, birleşik havuz) |
| 4 slot açık (`equal4`) toplam t/sn | 170 | - | 393 |

* **Flash attention:** `-fa` verilmediğinde llama.cpp `auto` kullanır ve Vulkan RDNA3'te açar: Qwen3-4B 32k f16, `-fa auto` 150,6 t/sn, `-fa on` 148,1, `-fa off` **123,4** (ve 2,1 GiB paylaşılan belleğe taşıyor). Yani "FA kapalı" yalnız etkin ayar ekranındaki `flash_attn: false` alanıdır; varsayılan `auto` doğru. Katman sayısının düşük olduğu eski ölçüm (29/37) `full_offload_context` ile giderildi.
* **Taşma eşiği (AMD/Windows):** Qwen2.5-Coder-14B Q4_K_M, `-ngl 99`, `-fa on`; sürecin ayrılmış belleği / kartın toplam kullanımı / paylaşılan belleğe taşan: 8k f16 9,8 GiB / 14,1 GiB / 49 MiB; 16k q8_0 9,9 GiB / 14,2 GiB / 57 MiB; 24k q8_0+q4_0 9,3 GiB / 13,6 GiB / **923 MiB**; 32k q8_0 9,5 GiB / 13,9 GiB / **2113 MiB**. Vulkan'ın ve DXGI'nin bildirdiği bütçe (15557 MiB) bunu göstermiyor; sürücü, bütün süreçlerin toplamı kartın ~2,1 GiB altını geçince taşıyor. Tahmin doğru (9956 MiB tahmin / 9986 ölçülen). Bu yüzden Windows + ayrık AMD kartta otomatik pay `max(1 GiB, %8)` yerine en az 2200 MiB (`vram::AMD_WINDOWS_DRIVER_RESERVE`). NVIDIA'da 3801/3962 MiB taşmadan ölçüldüğü için pay değişmedi.
* **Birleşik havuz + ana bellek istem önbelleği takılması (llama.cpp b11552):** 4 slot `--kv-unified` ve `--cache-ram > 0` (varsayılan 8192, Harness'ın 1024'ü) iken her paralel istek dalgasının ilk jetonu ~3,7 sn gecikiyor (`launch_slot_` öncesi bekleme); ilk dalga hızlı. 4 akış x 256 jeton, Qwen3-4B: `-np 4` (birleşik değil) 395 t/sn; `-np 4 --kv-unified` 172; `--kv-unified --cache-ram 0` 400; yalnız `-c 32768` (llama-server varsayılanı, birleşik) 172; ek olarak `--cache-ram 0` 400; `-np 2 --kv-unified` 136-174 (TTFT 2-6 sn). Bu, karşılaştırmadaki "Harness 4 slot 170, llama-server 398" farkının sebebidir: llama-server satırı `-np 4`'ü birleşiksiz çalıştırmıştı. Çözüm: birleşik havuzda birden çok slot varsa ve kullanıcı seçmediyse `--cache-ram 0` (`slots::runs_unified_pool`, `process.rs`). Slot başına önceki istem yine slotta kalır; kaybedilen yalnız slotlar arası ana bellek önbelleğidir.
