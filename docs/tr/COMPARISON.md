# Harness Studio, LM Studio, Ollama ve llama.cpp karşılaştırması

[English](../en/COMPARISON.md)

2026-10-11'de tek bir Windows makinesinde, her araç için aynı GGUF dosyalarıyla ölçüldü. Tam tablolar ve ölçüm betiği
[benchmarks/2026-10-11/summary.md](../benchmarks/2026-10-11/summary.md) ve
[`bench/compare/`](../../bench/compare/) içindedir. Betik her sayıyı başka bir makinede yeniden üretir. Özellik özellik matris (hangi araçta ne var)
[FEATURES.md](FEATURES.md) içindedir; bu ölçümler oradaki §0'da da yer alır.

## Kısa özet

- **Ayarsız hız:** tek istekli üretimde LM Studio ile başa baş, Ollama ve düz llama.cpp'nin önünde. Aynı anda dört istek geldiğinde **LM Studio'dan
  2,2 kat, Ollama'dan 2,7 kat** hızlı.
- **Araç çağırma:** 14B kodlayıcı modelin metin biçimindeki araç çağrılarını gerçek çağrıya çeviren tek araç. Harness 36/38 aldı;
  diğerleri 4-5/38.
- **Uzun istemler ve ajanlar:** kutudan çıkan bağlam 20 bin token'lık istemi ve 10,8 bin token'lık ajan konuşmasını karşılıyor. LM Studio (8k) ve
  Ollama (4k) kullanıcı sınırı elle yükseltene kadar bağlam hatası döndürüyor.
- **Bellek güvenliği:** Harness modeli paylaşımlı sistem belleğinin dışında tutan ayarlar seçer. Sığmayan istekleri yüklemek ve yavaş çalışmak yerine
  reddeder ve nedenini açıklar. Bunun bedeli: 14B model 65k bağlamda, kısa istemli testte rakiplerin paylaşımlı belleğe taşan yüklemelerinden daha
  yavaş üretir.
- **Tek uç noktadan model değiştirme:** 6,5 sn (Ollama 5,6 sn). LM Studio varsayılan JIT ayarıyla sessizce yanlış modelle yanıt verdi.
  llama.cpp yeniden başlatma gerektirir.

Rakiplerin önde olduğu yerler aşağıda aynı ağırlıkla listelenmiştir.

## Test düzeni

| | |
|---|---|
| Makine | Windows 11 Pro, AMD Ryzen 7 9700X, 32 GB RAM, AMD Radeon RX 7900 GRE 16 GB (Vulkan) |
| Harness Studio | 0.1.0, release derlemesi, yönetilen llama.cpp b11556 (Vulkan), `harness serve --gateway` |
| LM Studio | lms CLI 69d945a, motor llama.cpp-win-x86_64-vulkan-avx2 2.51.0 |
| Ollama | 0.35.1, ayrı ve temiz bir `ollama serve` örneği |
| llama.cpp | b11026 (winget) ve b11552, `llama-server`, Vulkan |
| Modeller | Qwen3-4B-Instruct-2507 Q4_K_M (2,5 GB), Qwen2.5-Coder-14B-Instruct Q4_K_M (9,0 GB): her araç için aynı dosyalar |

**Çalıştırma koşulları**

- Araçlar tek tek çalıştırıldı. Her yüklemeden önce diğer tüm arka uçlar durduruldu ve ayrılmış VRAM'in başlangıç değerine dönmesi beklendi.
- `temperature 0` ve `seed 42` ile akışlı OpenAI `chat/completions` istekleri. Her isteğin benzersiz bir nonce'u vardı; böylece istem önbellekleri
  soğuk istekleri olduğundan iyi göstermedi.
- Her değer, atılan bir ısınma turundan sonra 5 üretim, 3 istem işleme ve 3 eşzamanlılık turunun ortancasıdır.

**Profiller**

- **kutudan çıktığı gibi:** her araç modeli kendi varsayılanlarıyla yükler.
- **eşit:** her araca aynı açık ayarlar verilir: 32k bağlam, tam GPU'ya aktarım, flash attention, f16 KV, 1 slot.

Tam yöntem: [summary.md](../benchmarks/2026-10-11/summary.md#methodology).

## Sonuçlar

tok/sn için yüksek, saniye ve paylaşımlı bellek artışı için düşük değer iyidir. "Bağlam hatası", HTTP 400 "exceeds the available
context" demektir. Harness 14B kutudan çıktığı gibi: 49/49 katman, 16k bağlam, q8_0 KV, 2 slot. Harness 4B: 37/37 katman, 32k bağlam, f16 KV, 4 slot.

| Test | Harness Studio | LM Studio | Ollama | llama.cpp |
|---|---|---|---|---|
| 4B üretim, kutudan çıktığı gibi (tok/sn) | **140,9** | **142,0** | 128,9 | 114,5 |
| 4B istem işleme, 4k token (tok/sn) | 2460 | 2308 | **2997** | 1381 |
| 14B üretim, kutudan çıktığı gibi (tok/sn) | **48,6** | 42,0 (tekrar ölçüm 50,7) | **49,0** | 40,9 |
| 14B istem işleme, 4k token (tok/sn) | 737 | 442 (tekrar ölçüm 822) | **1109** | 402 |
| 4B üretim, eşit ayarlar (tok/sn) | 144,3 | 143,3 | 128,9 | **146,9** |
| 14B, eşit ayarlar (~14,6 GB gerekir, 12,1 GB boş) | açıklamayla reddedildi | yüklendi, paylaşımlı belleğe taştı | yüklendi, taştı | yüklendi, taştı |
| 20k token'lık istem, kutudan çıktığı gibi | **doğru** | bağlam hatası | bağlam hatası | **doğru** |
| Araç çağırma, 4B (38 durum) | **38/38** | **38/38** | **38/38** | **38/38** |
| Araç çağırma, 14B kodlayıcı (38 durum) | **36/38** | 5/38 | 4/38 | 5/38 |
| Ajan testi, önbellekli 2./3. tur TTFT (sn) | **0,45 / 0,25** | bağlam hatası | bağlam hatası | 0,95 / 0,30 |
| Ajan testi, soğuk ilk tur (sn) | **6,6** | bağlam hatası | bağlam hatası | 27-30 |
| 4 eşzamanlı istek, kutudan çıktığı gibi (toplam tok/sn) | **344** | 157 | 126 | 152 |
| 4 eşzamanlı istek, her birinde 4 slot (toplam tok/sn) | **399** | 156 | 266 | **398** |
| 65k bağlamda 14B, üretim (tok/sn) | 15,5 | **46,0** | 43,8 | 14,6 |
| 65k bağlamda 14B, paylaşımlı bellek artışı (MiB) | **98** | 12376 | 10326 | 5208 |
| Tek uç noktadan 4B → 14B geçişi, ilk token süresi (sn) | 6,5 | yanlış model yanıt verdi | **5,6** | desteklenmiyor (yeniden başlatma 22,9) |

<details>
<summary>Önce ve sonra: ölçüm Harness Studio'yu da düzeltti</summary>

Günün ilk çalıştırması Harness'ın önceki derlemesiyle yapıldı. Dört zayıf varsayılanı ortaya çıkardı:

- AMD/Windows sürücü payı fazla temkinliydi (4B 29/37 katmanda, 14B 33/49 katmanda).
- Gateway yüklemeleri tek slot kullanıyordu.
- Paylaşımlı KV havuzları `--cache-ram 0` olmadan çalışıyordu.
- Metin araç çağrısını kurtarma isteğe bağlıydı.

Dördü de yayından önce düzeltildi ve Harness satırları release derlemesinde yeniden ölçüldü. Bir LM Studio kontrol turu dışında rakip
satırları yeniden çalıştırılmadı.

| Test | Önceki derleme | Release derlemesi |
|---|---|---|
| 4B üretim (tok/sn) | 47,9 | 140,9 |
| 14B üretim (tok/sn) | 14,2 | 48,6 |
| 14B araç çağırma | 5/38 | 36/38 |
| 4 eşzamanlı, kutudan çıktığı gibi (tok/sn) | 53 | 344 |
| 4 eşzamanlı, 4 slot (tok/sn) | 170 | 399 |
| Model geçişi TTFT (sn) | 9,1 | 6,5 |

</details>

## Sayılar ne anlama geliyor

- **Varsayılanlar tepe hızdan daha önemlidir.** Eşit ayarlarda dört motor da aşağı yukarı aynı hızda üretir, çünkü llama.cpp çekirdeğini paylaşırlar.
  Fark, her aracın kullanıcı için neyi seçtiğinden gelir:
  - bağlam boyutu
  - katman sayısı
  - KV önbellek türü
  - slot sayısı
  - metin biçimindeki araç çağrısının kurtarılıp kurtarılmadığı
  - sığmayan bir modelin yine de yüklenip yüklenmediği
- **Ayarsız çalışan bağlam.** Harness bağlamı modelden (en çok 32k) ve boş VRAM'den boyutlandırır. LM Studio (8k) ve Ollama (4k), sınır
  yükseltilene kadar 20 bin token'lık istemde ve ajan testinde başarısız oldu. Düz llama.cpp kısmen paylaşımlı belleğe taşan 85k bağlam seçti; soğuk
  ajan turunun 27-30 sn sürmesinin nedeni budur.
- **İstemcilerin kullanabildiği araç çağrıları.** Qwen2.5-Coder araç çağrılarını `<tools>{json}</tools>` metni olarak yazar. Bunu varsayılan olarak OpenAI
  `tool_calls`'a çeviren tek araç Harness'tır. Claude Code, OpenCode, Zed veya VS Code gibi istemciler böylece gerçek çağrı alır.
- **Eşzamanlılık.** Gateway yüklemeleri gerçek isteklere göre boyutlandırılmış dört slot açar ve paylaşımlı havuzlardaki llama.cpp istem önbelleği
  takılmasından kaçınır. Kutudan çıktığı gibi dört paralel istek, hiçbir bayrak ayarlanmadan LM Studio'dan 2,2 kat, Ollama'dan 2,7 kat hızlı çalışır.

## Rakiplerin önde olduğu yerler

- **İstem işleme:** bu testte 4k token'lık istemleri en hızlı Ollama işledi: 4B'de 2997'ye karşı 2460 tok/sn, 14B'de 1109'a karşı 737.
- **Çok küçük kartta büyük bağlam:** 65k'da LM Studio ve Ollama, 10-12 GB'ı paylaşımlı belleğe taşıyarak kısa istemleri Harness'tan yaklaşık 3 kat
  hızlı üretti. Bu çalıştırma, taşmanın uzun istemleri ne kadar yavaşlattığını ölçmedi. Harness modeli paylaşımlı bellekten uzak tuttu ve katmanları
  CPU'ya alarak üretim hızından vazgeçti.
- **Sığmayan açık ayarlar:** Harness bunları yavaş yüklemek yerine reddeder (HTTP 503; gereken ve boş bellekle birlikte). "Yine de yükle"yi
  tercih edenlerin ayarları düşürmesi gerekir.
- **Olgunluk ve erişim:**
  - Harness Studio ilk 0.1.0 sürümüdür; tek bir bakımcısı var.
  - LM Studio ve Ollama'nın geniş kullanıcı kitleleri, model kataloğları, macOS/MLX desteği ve SDK'ları var.
  - LM Studio'nun 30'dan fazla dilde arayüzü var. Ollama'nın bulut modelleri ve `ollama launch`'ı var.
  - llama.cpp en geniş donanım arka uçları kümesine sahip; router kipi artık ayrı bir gateway gerektiren birkaç özelliği karşılıyor.
- **Harness'ta bugün eksik olanlar:**
  - Resmi TypeScript ve Python SDK'ları (planlı)
  - Karar modeli uç noktaları (`/v1/decisions`, `/v1/systemone`) ve başka bir Harness'a uzaktan erişim (Harness Link) (ikisi de planlı)
  - MLX motoru ve gerçek makinelerde doğrulanmış bir macOS derlemesi
  - Türkçe ve İngilizce dışındaki arayüz dilleri
  - Ollama `push` (bilerek: Harness'ın model kayıt defteri yok)

## NVIDIA CUDA, 4 GB dizüstü kartı (2026-10-15)

NVIDIA RTX 3050 Ti Laptop (4 GB, CUDA 12), Intel Core i7-12700H ve 16 GB RAM üzerinde ikinci bir çalıştırma. Harness Studio'yu (başsız,
main 623e87c), yönettiği `llama-server` yapısının (b11574, CUDA) aynı dosyalarla doğrudan başlatılmasıyla karşılaştırır. Bu yüzden her
fark motor farkı değil, ürün tercihidir. Ollama ve LM Studio bu çalıştırmada yoktu. 14B model 4 GB'a sığmadığı için ikinci model
Qwen3-0.6B Q8_0'dır. Tam tablolar: [benchmarks/2026-10-15-nvidia/summary.md](../benchmarks/2026-10-15-nvidia/summary.md).

| Test | Harness Studio | llama.cpp (aynı yapı, doğrudan) |
|---|---|---|
| 4B çözme, kutudan çıktığı gibi (tok/sn; llama-bench ile kart tavanı 56,7–60,8) | **55,3** | 29,9 |
| 4B istem işleme, 4k token (tok/sn) | **1963** | 1573 |
| 0.6B çözme, kutudan çıktığı gibi (tok/sn) | 157,7 | **187,7** |
| 0.6B istem işleme, 4k token (tok/sn) | 3967 | **9168** |
| 4B, eşit ayarlar (32k f16 KV ~7,2 GB ister, 4,1 GB boş) | açıklamayla reddedildi | yüklendi, taştı: 42,5 tok/sn, istem işleme 341 tok/sn, soğuk ajan turu 53 sn |
| 0.6B, eşit ayarlar (çözme / istem işleme tok/sn) | **186,6 / 8971** (VRAM'de kalmak için bağlam 16k'ya indirildi) | 155,9 / 1443 (32k, taştı) |
| Araç çağırma, 4B (38 durum) | **38/38** | **38/38** |
| Araç çağırma, 0.6B, kutudan çıktığı gibi / eşit | **36/38** / 32/38 | 34/38 / **35/38** |
| Ajan testi, kutudan çıktığı gibi: soğuk / 2. tur / 3. tur TTFT (sn) | **7,0 / 0,36 / 0,13** | bağlam hatası (4k bağlam) |
| 4 eşzamanlı istek, 4B kutudan çıktığı gibi (toplam tok/sn) | 56,4 (1 yuva) | **75,3** (4 yuva, 4k bağlam) |
| 0.6B 65k bağlamda, çözme (tok/sn) | **123,5** | 59,3 |
| Tek uç noktadan 4B → 0.6B geçişi (sn) | **2,0** | desteklenmiyor (yeniden başlatma 7,6) |

- **Küçük kartta kutudan çıktığı gibi** Harness 4B modelin tamamını GPU'ya koyar ve ayar yapmadan kartın tavanına ulaşır. Varsayılanlarıyla
  düz `llama-server` bu hızın yaklaşık yarısında çözer.
- **Düz llama.cpp'nin önde olduğu yerler:**
  - **Kutudan çıktığı gibi küçük modeller:** 0.6B'de Harness'ın varsayılanları (32k bağlam, q8_0 KV, 4 yuva) `llama-server`'ın
    varsayılanlarından %16 daha yavaş çözer ve istemleri yarısından daha düşük hızla işler.
  - **4B'de kutudan çıktığı gibi eşzamanlılık:** Harness 4 GB kartta 12k bağlamlı tek yuva açar, bu yüzden dört paralel istek sıraya
    girer (ortanca TTFT 6,9 sn). `llama-server` 1k'lık dört yuva açar ve dördüne birden yanıt verir; bedeli 4k toplam bağlamdır.
- **0.6B'de araç çağırma** çalıştırmalar ve profiller arasında birkaç durum oynar; durum başına tek çalıştırma bu farkları gürültüden
  ayıramaz.

## Bu karşılaştırmanın sınırları

- Ana çalıştırma tek makine ve tek GPU üreticisi kullandı: Windows'ta Vulkan'lı AMD. Yukarıdaki NVIDIA/CUDA bölümü LM Studio ve Ollama olmadan yapılan daha küçük bir çalıştırmadır. Linux ve macOS ölçülmedi.
- Rakip satırları ilk çalıştırmadan gelir. Tekrarlanan LM Studio kontrol ölçümü 4B'de ilk çalıştırmayla uyuştu (142,7'ye karşı 142,0
  tok/sn), ama 14B'de yaklaşık %20 daha hızlıydı (50,7'ye karşı 42,0). 14B'de çalıştırmalar arası değişkenlik yaklaşık %20'dir; daha küçük 14B farklarını gürültü sayın.
- Her biri tek bir aileden iki model. Diğer mimariler, görü modelleri ve MoE modelleri denenmedi.
- Araç paketi 38 durumdan oluşur ve her durum bir kez çalışır. Biçim kurtarma etkisini net gösterir ama akıl yürütme kalitesini notlandırmaz.
- Ölçüm betiğini Harness'ın yazarı yazdı. Başkalarının yeniden çalıştırabilmesi için [`bench/compare/`](../../bench/compare/) içinde açıktır.
