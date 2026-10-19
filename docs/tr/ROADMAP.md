# Yol Haritası

[English](../en/ROADMAP.md)

Harness Studio, yerel metin, görsel, konuşma ve araç iş yüklerini tek yerden yönetir. Öncelik yerel kullanımdadır; ek motorlar ve bulut bağlantıları isteğe bağlıdır. Bu yol haritası yön gösterir; öncelikler geri bildirimle değişebilir. Önerilerinizi [Discussions](https://github.com/struklabs/harness-studio/discussions) veya [issue](https://github.com/struklabs/harness-studio/issues) üzerinden paylaşın. Ayrıntılar için [değişiklik günlüğüne](../CHANGELOG.md) bakın.

## 0.1.2 ile Gelenler

- [x] **Playground:** araç sonuçları, tablolar, grafikler ve basit arayüz blokları için üretken arayüz.
- [x] **Model Keşfi:** Donanımıma Uygun sıralaması ve sığma etiketleri; Hugging Face ile aynı sıralama.
- [x] **Günlük Denetimler:** Basit ve Gelişmiş modlar, iyileştirilmiş sayfa düzenleri, açık kalan bölüm menüsü, araçlar için Tüm İnternet izni ve Otomatik Araç Oluşturucu'da Günlüğü Temizle.
- [x] **Güvenlik:** arşiv açılırken sembolik bağlantılar ve hedef klasör dışındaki yollar reddediliyor, ayrıcalıklı izin bitleri kaldırılıyor.

## 0.1.1 ile Gelenler

- [x] **Motorlar:** transcribe.cpp, llama.cpp Qwen3-TTS, Apple Silicon'da mflux ile görseller, CSM dahil genişleyen MLX konuşma desteği.
- [x] **Model Kapsamı:** daha fazla ONNX görü, NLP ve ses görevi; görsel düzenleme, video üretme ve konuşmacı ayrımıyla ses işleme.
- [x] **Bağlantılar:** Harness Link ile uzak Harness modellerini Playground'da ve yerel ağ geçidinde kullanma.
- [x] **Masaüstü Deneyimi:** erişilebilirlik ve yüksek arayüz ölçeği düzeltmeleri, ilk çalıştırma iyileştirmeleri, daha hızlı uzun sohbetler ve model kütüphaneleri.
- [x] **Platformlar ve Kararlılık:** macOS güncellemeleri, Linux AppImage desteği; bellek baskısı, indirmeler ve model başlatma düzeltmeleri.
- [x] **SDK'lar:** TypeScript SDK npm'de yayımlandı; TypeScript ve Python istemcileri CI'da Control API'ye karşı denetleniyor.
- [x] **Linux Güncellemeleri:** AppImage ve `.deb` paketleme; AppImage uygulama içi güncelleme ve `.deb` elle güncelleme yönlendirmesi ([PR #101](https://github.com/struklabs/harness-studio/pull/101), [PR #106](https://github.com/struklabs/harness-studio/pull/106), [değişiklik günlüğü](../CHANGELOG.md)). Daha geniş uçtan uca doğrulama açık kalır.

## Mevcut Altyapı

- [x] **Arayüzsüz Sunucu:** `harness serve`, Control API ve Gateway mevcut ([kaynak](../../crates/harness-cli/src/serve.rs)); tarayıcı yönetimi ve tam sunucu işletimi planlanıyor.

## Yayın Sonrası Adımlar

- [ ] Linux'u daha fazla AMD/NVIDIA makinede ve dağıtımda doğrulamak; önizlemeyi kaldırmadan ilk çalıştırma, erişilebilirlik ve benchmark kontrollerini tamamlamak.
- [ ] AppImage güncellemelerini ve `.deb` elle güncelleme akışını uçtan uca doğrulamak; paketleme ve güncelleyici entegrasyonu teslim edildi.
- [ ] Linux NVIDIA doğrulamasından sonra CUDA vLLM eklemek.
- [ ] Python SDK'yı Trusted Publishing ile PyPI'da yayımlamak.
- [ ] Web sitesi ve imzalama hizmeti hazır olduğunda Windows kurulum paketini imzalamak.
- [ ] Upstream llama.cpp desteklediğinde LFM2.5-Audio ses çıkışını etkinleştirmek.

## 0.2

Uçtan uca akışları sırayla sunmak: her dalga kurulum, kullanım, izin ve kurtarma adımlarını kapsar. Her üçüncü dalga (W3, W6, …) sunulan akışları iyileştirir. Modül altyapısı, Daha Sonra özelliklerini 0.2'ye taşımaz.

### W1 — Modül Keşfi ve Kurulumu

- [ ] Temel uygulamayı hafif tutmak; işlevsel alan olan Module, paylaşılan Component, Skill, MCP sunucusu, Connector, Tool ve Client Integration türlerini ayırmak. İsteğe bağlı Training, Agents, Automations, Paperclip ve REA alanları için anlaşılır gezinme planlamak.
- [ ] Kullanıcının modül işlevlerini, bağımlılıklarını, izinlerini ve disk tahminini incelemesini, kurulumu onaylamasını ve aynı menüden çalışma alanını açmasını sağlamak. İlk açılış, modül kurulumu ve tekrar kullanılabilir iş hazırlığını ayırmak.
- [ ] Uyumluluk denetimiyle sürümlü manifest, API/IPC sınırları ve yalıtılmış arka uç süreçleri kullanmak. Modül arayüzünü uygulamayla dağıtmak; indirilebilir ekran veya dış JavaScript arayüzü olmadan yalnız arka uçları/bağımlılıkları indirmek.
- [ ] Mevcut `struklabs/harness-registry` deposunu yalnız resmî StrukLabs Skill, MCP sunucusu ve Tool içerikleri için ilerletmek. Mevcut içeriği doğrulamak; web sitesi veya otomatik yayını beklemeden sürümlü indeks ve bakımcı PR'larıyla arama, sıralama, filtre, açıklama, sürüm bilgisi ve CI kontrolü sunmak.

### W2 — Ajan Bağlama ve Görev Tamamlama

- [ ] OpenClaw veya Hermes Agent'ı isteğe bağlı kurmak ya da mevcut kurulumlara bağlanmak; dış arayüzlerini koruyarak upstream sürümleri, kurulum, başlatma/güncelleme, eksik bağımlılıklar ve ilk çalıştırma testlerini yönetmek.
- [ ] Yerel veya Harness Link modellerini OpenClaw, Hermes Agent ve mevcut Paperclip örneğine sağlamak; görev/donanıma uygun model önerip uç nokta/anahtarları güvenle yapılandırmak. Paperclip yaşam döngüsü yönetimini Daha Sonra kapsamında tutmak.
- [ ] Onay, denetim kaydı ve durdurma denetimleriyle ajan görev/oturumlarını, model bağlantılarını, durumu, performansı, hataları ve kapsamlı araç izinlerini yönetmek; varsayılan olarak sınırsız yetki vermemek.
- [ ] Mevcut Otomatik Araç Oluşturucu'nun oluşturma/test/kaydetme akışını ve izin gösterimini iyileştirmek; kullanıcı araçlarıyla kurulan Tool'ları ayırmak, Agent Skills/`SKILL.md` desteği sunmak, dış kodlama ajanlarının akışlarını yinelemek yerine Harness araçlarını paylaşmak.

### W3 — İyileştirme Dalgası: Güvenilir Kurulum ve Ajan Kullanımı

- [ ] Paylaşılan bileşen sahipliği, disk kullanımı, uyumluluk denetimi, geri alma ve uygulanabilir çevrimdışı kurulum/kaldırmayla modül açma/kapatma, güncelleme, onarma ve kaldırmayı tamamlamak. Kullanımdaki bileşenlerin kaldırılmasını engellemek.
- [ ] İş, sohbet, model ve fiziksel dosya silmeyi ayırmak; modül ve uygulama kaldırılırken veri kümelerini, modelleri, projeleri ve değerli çıktıları korumak, silme tercihlerini açıkça sunmak.
- [ ] Mevcut Güvenlik Modeli'ni MCP, ajan motoru, eğitim arka ucu ve Tool güven seviyeleriyle genişletmek; dosya/klasör, ağ, kabuk, servis, hesap ve dışarı veri aktarımı izinlerini verme/geri alma anında açıklamak. Mümkün olduğunda işletim sistemi yalıtımı kullanıp Windows/macOS/Linux sınırlarını belgelemek.
- [ ] Durum, bağımlılık, ilerleme, hata ve kurtarmayı tutarlı kılmak; disk alanı ve süreç temizliğiyle modüllerin uygulamayı kilitlemesini önlemek. Entegrasyon öncesinde üçüncü taraf lisans, yeniden dağıtım, ticari kullanım, gömme, barındırma ve atıf koşullarını incelemek.

### W4 — Model Seçimi ve İlgili Araç Kullanımı

- [ ] Seçilen modelin sohbet, görü, görsel üretimi, STT, TTS, video, embedding, yeniden sıralama ve araç çağırma yeteneklerini göstermek; deneysel veya uyumsuz model/motor birleşimlerini açıkça etiketlemek.
- [ ] Her şemayı her isteğe eklemek yerine gerektiğinde keşif ve şema yüklemeyle ilgili Tool/MCP yeteneklerine yönlendirmek; yönlendirme hatalarını ve izin ihtiyaçlarını göstermek.
- [ ] Keşfet'te Skill, MCP sunucusu, Connector, Tool ve uygulamayla gelen Module türlerini ayrı göstermek; modül/arka uç kurulum metadatasını resmî Skill/MCP/Tool Registry'sinden ayrı tutmak.
- [ ] İlk model kurulumu, kaynak seçimi, model değiştirme ve çalıştırma konumunu anlaşılır kılmak; akış boyunca model, sağlayıcı, ajan, iş akışı ve cihazı ayırmak.

### W5 — Mevcut Harness Sunucusunu Yönetme

- [ ] Tarayıcı yönetimini teslim edilmiş `harness serve`, Control API, Gateway, Harness Link ve yetkilendirme yetenekleri üzerine kurmak; genişletmeden önce mevcut sınırları incelemek.
- [ ] Uzak yönetim için tarayıcı oturumları, HTTPS, kullanıcılar ve kapsamlı token'ları; anlaşılır bağlantı onayı, iptal ve hatalarla sunmak.
- [ ] Linux hizmet paketleri ve Docker imajlarıyla sunucu güncelleme, günlük, yedekleme/geri yükleme ve hizmet yaşam döngüsü akışlarını eklemek.
- [ ] Control API ve TypeScript/Python SDK kapsamını incelemek; paket deposundaki yayın durumunu doğrulayıp kurulum rehberinde yayımlanmış paketlerle yalnız kaynak kodu bulunan istemcileri ayırmak.

### W6 — İyileştirme Dalgası: Uçtan Uca Yayına Hazırlık

- [ ] Desteklenen platformlarda keşfet → kur → model bağla → ajan/araç çalıştır → hatadan kurtar → kaldır akışlarını; izin iptali ve kullanıcı dosyalarının korunmasıyla doğrulamak.
- [ ] Eşzamanlı modül/sunucu kullanımında arayüz tepkisini, disk kullanımını ve süreç/bellek temizliğini ölçmek; kapsamı genişletmeden gerilemeleri düzeltmek.
- [ ] Gerektiğinde araç yönlendirmeyi, yetenek etiketlerini, güvenli uç nokta/anahtar yönetimini ve yerel/bağlı model uyumluluğunu doğrulamak.
- [ ] İlk model, ilk ajan ve uzak sunucu senaryolarıyla ilk kullanım ve tekrar kullanılabilir hazırlığı iyileştirmek; teslim edilen sınırları, bağımlılık sürümlerini ve kurtarma adımlarını iki dilde belgelemek.

## Daha Sonra

### Eğitim

- [ ] Mevcut tek arka uç ve küçük, tekrarlanabilir eğitim PoC'siyle başlamak. Unsloth, LLaMA-Factory, MLX-LM ve Axolotl'ı gerçek model/GPU/işletim sistemi desteğiyle karşılaştırmak; ilk yöntemi seçmeden LoRA/QLoRA, SFT, DPO, çok modlu eğitim, tam ince ayar ve ön eğitimi değerlendirmek.
- [ ] Hedef/model/veri kümesi/cihaz seçimi, taslak/geri dönme/devam, veri temizleme/biçim/bölme/lisans kontrolü, tahmin, deneme, kuyruk, ölçüm, değerlendirme ve checkpoint içeren tekrar kullanılabilir Eğitim İşleri sunmak; duraklatma/devamı arka uç destekliyorsa açmak. Mimari, ağırlık biçimi, sürücü, hesaplama yeteneği, VRAM/RAM, CPU, disk, kütüphane sürümleri ve ölçülen performansa göre uyumlu yöntem önermek; desteklenmeyen birleşimleri açıklamak. Yeni arka uçlar geldikçe elle değiştirme eklemek.
- [ ] LoRA dışa aktarma, birleştirme, dönüştürme, nicemleme ve Kütüphane'ye aktarmayı yönetmek; Harness Cloud GPU zorunluluğu olmadan kullanıcının bağlı GPU sunucusunda çalıştırıp masaüstünden/mobilden izlemek.

### Ajanlar ve Orkestrasyon

- [ ] Orkestrasyonu test edilmiş model/bağlam/araç çağırma gereksinimleri, gerekçeli model/sağlayıcı değişimi, BYOK, eşzamanlılık, çoklu cihaz/model, yeniden deneme, zaman aşımı, maliyet/token, ara çıktı, onay ve kurtarmayla genişletmek. DeepSeek Harness yaklaşımlarını ve Core/modül/adapter sahipliğini karşılaştırmak.
- [ ] Paperclip kurulumunu veya mevcut örneğini ekip, rol, ajan, görev, çalıştırma, bütçe, model protokolü, sağlık ve Projeler bağlantıları için desteklenen API'lerle entegre etmek; upstream akışlarını koruyup yalnız yararlı yerlerde Harness ekranları eklemek.

### Otomasyonlar

- [ ] Kullanıcının kendi n8n örneğine bağlanmak; iş akışı, tetikleyici, model/sağlayıcı, çalıştırma, geçmiş, hata ve düzenleyici bağlantılarını göstermek. Gateway modelleri ve kapsamlı dosya/araç erişimi sunmak; otomasyon motorunu gömmemek, dağıtmamak, kurmamak veya işletmemek.
- [ ] Playground'un izinli n8n API/MCP üzerinden onay ve sonuç incelemesiyle iş akışı taslağı hazırlamasını, oluşturmasını, düzenlemesini ve tetiklemesini sağlamak; gerçek API yeteneklerini ve ilgili entegrasyon/ticari koşulları doğrulamak.

### REA

- [ ] Desteklenen EXE/DLL, Electron/JavaScript, .NET, APK, web ve ikili hedeflerde REA'nın mevcut CLI/MCP motorlarını kullanan isteğe bağlı Playground analiz alanı eklemek; decompiler veya tam IDE geliştirmeden hedef, analiz türü, model ve motor seçmek.
- [ ] Node.js ve kurulu motorları algılamak; yalnız gereken bileşenleri kurmak, Ghidra/Hopper/IDA gereksinimleri ve ücretli lisanslarını açıklamak; yetkili, gizliliği korunan analiz için inceleme, çalıştırma, debugger, ADB, ağ ve cihaz izinlerini ayırmak.
- [ ] Desteklendiği ölçüde ilerleme, kanıt, geçmiş, bulgu, sözde kod, sembol, çağrı grafiği, karşılaştırma ve rapor göstermek; tutarlı Analiz/Kodlama gezinmesiyle resmî REA MCP/Skill içeriklerini Harness modellerini kullanan OpenCode, Codex ve diğer istemcilere açmak.

### Kodlama

- [ ] Yerel/uzak modellerle kod üretimi, dosya düzenleme, araç, test, hata düzeltme ve yeniden düzenleme için proje kapsamlı alanı değerlendirmek; istemci entegrasyonlarını koruyup tam IDE yapmadan OpenCode, Codex, Claude Code, Continue, Cursor ve Zed yaklaşımlarından yararlanmak.
- [ ] Çalışma alanı izinlerini, değişen dosyaları, komutları, görev ilerlemesini ve geri alınabilir yamaları/checkpoint'leri göstermek; mobil görev izleme ve odaklı değişiklik incelemesi sunmak.

### Bağlam ve RAG

- [ ] Tercihen en çok 200–300 MB boyutunda güvenilir küçük embedding bileşeniyle belge, proje, ajan görevi, araç çıktısı ve geçmiş kararlardan bağlam getirmek; model değişimi, indeks uyumluluğu, güncelleme, taşınabilirlik ve çok dilli kaliteyi yönetmek.
- [ ] Metin, ses/video dökümü, zaman damgası, görsel, OCR/görü, metadata ve kaynakları gerektiğinde ayrı çıkarım/erişim aşamalarıyla ilişkilendirmek; GPU'yu sürekli meşgul etmeden indeks süresi, CPU/RAM/disk, gecikme ve pil maliyetini ölçmek.

### Playground

- [ ] Sohbeti görsel/ses/video, ses dönüştürme, belge, kodlama, analiz ve ajan için işe özel Oluştur/Stüdyo alanlarıyla genişletmek; ComfyUI köprüsü eklemek, mevcut çok modlu özellikleri korumak ve Core/modül ekranlarını yinelememek. Atomic Chat gibi bütünleşik deneyimleri karşılaştırmak.
- [ ] Her görevin API, akış, dosya, çıktı ve işlem kaynağı ihtiyacını karşılamak; uzun video işlerinde ilerleme, iptal, çıktı biçimi ve dosya önizlemesi sunmak.

### Projeler ve Çıktılar

- [ ] Sohbet, dosya, araç, ajan, eğitim işi, çıktı ve izinleri hafif Proje/Çalışma Alanlarında toplamak; ağır veritabanı veya zorunlu bulut olmadan kaynak, metadata, izin ve yaşam döngüsüyle çıktıları modüller arasında paylaşmak.
- [ ] Ajan Görevleri ve Analiz Oturumlarında taslak, ileri/geri adım ve sonradan devam sunmak. Canlı RAM taşımayı zorunlu tutmadan geçmiş, görev durumu, dosya referansı ve checkpoint ile cihazlar arasında işe devam etmek; model/sağlayıcı uyumluluk sınırlarını açıklamak.

### StrukLabs Hesabı ve Web

- [ ] Profil, cihaz, senkronizasyon ve uzak hizmetler için StrukLabs ürünleri arasında ortak kimlik tasarlamak; OIDC/SSO/OAuth ve yönetilen/sunucusuz kimlik seçeneklerini güvenlik, bakım ve maliyetle karşılaştırmak.
- [ ] Web sitesinde gerektiğinde ürün, hesap, yayın, belgeleme, abonelik ve bulut hizmeti yönetimi sunmak; resmî Registry'yi deposundan yönetmek, topluluk gönderimi, moderasyon veya Registry yönetim paneli eklememek.

### Mobil

- [ ] Gezinme, onay, akış, dosya görüntüleme ve iş yönetimiyle mobile özel tam istemci geliştirmek; Svelte/Capacitor, Kotlin/Swift ve alternatifleri kod paylaşımı, yerel API, arka plan, bildirim, mağaza politikaları ve bakımla karşılaştırmak.
- [ ] Telefondan ev/sunucu modellerini veya bilgisayarsız BYOK/özel uç noktaları kullanmak; kapsamlı rollerle model, modül, ajan, eğitim/REA durumu, ayar ve performansı yönetmek. Sağlayıcı ile GPU cihazı bağlantılarını, çalıştırma konumunu ve ajanın model/cihaz/yeteneklerini açıkça göstermek.

### Uzak Erişim ve Cihazlar

- [ ] Mevcut bağlantıları güvenli eşleştirme, cihaz doğrulama, kapsamlı token, onay, denetim, iptal ve kayıp telefon kurtarmayla genişletmek; telefondan sohbeti ve uzaktaki OpenCode'un seçilen Gateway'e erişimini sağlamak.
- [ ] WebRTC, QUIC veya dışa açılan WebSocket ile doğrudan LAN, internet P2P ve relay yedeğini NAT/güvenlik duvarı, mobil güvenilirlik, bant genişliği ve maliyetle karşılaştırmak; çıkarım/dosyaları uygulanabildiğinde seçilen cihazda tutup büyük GGUF dosyalarını mümkünse doğrudan aktarmak.
- [ ] Cihaz rolü, model, motor, izin, disk, iş yükü ve erişilebilirliği göstermek; yerel donanım yetersizse başka cihaz önermek ve yalnız role uygun bileşenleri kurmak. StrukLabs'ı zorunlu çıkarım aktarımı yerine kimlik/keşif/sinyalleme için kullanmak.

### BYOK

- [ ] OpenAI, Gemini, Anthropic, OpenRouter ve özel uç noktalar için masaüstü/sunucu/mobil sağlayıcı kurulumu, model keşfi, yetenek, kota/hata ve maliyeti ortaklaştırmak; Chat Completions, Responses, Anthropic Messages, Ollama, akış, araç ve çok modlu protokol farklarını doğrulamak.
- [ ] Bağlantı testi, hata açıklaması ve sır yönetimini ortaklaştırırken anahtarların yalnız kullanıcının seçtiği telefon veya masaüstünde kalabilmesini sağlamak.

### Performans

- [ ] Çıkarım, eğitim, ajan, görsel ve seste ölçülen yükleme, benchmark ve araç çağırma güvenilirliğiyle görev/donanım önerilerini geliştirmek; donanım uyumu, görev uyumu, kalite ve hızı ayrı değerlendirmek.
- [ ] Bellek tahmini, öncelik, kuyruk, GPU rezervasyonu, iptal ve güvenli yedek yolla yarışan eğitim/görsel/çıkarım işlerini yönetmek; gerilemeleri izlemek.
- [ ] NVIDIA benchmark'ından sonra faster-whisper'ı, SEA-G2P uygulanabilir olduğunda VieNeu'yu değerlendirmek; OpenVINO ve MLC'nin yararlı kapsam sağlayıp sağlamadığına karar vermek.

### Güvenlik

- [ ] Yoğun GPU/CPU kullanımı, mikrofon ve kamera için iş yükü izinlerini genişletmek; zararlı paket, prompt injection, bağımlılık güncellemesi, tedarik zinciri, kimlik bilgisi sızıntısı ve geri alınabilir ajan değişikliklerini değerlendirmek.
- [ ] Açık varsayılanlar ve saklama süresiyle anonim, izinli çökme/ortam/günlük raporları sunmak; sohbet, model ve dosya içeriğini telemetri dışında tutmak.

### Açık Kaynak

- [ ] Gelecekteki web/hesap/mobil çalışmaları dahil tek monorepo'da kalmak; önce gerçek modül sınırları ve hedefli derleme/test sağlamak. Daha sonra sürümlü sözleşmeler ve uyumlu arka uç güncellemeleriyle çoklu arka uç/bileşen ve modülün sunduğu entegrasyonları desteklemek.
- [ ] MIT temel uygulama, SDK dağıtımı, üçüncü taraf katkısı ve ücretli hizmet sınırlarını açıklamak; çatallar yerine sürdürülebilir upstream katkıları/adapter'ları tercih edip güncelleme/destek sorumluluğunu belgelemek. Registry'yi paket imzalama veya topluluk yayını olmadan yalnız resmî içerikle tutmak.
- [ ] README, yayın, lisans, platform uyumu, amaç, popülerlik ve bakım metadatasıyla ayrı GitHub arama/URL keşfi eklemek; resmî API sınırı, OAuth, indeks/önbellek ve maliyeti değerlendirmek. Keşfi, kurulabilirlikten ve resmî kataloğa dahil olmaktan ayırmak.
- [ ] Keşfedilen `SKILL.md` referans/script, güvenlik, sürüm sabitleme ve kurulum kapsamını doğrulamak; resmî MCP Registry metadata/protokol denetimini değerlendirip yerel stdio ile uzak HTTP farkını açıklamak. Eksik Skill/MCP/Tool/Connector taslağından önce resmî paket önermek; OAuth, onay, gizlilik ve iptalle Gmail, Excel, Drive ve takvim Connector'leri eklemek.

### CI ve SDK

- [ ] Değişen Rust crate, Svelte modülü, Registry manifesti ve SDK için hedefli CI; kritik sınırlarda entegrasyon testi, takvimli/yayın aşamalarında tam matris kullanmak. Modül ve üçüncü taraf geliştiriciler için sürümlü API/SDK tanımlamak.
- [ ] Harness geliştirme, araç/Skill, ajan görevi, küçük ince ayar, Registry kurulumu ve mobil yönetimde kendi ürününü kullanmayı tekrarlanabilir QA'ya dönüştürmek; küçük güvenilir örnekleri CI'a bağlamak.

### Ticari

- [ ] İsteğe bağlı ücretli çevrimiçi, depolama/senkronizasyon, relay ve ekip hizmetlerini değerlendirmek; kimlik/metadata/Registry/sinyalleme bütçesini aktarım/depolama/bildirimden ayırıp zorunlu merkezî çıkarım veya büyük dosya aktarımından kaçınmak.
- [ ] Kendi barındırılan ve yönetilen web altyapısını güvenilirlik, bakım, açık işletim maliyeti ve sağlayıcıya kilitlenmeden taşınabilirlik açısından karşılaştırmak.

## Planlanmayanlar

- Ollama kayıt deposundan indirme (`hf.co/...` başvuruları desteklenir).
- Temel uygulama için Python zorunluluğu veya yerel motorların model kapsamını yineleyen Python motorları; ek motorlar isteğe bağlı kalır.
