# Changelog

All notable changes to Harness Studio are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

Release notes with system requirements and known limitations (English and Turkish): [0.1.2 English](en/RELEASE_NOTES_0.1.2.md) / [Türkçe](tr/RELEASE_NOTES_0.1.2.md).

## [Unreleased]

- Modules: a new Modules screen; speech recognition is an optional module you can install, enable, disable, update, repair, roll back and remove, with disk usage shown and your data kept on removal. (#307, #312)
- Modüller: yeni Modüller ekranı; konuşma tanıma kurulup açılıp kapatılabilen, güncellenebilen, onarılabilen, geri alınabilen ve kaldırılabilen isteğe bağlı bir modül; disk kullanımı gösterilir, kaldırmada verileriniz korunur. (#307, #312)
- Official Harness Registry: browse and install official Tools, MCP servers and Skills. (#308, #310)
- Resmi Harness Registry: resmi Araçları, MCP sunucularını ve Skill'leri görüntüleyip kurabilirsiniz. (#308, #310)
- Agents: connect Hermes Agent or an existing Paperclip instance to Harness models, run tasks, and review task history, per-run permissions and the audit log. (#309, #311)
- Ajanlar: Hermes Agent'ı veya mevcut bir Paperclip kurulumunu Harness modellerine bağlayın, görev çalıştırın; görev geçmişini, çalıştırma bazlı izinleri ve denetim kaydını inceleyin. (#309, #311)
- Agent Skills (SKILL.md) support; Tool Builder fixes and clearer permissions. (#310)
- Agent Skills (SKILL.md) desteği; Tool Builder düzeltmeleri ve daha net izinler. (#310)
- Clearer trust levels and permission summaries; MCP and agent processes are contained and cleaned up. (#313)
- Daha net güven seviyeleri ve izin özetleri; MCP ve ajan süreçleri sınırlanır ve temizlenir. (#313)
- Model capability badges, including Audio Input; tools load on demand, cutting tool prompt tokens by about half. (#315, #322)
- Model yetenek rozetleri (Ses Girdisi dahil); araçlar gerektiğinde yükleniyor, araç istemi token'ları yaklaşık yarıya iniyor. (#315, #322)
- Discover has categories for Skills, MCP, Connectors, Tools and Modules; model setup and where models run are clearer. (#316)
- Keşfet, Skill, MCP, Connector, Araç ve Modüller için kategoriler sunuyor; model kurulumu ve modellerin çalıştığı yer daha net. (#316)
- Manage a headless Harness server from your browser with a secure admin login and optional HTTPS. (#317)
- Ekransız Harness sunucusunu güvenli yönetici girişi ve isteğe bağlı HTTPS ile tarayıcıdan yönetin. (#317)
- Fixed on-demand tool routing dropping a tool forced through tool_choice; the tool routing warning is shorter, hides credentials and offers copyable details. (#321, #322)
- İsteğe bağlı araç yönlendirmesinin tool_choice ile zorlanan aracı düşürmesi düzeltildi; araç yönlendirme uyarısı kısaldı, kimlik bilgilerini gizler ve kopyalanabilir ayrıntı sunar. (#321, #322)

## [0.1.2] - 2026-10-18

- The section menu stays open after you choose an item; close it with Collapse.
- Bölüm menüsü bir öğe seçtikten sonra açık kalıyor; Daralt ile kapatılıyor.
- Automatic Tool Builder: the activity log can be cleared with Clear Log.
- Otomatik Araç Oluşturucu: işlem günlüğü Günlüğü Temizle ile temizlenebiliyor.
- Tools can be given Entire Internet access; the allowed-domains list is now an optional restriction. Local and private network addresses stay blocked.
- Araçlara Tüm İnternet erişimi verilebiliyor; izin verilen alan adları artık isteğe bağlı bir sınırlama. Yerel ve özel ağ adresleri engelli kalıyor.
- Model Hub sorting matches Hugging Face; the sort menu shows its full label when there is room.
- Model Hub sıralaması Hugging Face ile aynı; sıralama menüsü yer olduğunda tam etiketini gösteriyor.
- Model Hub: Best for My Hardware ranks models by how well they fit this machine and shows a fit badge.
- Model Hub: Donanımıma Uygun, modelleri bu bilgisayara ne kadar sığdıklarına göre sıralıyor ve sığma etiketi gösteriyor.
- Security: archive extraction rejects symlinks and paths outside the target folder, and drops privileged permission bits.
- Güvenlik: arşiv açma, kısayol dosyalarını ve hedef klasör dışındaki yolları reddediyor, ayrıcalıklı izin bitlerini kaldırıyor.
- Playground renders tool results, tables, charts and simple UI blocks as rich components.
- Playground araç sonuçlarını, tabloları, grafikleri ve basit arayüz bloklarını zengin bileşenlerle gösteriyor.
- Polished Dashboard, Servers, Model Hub and My Library layouts.
- Dashboard, Sunucular, Model Hub ve Kütüphanem düzenleri iyileştirildi.
- Polished Settings, Connections, Create, Performance and Analytics layouts.
- Ayarlar, Bağlantılar, Oluştur, Performans ve Analitik düzenleri iyileştirildi.
- Simple mode shows only the essentials with plain labels; Advanced keeps every option, better grouped.
- Basit mod yalnızca temel işleri sade etiketlerle gösteriyor; Gelişmiş mod tüm seçenekleri daha düzenli gruplarla sunuyor.

## [0.1.1] - 2026-10-16

- Long chats open instantly: recent messages show first and older ones load in the background without moving the view.
- Uzun sohbetler anında açılıyor: son mesajlar önce görünüyor, eskiler görünümü kaydırmadan arka planda yükleniyor.
- Scrollable code blocks are announced as "Code Block" and show a full focus ring.
- Kaydırılabilir kod blokları "Kod Bloğu" olarak duyuruluyor ve tam odak halkası gösteriyor.
- MLX text-to-speech: a long text is no longer silently cut at two minutes of audio; it is spoken in full within the time limit, and when the limit stops it the answer says so (x-harness-truncated: time).
- MLX metinden sese: uzun metin artık iki dakikalık seste sessizce kesilmiyor; süre sınırı içinde tamamı konuşuluyor, sınır durdurursa yanıt bunu bildiriyor (x-harness-truncated: time).
- MLX text-to-speech reads up to about ten minutes of text (the time limit is 600 s, kept under the gateway's timeout), pauses briefly between the parts of a long text, and the Speech panel says when the audio was cut (at the time limit, or because the model ran to its length limit).
- MLX metinden sese yaklaşık on dakikalık metni okuyabiliyor (süre sınırı 600 sn, ağ geçidi zaman aşımının altında tutuluyor), uzun metnin parçaları arasında kısa bir duraklama yapıyor ve Konuşma paneli sesin kesildiğini bildiriyor (süre sınırında ya da model uzunluk sınırına ulaştığında).
- Library: MLX OuteTTS folders whose model card is named in lower case are listed on macOS, and Spark-TTS folders show "spark" as their architecture instead of "qwen2".
- Kütüphane: model kartı küçük harfle adlandırılmış MLX OuteTTS klasörleri macOS'ta listeleniyor; Spark-TTS klasörlerinin mimarisi "qwen2" yerine "spark" görünüyor.
- A second Harness instance with its own data folder no longer rewrites client configs (OpenCode, editors, MCP clients) that another instance set up; fewer leftover backup files.
- Kendi veri klasörüyle çalışan ikinci bir Harness örneği, başka örneğin kurduğu istemci yapılandırmalarını (OpenCode, editörler, MCP istemcileri) artık değiştirmiyor; daha az yedek dosyası birikiyor.
- Speech panel: a new request clears the previous audio and its "cut" notice; MLX text-to-speech keeps a space between a CJK sentence and a following Latin one.
- Konuşma paneli: yeni istek önceki sesi ve "kesildi" uyarısını temizliyor; MLX metinden sese, CJK cümlesiyle ardından gelen Latin cümle arasında boşluk bırakıyor.
- MLX text to speech: CSM (mlx-community/csm-1b-fp16) speaks on Apple Silicon with its two voices; its Mimi codec, text tokenizer and voice prompts download with it from pinned, checksum-verified commits (all MLX codecs are now pinned). CSM output is not watermarked. MLX voice models keep their weights in memory, so requests on small Macs no longer slow down unpredictably, and the Speech panel shows the speed slider only for models that support it.
- MLX metinden sese: CSM (mlx-community/csm-1b-fp16) Apple Silicon'da iki sesiyle konuşuyor; Mimi codec'i, metin tokenizer'ı ve ses örnekleri modelle birlikte, sabitlenmiş ve sağlama toplamı denetlenen commit'lerden iniyor (bütün MLX codec'leri artık sabitlenmiş). CSM çıktısı filigranlı değil. MLX ses modelleri ağırlıklarını bellekte tutuyor, küçük Mac'lerde istekler artık öngörülemez biçimde yavaşlamıyor; Konuşma paneli hız kaydırıcısını yalnız hızı destekleyen modellerde gösteriyor.
- Transcription with llama.cpp models that hear (LFM2.5-Audio, Qwen3-ASR, Voxtral...) on `/v1/audio/transcriptions` and in Transcribe; LFM2.5-Audio shows as partially supported in the Hub.
- Duyan llama.cpp modelleriyle (LFM2.5-Audio, Qwen3-ASR, Voxtral...) `/v1/audio/transcriptions` ve Yazıya Çevir'de yazıya çevirme; Hub'da LFM2.5-Audio kısmi destekli görünüyor.
- First-run wizard on macOS: Apple Silicon memory is shown as unified memory instead of VRAM; the models folder and engine paths are shown in full; the storage and engine texts no longer say every model is GGUF.
- macOS'ta ilk çalıştırma sihirbazı: Apple Silicon belleği VRAM yerine birleşik bellek olarak gösteriliyor; model klasörü ve motor yolları tam görünüyor; depolama ve motor metinleri artık her modeli GGUF saymıyor.
- Harness Link: add a remote Harness (address + inference token) under Connections and use its models in the Playground and through your local gateway; tokens stay in the system keychain.
- Harness Link: Bağlantılar'dan uzak bir Harness (adres + inference token) ekleyip modellerini Playground'da ve yerel gateway üzerinden kullanın; token'lar sistem anahtar zincirinde kalır.
- Hub: speech recognition GGUF repos with a GGUF projector (Audar-ASR and Qwen3-ASR quantizations) show as runnable on llama.cpp instead of unsupported.
- Hub: GGUF projektörlü konuşma tanıma GGUF depoları (Audar-ASR ve Qwen3-ASR nicemlemeleri) desteklenmiyor görünmek yerine llama.cpp ile çalışır görünüyor.
- Hub: CosyVoice3, VoxCPM2 and other versioned speech repos say they need their own package instead of "no GGUF file".
- Hub: CosyVoice3, VoxCPM2 ve sürümlü adlı diğer konuşma depoları "GGUF dosyası yok" yerine kendi paketini gerektirdiğini söylüyor.
- Hugging Face ONNX path: timm classifiers, BiRefNet and CLIPSeg segmentation, RT-DETR, legacy BERT `vocab.txt` repos, generic-head NER and sentence embeddings (`/v1/hf` feature extraction) now run; question-answering spans on RoBERTa-style tokenizers are no longer clipped; a GPU warm-up with an error or non-finite output falls back to CPU.
- Hugging Face ONNX yolu: timm sınıflandırıcıları, BiRefNet ve CLIPSeg segmentasyonu, RT-DETR, eski BERT `vocab.txt` depoları, genel başlıklı NER ve cümle gömmeleri (`/v1/hf` özellik çıkarımı) artık çalışıyor; RoBERTa tarzı belirteçleyicilerde soru-cevap aralıkları artık kırpılmıyor; hata ya da sonlu olmayan çıktı veren GPU ısınması CPU'ya düşüyor.
- Accessibility on macOS: VoiceOver can use the ⌘K command palette after the first-run wizard, no longer reaches the app behind the open wizard, and announces the window as "Harness Studio".
- macOS'ta erişilebilirlik: VoiceOver ilk çalıştırma sihirbazından sonra ⌘K komut paletini kullanabiliyor, açık sihirbazın arkasındaki uygulamaya artık ulaşmıyor ve pencereyi "Harness Studio" olarak okuyor.
- My Models lists models from your Harness Link connections in a read-only Remote group with Open in Playground; the docs describe Harness Link setup.
- Kütüphanem, Harness Link bağlantılarınızdaki modelleri salt okunur bir Uzak grubunda Playground'da Aç ile listeliyor; belgeler Harness Link kurulumunu anlatıyor.
- Text to speech on Apple Silicon: Qwen3-TTS models (Base, CustomVoice, VoiceDesign) download their speech codec and speak, and Chatterbox Turbo downloads its default voice; the 8-bit CSM build that cannot run is no longer offered.
- Apple Silicon'da metinden konuşma: Qwen3-TTS modelleri (Base, CustomVoice, VoiceDesign) konuşma codec'lerini indirip konuşuyor, Chatterbox Turbo varsayılan sesini indiriyor; çalışmayan 8-bit CSM sürümü artık sunulmuyor.
- Audio processing and diarization: Model Hub and `/v1/audio/process` now run RE-USE (speech restoration), HTDemucs and HTDemucs 6-stem (stem separation), and Nemotron 3 Diarization and Sortformer (speaker turns); built-in diarization finds the right number of speakers more often and gives every transcript segment one speaker; `harness pull` no longer rejects repositories with a `LICENSE.html`; a GPU out-of-memory is reported as such after idle audio models are unloaded and the request retried.
- Ses işleme ve konuşmacı ayırma: Model Hub ve `/v1/audio/process` artık RE-USE (konuşma onarımı), HTDemucs ve HTDemucs 6-stem (kanal ayırma) ile Nemotron 3 Diarization ve Sortformer (konuşmacı turları) modellerini çalıştırıyor; yerleşik konuşmacı ayırma konuşmacı sayısını daha sık doğru buluyor ve her döküm bölümüne tek konuşmacı veriyor; `harness pull` `LICENSE.html` içeren depoları artık reddetmiyor; GPU belleği yetmediğinde boştaki ses modelleri kapatılıp istek yeniden deneniyor ve hata açıkça bildiriliyor.
- Speech recognition on Apple Silicon: mlx-community's older MLX Whisper conversions (large-v3, turbo, medium, small, base, their 4/8-bit builds) are offered and transcribe; their processor is downloaded with them.
- Apple Silicon'da konuşma tanıma: mlx-community'nin eski MLX Whisper dönüşümleri (large-v3, turbo, medium, small, base ve 4/8-bit sürümleri) artık sunuluyor ve yazıya döküyor; işlemcileri modelle birlikte iniyor.
- Vocal separation on Apple Silicon: mlx-community's Mel-Band-RoFormer models split audio into vocals and instrumental through `/v1/audio/process` and Playground, next to audio.cpp's models.
- Apple Silicon'da vokal ayırma: mlx-community'nin Mel-Band-RoFormer modelleri, sesi `/v1/audio/process` ve Playground üzerinden audio.cpp modellerinin yanında vokal ve enstrümantal olarak ayırıyor.
- The TypeScript SDK `@struklabs/harness-studio-sdk` 0.1.0 is published on npm with provenance; publishing uses npm Trusted Publishing (OIDC, no token).
- TypeScript SDK `@struklabs/harness-studio-sdk` 0.1.0 npm'de provenance ile yayımlandı; yayımlama npm Trusted Publishing (OIDC, token yok) ile yapılıyor.
- CI checks that the SDK OpenAPI snapshot is current and tests the TypeScript and Python clients against a running server.
- CI, SDK OpenAPI anlık görüntüsünün güncel olduğunu denetler ve TypeScript ile Python istemcilerini çalışan bir sunucuya karşı test eder.
- The TypeScript SDK's npm name is `@struklabs/harness-studio-sdk`; a manually triggered workflow (workflow_dispatch, from main) publishes the SDKs after the snapshot check and tests: npm with provenance, PyPI only through trusted publishing once it is set up.
- TypeScript SDK'nın npm adı artık `@struklabs/harness-studio-sdk`. Elle tetiklenen bir iş akışı (workflow_dispatch, main'den), anlık görüntü denetimi ve testlerden sonra SDK'ları yayımlar: npm'e provenance ile, PyPI'a ise trusted publishing kurulduktan sonra yalnız onunla.
- The Python SDK requires Python 3.11 or later (the generated client uses `typing.Self`).
- Python SDK artık Python 3.11 veya üstünü gerektirir (üretilen istemci `typing.Self` kullanıyor).
- Windows: launch at login now reads the startup approval from its status byte, so an entry disabled in Task Manager is no longer shown as on.
- Windows: oturum açılışında başlatma, başlangıç onayını durum baytından okuyor; Görev Yöneticisi'nde kapatılan kayıt artık açık görünmüyor.
- Playground > Speech: a disabled Speak button now says why (engine missing, gateway off, no voice model, no text), with the engine install right next to it.
- Playground > Konuşma: devre dışı Seslendir düğmesi artık nedenini yanında söylüyor (motor eksik, gateway kapalı, ses modeli yok, metin yok); motor kurulumu da hemen yanında.
- Model Hub: the whisper.cpp version of a CTranslate2 Whisper model shows its file name in Simple mode. A downloaded variant offers Update when an update exists and Download Again in Advanced mode, instead of a plain Download.
- Model Hub: CTranslate2 Whisper modelinin whisper.cpp sürümü Basit modda dosya adıyla görünüyor. İndirilmiş varyant düz İndir yerine güncelleme varsa Güncelle, Gelişmiş modda Yeniden İndir sunuyor.
- Dashboard: memory meters show used and total with the same precision ("2.0 / 15.9 GB").
- Pano: bellek göstergeleri kullanılan ve toplam değeri aynı hassasiyetle gösteriyor ("2,0 / 15,9 GB").
- Model Hub: after an engine install, wherever it was started, the "needs the engine" banners and the cards' Required chips disappear at once.
- Model Hub: motor nereden kurulursa kurulsun, "motor gerekli" uyarıları ve kartlardaki Gerekli etiketleri hemen kayboluyor.
- Setup wizard: on the goals step, Install Selected is the primary button and Skip is secondary; a model being downloaded stays checked, and two goal titles use Title Case.
- Kurulum sihirbazı: amaç adımında birincil düğme Seçilenleri Kur, Atla ikincil; indirilen model işaretli kalıyor ve iki amaç başlığı büyük harfle yazılıyor.
- High contrast themes: unavailable models, rows and servers keep full text contrast (AAA) and are marked with a dashed outline instead of being dimmed.
- Yüksek kontrast temaları: kullanılamayan model, satır ve sunucular soluklaştırılmıyor; metin tam kontrastta (AAA) kalıyor ve durum kesik çizgili çerçeveyle gösteriliyor.
- Toasts no longer cover the Playground message box and its Send button; they appear just above it.
- Bildirimler artık Playground mesaj kutusunu ve Gönder düğmesini kapatmıyor; hemen üstünde görünüyor.
- Download, install, resource and memory estimate lines show used and total sizes with the same precision.
- İndirme, kurulum, kaynak ve bellek tahmini satırları kullanılan ve toplam boyutu aynı hassasiyetle gösteriyor.
- Documentation: the user guide, README, FEATURES and ARCHITECTURE match the behaviour merged on 15.10 (Hub refusals and whisper.cpp builds, wizard buttons, start at sign-in, updater, ONNX Runtime sizes, random seeds, memory fitting).
- Dokümantasyon: kullanım kılavuzu, README, FEATURES ve ARCHITECTURE 15.10'da birleşen davranışla uyumlu (Hub ret nedenleri ve whisper.cpp sürümleri, sihirbaz düğmeleri, oturum açılınca başlatma, güncelleyici, ONNX Runtime boyutları, rastgele tohum, bellek sığdırma).
- Sliders (GPU layers, LoRA scale) have accessible names for screen readers.
- Kaydırıcılar (GPU katmanları, LoRA ölçeği) ekran okuyucular için erişilebilir ada sahip.
- Screen readers tell repeated buttons apart: Harness MCP client actions name their client, and each "Show All Settings" names its parameter group.
- Ekran okuyucular tekrar eden düğmeleri ayırt eder: Harness MCP istemci düğmeleri istemcinin adını, her "Tüm Ayarları Göster" düğmesi parametre grubunun adını taşır.
- Closing the Playground model picker with Escape returns focus to its button.
- Playground model seçicisi Escape ile kapatılınca odak düğmesine döner.
- MLX audio engine (Apple Silicon): text to speech with OuteTTS, Orpheus and Dia. Their audio codec (DAC or SNAC) downloads together with the model, and its size is shown in the download total; the engine still runs offline.
- MLX ses motoru (Apple Silicon): OuteTTS, Orpheus ve Dia ile seslendirme. Gerekli ses kodeği (DAC veya SNAC) modelle birlikte iniyor ve boyutu indirme toplamında gösteriliyor; motor yine çevrimdışı çalışıyor.
- Model Hub: searches return about a third faster; independent Hugging Face requests run in parallel and reuse one connection.
- Model Hub: aramalar yaklaşık üçte bir daha hızlı dönüyor; birbirinden bağımsız Hugging Face istekleri paralel çalışıyor ve tek bağlantıyı yeniden kullanıyor.
- MLX text to speech: sentences are no longer cut after Turkish abbreviations and ordinals ("Dr.", "vs.", "3."); lists and long texts without punctuation are split at line breaks and at a length cap, so the time limit applies to them too. A Spark-TTS folder is no longer listed as a chat model.
- MLX seslendirme: cümleler artık Türkçe kısaltma ve sıra sayılarından ("Dr.", "vs.", "3.") sonra bölünmüyor; noktalamasız listeler ve uzun metinler satır sonlarında ve bir uzunluk sınırında bölünüyor, böylece süre sınırı onlara da uygulanıyor. Spark-TTS klasörü artık sohbet modeli olarak listelenmiyor.
- Starting a server again after a stop that timed out now ends the old process first, instead of leaving it running unseen with its memory and port.
- Zaman aşımına uğrayan bir durdurmanın ardından sunucu yeniden başlatılırken eski süreç önce sonlandırılıyor; artık belleği ve portuyla görünmeden çalışmaya devam etmiyor.
- With automatic restart on, stopping a crashed server while its restart was due could start it again; the stop now wins.
- Otomatik yeniden başlatma açıkken, yeniden başlatma zamanı gelmiş çökmüş bir sunucuyu durdurmak onu yeniden başlatabiliyordu; artık durdurma geçerli oluyor.
- Automatic chat titles now follow the conversation's language, also on small models; when the model replies in another language or with a bare answer, the title comes from your first message.
- Otomatik sohbet başlıkları artık küçük modellerde de sohbetin dilinde; model başka dilde ya da yalnızca bir cevapla dönerse başlık ilk mesajınızdan oluşturuluyor.
- A server that crashes is now announced on every page, with a button to its details; failure messages name the engine that ran (mlx, mistral.rs...) instead of always "llama-server".
- Çöken sunucu artık her sayfada, ayrıntılarına giden bir düğmeyle bildiriliyor; hata metinleri her zaman "llama-server" demek yerine çalışan motorun adını (mlx, mistral.rs...) söylüyor.
- My Models stays fast with hundreds of models: only the visible cards are rendered, and clearing the search no longer freezes the page. Keyboard navigation, screen reader counts and scroll position are kept.
- Kütüphanem yüzlerce modelde de hızlı: yalnızca görünen kartlar çiziliyor, aramayı temizlemek sayfayı dondurmuyor. Klavye gezinmesi, ekran okuyucu sayıları ve kaydırma konumu korunuyor.
- After the setup wizard, keyboard focus starts at the opened page instead of nowhere.
- Kurulum sihirbazından sonra klavye odağı boşlukta değil, açılan sayfanın başında başlar.
- In narrow windows and at high display scales, page section menus collapse to a drawer so content keeps more room.
- Dar pencerelerde ve yüksek ekran ölçeğinde sayfa bölüm menüleri çekmeceye dönüşür, içeriğe daha çok yer kalır.
- The Dashboard model picker stays fast with hundreds of models and has a filter field; arrow keys, Home/End and Page Up/Down reach every model.
- Dashboard'daki model seçici yüzlerce modelde de hızlı ve bir filtre alanı var; ok tuşları, Home/End ve Page Up/Down her modele ulaşıyor.
- A server ended from outside (SIGKILL) now says so and points to low memory as the likely cause; on macOS, when the system's log confirms it ended the server for memory, it is shown as an out-of-memory failure with the recovery options and is not restarted automatically. A Python worker's MemoryError counts as out of memory.
- Dışarıdan sonlandırılan (SIGKILL) sunucu artık bunu söylüyor ve olası nedenin bellek azlığı olduğunu belirtiyor; macOS'ta sistem günlüğü sunucunun bellek yüzünden sonlandırıldığını doğrularsa bellek hatası olarak, kurtarma seçenekleriyle gösteriliyor ve otomatik yeniden başlatılmıyor. Python çalışanının MemoryError'u bellek hatası sayılıyor.
- My Models opens about four times faster with large libraries: one directory scan is shared by the whole page and the listing no longer re-reads every file.
- Kütüphanem büyük kütüphanelerde yaklaşık dört kat daha hızlı açılıyor: sayfanın tamamı tek bir dizin taramasını paylaşıyor ve liste her dosyayı yeniden okumuyor.
- Automatic chat titles: titles in Turkish chats are only replaced when they are clearly English, abbreviations such as "örn." or "Prof. Dr." no longer cut the fallback title, and a greeting before a request is skipped.
- Otomatik sohbet başlıkları: Türkçe sohbette başlık yalnızca açıkça İngilizceyse değiştiriliyor, "örn." ya da "Prof. Dr." gibi kısaltmalar yedek başlığı kesmiyor ve isteğin önündeki selamlama atlanıyor.
- `harness run` and `harness chat` work with MLX and Hugging Face models when the gateway is off (they failed with HTTP 401); the key such a server needs is read from this computer's own instance only and never stored.
- `harness run` ve `harness chat`, ağ geçidi kapalıyken de MLX ve Hugging Face modelleriyle çalışıyor (HTTP 401 ile başarısız oluyordu); bu sunucuların gerektirdiği anahtar yalnızca bu bilgisayardaki kendi örnekten okunuyor ve hiçbir yere kaydedilmiyor.
- Model picker: Enter in the filter picks the first match, ArrowUp from the first row returns to the filter, and screen readers get the full combobox pattern.
- Model seçici: filtrede Enter ilk eşleşmeyi seçiyor, ilk satırda ArrowUp filtreye dönüyor ve ekran okuyucular tam combobox kalıbını alıyor.
- macOS: when Harness Studio crashes, the processes a model server started are now ended with it, not only the server itself.
- macOS: Harness Studio çöktüğünde artık yalnız model sunucusu değil, onun başlattığı süreçler de birlikte sonlandırılıyor.
- Library scanning stays responsive during long imports and copies less data per lookup.
- Kütüphane taraması uzun içe aktarmalar sırasında da yanıt veriyor ve her aramada daha az veri kopyalıyor.
- macOS: an MLX model too large for the GPU share of unified memory is no longer started (it used to push the Mac into heavy swapping); the message gives the numbers and suggests a smaller model or quantization.
- macOS: birleşik belleğin GPU payına sığmayan MLX modeli artık başlatılmıyor (Mac'i ağır takas kullanımına sokuyordu); mesaj gereken ve kullanılabilir belleği gösterip daha küçük bir model ya da niceleme öneriyor.
- The library lists Spark-TTS and OuteTTS MLX folders again when they have no config.json, and shows their architecture correctly instead of "parakeet".
- Kütüphane, config.json'u olmayan Spark-TTS ve OuteTTS MLX klasörlerini yeniden listeliyor ve mimarilerini "parakeet" yerine doğru gösteriyor.
- Accessibility: the command palette is announced with its title, headings follow a consistent order, labelled status icons and groups have proper roles, and wide code blocks can be scrolled with the keyboard.
- Erişilebilirlik: komut paleti başlığıyla duyuruluyor, başlıklar tutarlı bir sırada, etiketli durum simgeleri ve gruplar doğru rollere sahip ve geniş kod blokları klavyeyle kaydırılabiliyor.
- MLX: long chats no longer exhaust memory; the prompt cache now has a size limit based on the memory the model leaves (it grew by a full conversation's cache per turn with Qwen3 and similar templates).
- MLX: uzun sohbetler artık belleği tüketmiyor; istem önbelleğinin, modelden kalan belleğe göre bir boyut sınırı var (Qwen3 ve benzeri şablonlarda her turda bütün konuşmanın önbelleği kadar büyüyordu).
- The main window appears about a third of a second sooner on Windows: the CPU sampler for the Performance view is now created on first use.
- Ana pencere Windows'ta yaklaşık üçte bir saniye daha erken açılıyor: Performans görünümünün CPU örnekleyicisi artık ilk kullanımda oluşturuluyor.
- Faster startup: repeated boot requests are shared and the Dashboard model page loads its data only when shown.
- Daha hızlı açılış: açılıştaki tekrarlı istekler paylaşılıyor, Pano model sayfası verisini yalnızca gösterildiğinde yüklüyor.

- audio.cpp: on the CPU backend, workers use all physical cores when no thread count is set (audio.cpp otherwise runs on one thread; Apollo on an 11 s clip went from 97 s to 32 s on an 8-core Ryzen).
- audio.cpp: CPU arka ucunda iş parçacığı sayısı ayarlanmamışsa işçiler tüm fiziksel çekirdekleri kullanır (audio.cpp aksi halde tek iş parçacığında çalışır; 8 çekirdekli Ryzen'de 11 sn'lik klipte Apollo 97 sn'den 32 sn'ye indi).
- Linux: the start-at-login entry quotes the app path (an AppImage in a folder with a space now starts), is written under `$XDG_CONFIG_HOME/autostart` when set, and is rewritten at each launch so a moved or renamed AppImage keeps starting.
- Linux: oturum açılışında başlatma girdisi uygulama yolunu tırnak içine alır (boşluklu klasördeki AppImage artık açılıyor), `$XDG_CONFIG_HOME` ayarlıysa `$XDG_CONFIG_HOME/autostart` altına yazılır ve her açılışta yeniden yazılır; taşınan ya da adı değişen AppImage açılmaya devam eder.
- Linux: the tray icon is a StatusNotifierItem, so activating it (left click on KDE, double click with GNOME's AppIndicator extension) opens the window; when the tray host starts after Harness (login autostart, extension turned on later), the icon appears once it does.
- Linux: tepsi simgesi StatusNotifierItem oldu; etkinleştirmek (KDE'de sol tık, GNOME AppIndicator eklentisinde çift tık) pencereyi açar. Tepsi sunucusu Harness'tan sonra gelirse (girişte otomatik başlatma, eklentinin sonradan açılması) simge o an görünür.
- Gateway: decision models. `POST /v1/systemone` (Ollama, LM Studio, llama.cpp format) and `POST /v1/decisions` (OpenAI, LM Studio format) answer yes/no, choice and score questions with probabilities. A native decision model (`<arch>.decision.type` in its GGUF, labelled "Decision model" in the Library) answers through llama-server's own `/v1/systemone`; any other chat model answers by emulation (one GBNF-constrained token per question, probabilities from logprobs; not calibrated), on by default and switchable in Settings > Gateway > Routing. `X-Harness-Decision-Mode` says which one answered; errors follow each API's own shape, and the same token, origin and JIT rules apply as on the other routes.
- Ağ geçidi: karar modelleri. `POST /v1/systemone` (Ollama, LM Studio, llama.cpp biçimi) ve `POST /v1/decisions` (OpenAI, LM Studio biçimi) evet/hayır, seçim ve puan sorularını olasılıklarla yanıtlar. Yerel karar modeli (GGUF'ta `<arch>.decision.type`; Kütüphane'de "Karar modeli" etiketi) llama-server'ın kendi `/v1/systemone` ucuyla yanıt verir; diğer sohbet modelleri emülasyonla yanıt verir (her soru için GBNF ile sınırlanmış tek token, olasılıklar logprobs'tan; kalibre değil). Emülasyon varsayılan olarak açıktır ve Ayarlar > Ağ Geçidi > Yönlendirme'den kapatılabilir. `X-Harness-Decision-Mode` hangisinin yanıtladığını söyler; hatalar her API'nin kendi biçimindedir, token, origin ve JIT kuralları diğer uçlarla aynıdır.
- Model Hub: a "Decision model" task filter lists the repositories tagged `decision-model` on Hugging Face, and their cards carry a "Decision model" label. A GGUF decision model counts as runnable on llama.cpp whatever its `pipeline_tag` says (many are tagged text-classification).
- Model Hub: "Karar modeli" görev filtresi Hugging Face'te `decision-model` etiketli depoları listeler; kartlarında "Karar modeli" etiketi bulunur. GGUF karar modelleri, `pipeline_tag` ne derse desin llama.cpp ile çalışabilir sayılır (çoğu text-classification etiketlidir).

- Image generation: GPU memory failures show localized guidance, blocking server names when available, and a Manage Servers action.
- Görsel üretimi: GPU belleği hataları yerelleştirilmiş yönlendirme, varsa belleği tutan sunucu adları ve Sunucuları Yönet eylemi gösterir.
- Playground: the Image tab header model chip follows the image model selection.
- Oyun Alanı: Görsel sekmesinin başlığındaki model etiketi seçili görsel modelini izler.
- Control API SDKs: TypeScript (`harness-studio-sdk`, openapi-fetch) and Python (`harness-studio-sdk`, httpx) clients in `sdk/`, generated from the OpenAPI document by `node sdk/generate.mjs` and tested against a real `harness serve` in CI. They find the running instance through `instance.json` and turn error answers into exceptions with the stable `code`. Not published to npm or PyPI yet. The OpenAPI document now gives every operation a unique `operationId`, documents 401/403 on every protected operation, types the conversation, compaction and Modelfile answers, and lists 22 operations' query parameters as `query` (they were listed as path parameters).
- Control API SDK'ları: `sdk/` altında TypeScript (`harness-studio-sdk`, openapi-fetch) ve Python (`harness-studio-sdk`, httpx) istemcileri. OpenAPI belgesinden `node sdk/generate.mjs` ile üretilir ve CI'da gerçek bir `harness serve` üzerinde sınanır. Çalışan örneği `instance.json` üzerinden bulur, hata yanıtlarını kararlı `code` değeriyle istisnaya çevirir. Henüz npm veya PyPI'da yayımlanmadı. OpenAPI belgesinde artık her işlemin benzersiz bir `operationId` değeri var, korumalı her işlemde 401/403 belgelenir, sohbet, sıkıştırma ve Modelfile yanıtları türlüdür ve 22 işlemin sorgu parametreleri `query` olarak listelenir (yol parametresi olarak görünüyorlardı).
- Model Hub: engine installation failures appear below the banner with expandable, scrollable output.
- Model Hub: motor kurulumu hataları banner altında genişletilebilir ve kaydırılabilir çıktıyla gösterilir.
- Model Hub: exact repository IDs are preserved in search and ranked first regardless of letter case.
- Model Hub: tam depo kimlikleri aramada korunur ve büyük/küçük harften bağımsız olarak ilk sıraya alınır.
- Model Hub: variant recommendations prefer current files over old, legacy and deprecated folders.
- Model Hub: varyant önerileri old, legacy ve deprecated klasörleri yerine güncel dosyaları tercih eder.
- Runtimes: a llama.cpp fork or registry release installs a CUDA 13 build on Windows only with NVIDIA driver 580 or newer (an older driver gets the CUDA 12 build, or the next backend), like the official install; its CUDA runtime companion is the one for the same OS (a release with the same CUDA version for Windows and Ubuntu no longer pulls the Linux archive onto Windows).
- Model Motorları: llama.cpp çatalı ya da kayıt sürümü Windows'ta CUDA 13 derlemesini yalnız NVIDIA sürücüsü 580 ve üstündeyse kurar (eski sürücü CUDA 12 derlemesini ya da sıradaki backend'i alır), resmî kurulumdaki gibi; CUDA çalışma zamanı eki aynı işletim sisteminin ekidir (aynı CUDA sürümünü Windows ve Ubuntu için yayımlayan bir sürüm Windows'a artık Linux arşivini indirmez).
- Out-of-memory recovery: on CUDA, ROCm and Apple Silicon the cache option stops at q8_0/q8_0, like the automatic choice; it no longer offers the mixed K q8_0 + V q4_0 pair, which those backends convert to f16 on every token (27-38% slower generation on CUDA, plus a conversion buffer the estimate did not count). The smaller-context option is the way down there.
- Bellek yetmeyince kurtarma: CUDA, ROCm ve Apple Silicon'da önbellek seçeneği otomatik seçim gibi q8_0/q8_0'da durur; bu backend'lerin her token'da f16'ya çevirdiği karışık K q8_0 + V q4_0 çiftini artık önermez (CUDA'da üretim %27-38 yavaş, ayrıca tahminin saymadığı bir dönüştürme arabelleği). Orada aşağı yol daha küçük bağlam seçeneğidir.

- Playground: per-chat Thinking levels and optional request budgets, model-aware OpenAI/Ollama translation, and copyable reasoning with saved duration and inline `<think>` handling. Presets remain independent; split view keeps each side's preset.
- Playground: sohbete özel Düşünme düzeyleri ve isteğe bağlı istek bütçesi, modele göre OpenAI/Ollama çevirisi, kayıtlı süre ve satır içi `<think>` desteğiyle kopyalanabilir düşünme metni. Ön ayarlar bağımsızdır; bölünmüş görünüm her tarafın ön ayarını kullanır.

- Experimental audio separation and enhancement with audio.cpp, behind the existing off-by-default switch. Playground > Speech has a new Process Audio panel (upload or record a WAV, play and save each output channel), and Model Hub marks the seven supported families as Experimental: BS-RoFormer, Mel-Band RoFormer, SAM Audio, Sidon, Apollo, AudioSR (GPU backend only) and UniverSR. The gateway's new `POST /v1/audio/process` accepts a WAV as multipart or base64, enforces size and duration limits, rejects unknown and path fields, and rebuilds the request for the worker; outputs come back inline and the worker's task, model and UI routes stay private. See [family details](en/FEATURES.md).
- Ses ayırma ve iyileştirme (deneysel, varsayılan kapalı audio.cpp anahtarının arkasında): Playground > Konuşma'da yeni Sesi İşle paneli (WAV yükleme veya kayıt, her çıktı kanalını dinleme ve kaydetme). Model Hub, desteklenen yedi aileyi Deneysel olarak işaretler: BS-RoFormer, Mel-Band RoFormer, SAM Audio, Sidon, Apollo, AudioSR (yalnız GPU arka ucu) ve UniverSR. Gateway'in yeni `POST /v1/audio/process` ucu WAV'ı multipart veya base64 olarak alır, boyut ve süre sınırı uygular, bilinmeyen ve yol alanlarını reddeder ve isteği işçi için yeniden kurar; çıktılar gövdede döner, işçinin görev, model ve arayüz uçları dışarı açılmaz.

- audio.cpp is experimental and off by default. Settings > Runtime > Installed Runtime > Other Runtimes requires risk confirmation: its local server has no authentication or Host/Origin checks and accepts server-side file paths. Managed platform builds serve TTS GGUF families through `/v1/audio/speech` with Playground voice selection from companion embeddings. The gateway exposes validated speech and processing requests only. Speech uses discovered voices and inline base64 references up to 5 MiB. Workers stop after five idle minutes.
- audio.cpp deneyseldir ve varsayılan olarak kapalıdır. Ayarlar > Model Motoru > Kurulu Model Motoru > Diğer Model Motorları altında risk onayıyla etkinleştirilir. Yönetilen platform derlemeleri TTS GGUF ailelerini `/v1/audio/speech` üzerinden çalıştırır; Playground yardımcı ses gömmelerini listeler. İşçiler beş dakika boş kalınca durur.
- Security: the gateway (OpenAI/Anthropic port, Ollama-compatible port) and the Control API now refuse browser requests from any origin other than the Harness window or the user's allowed browser origins (`403 forbidden_origin` / `api.forbidden_origin`), including preflights, `Origin: null` and cross-site requests without `Origin`. A web page can no longer make the machine load models with a "simple" cross-site POST. Clients without an `Origin` header (curl, SDKs, CLI) are unchanged.
- Güvenlik: ağ geçidi (OpenAI/Anthropic portu, Ollama uyumlu port) ve Control API, Harness penceresi ile kullanıcının izin verdiği tarayıcı origin'leri dışındaki tarayıcı isteklerini reddeder (`403 forbidden_origin` / `api.forbidden_origin`); ön uçuş, `Origin: null` ve `Origin` göndermeyen siteler arası istekler dahil. Bir web sayfası artık "basit" bir siteler arası POST ile makineye model yükletemez. `Origin` göndermeyen istemciler (curl, SDK'lar, CLI) değişmez.

Planned work is tracked in the [roadmap](en/ROADMAP.md).

### Added
- MLX audio engine (Apple Silicon): text to speech with Chatterbox Turbo, Soprano, VoxCPM2 and Spark-TTS besides Qwen3-TTS; the Hub marks their mlx-community repos "Speaks with MLX". Long texts now stop at the time limit on models that cannot stream.
- MLX ses motoru (Apple Silicon): Qwen3-TTS'in yanında Chatterbox Turbo, Soprano, VoxCPM2 ve Spark-TTS ile seslendirme; Hub bu ailelerin mlx-community depolarını "MLX ile konuşur" diye işaretler. Akış yapamayan modellerde uzun metinler artık süre sınırında durur.
- Documentation: corrected platform packages, runtime locations, context defaults, tool/task counts, MLX speech recognition, and Playground controls; aligned English and Turkish guides and locale navigation paths.
- Belgeler: platform paketleri, motor konumları, bağlam varsayılanları, araç/görev sayıları, MLX konuşma tanıma ve Playground denetimleri düzeltildi; İngilizce ve Türkçe kılavuzlar ile yerelleştirilmiş yollar eşitlendi.
- MLX audio: Whisper and Parakeet speech recognition on Apple Silicon (macOS 14+) through `/v1/audio/transcriptions`; Hub marks supported MLX repositories as running with mlx-audio.
- MLX ses: Apple Silicon (macOS 14+) üzerinde Whisper ve Parakeet ile `/v1/audio/transcriptions` üzerinden konuşma tanıma; Hub desteklenen MLX depolarını mlx-audio ile çalışır olarak işaretler.
- Linux releases: the workflow builds AppImage alongside the preview `.deb`; AppImage is the updater target.
- Linux sürümleri: iş akışı önizleme `.deb` yanında AppImage üretir; güncelleyici AppImage kullanır.
- Linux AMD APUs: integrated GPUs use a shared-memory budget even when the firmware reports a small dedicated carve-out.
- Linux AMD APU: tümleşik GPU, firmware küçük bir ayrılmış bellek bildirse bile paylaşılan bellek bütçesini kullanır.
- Python engine (python-hf) on NVIDIA GPUs (Windows, Linux x86-64): a CUDA build of its environment (`hf-cuda`, PyTorch 2.14.1 for CUDA 12.6, hash-pinned uv locks, about 2.8 GB on Windows). It is the one offered on a machine with an NVIDIA GPU, and a machine that already has the CPU build gets an "Install the CUDA build" button in Settings > Runtime > Installed Runtime. The engine uses the CUDA build when it is ready (the worker picks `cuda` itself), else the CPU one; each shows its size and can be removed.
- Python engine: diffusers image-to-image pipelines run (InstructPix2Pix, the SDXL refiner, Qwen-Image-Edit and other `Edit` classes), loaded as the diffusers class `model_index.json` names (diffusers' own pipelines only). They edit through `/v1/images/edits` and the Playground Image tab, which asks for a start image; a plain generation request gets a 400 saying so. FLUX.2 klein and FLUX.1 Kontext draw from text and edit with the same pipeline. Before this, InstructPix2Pix, Qwen-Image-Edit and FLUX.2 klein folders were taken for text-to-image and failed to load. Hub repos of either task with only component folders are listed as runnable on the Python engine; repos with a root checkpoint stay with stable-diffusion.cpp.
- ONNX engine on NVIDIA (Windows): the CUDA build of ONNX Runtime installs with the CUDA 12.9 runtime, cuBLAS, cuFFT and cuDNN 9 libraries (pinned, from NVIDIA's PyPI packages, about 2.1 GB), so the CUDA provider works without installing the CUDA Toolkit or changing PATH; once installed, `auto` uses it. A CUDA launch without that build runs on DirectML and says so in the server log.
- Model Hub: diffusers text-to-image pipelines (`segmind/tiny-sd` and other repos with `model_index.json` and component folders) run on the Python engine. They are listed as "Python, diffusers", or as needing the Python engine with a one-click install, and download as one folder: `model_index.json` and its components with one set of weights (fp16 safetensors when every component has them, else safetensors, PyTorch `.bin` only where a component has nothing else), without other variants, Flax, ONNX or root checkpoints. Repos with a single-file checkpoint stay with stable-diffusion.cpp. `.bin` weights load only through `torch.load(weights_only=True)` (the worker refuses them with a torch older than 2.6, and without them diffusers is told to use safetensors only); the card says so. The worker loads an fp16 download with `variant="fp16"`.
- Python engine image generation (diffusers): image-to-image and inpainting on the same text-to-image model through `/v1/images/edits` (OpenAI's multipart form or JSON; an OpenAI mask's transparent areas are repainted), with `strength`. Requests can pick the sampler (`euler`, `euler_a`, `heun`, `dpm2`, `dpm++2m`, `ddim`, `pndm`, `lms`, `unipc`, `lcm`, with `scheduler: karras` where it applies) and use the gateway's `steps` / `cfg_scale`. Two new model settings: **Largest image** (megapixels per request, 1 on the CPU and 4 on a GPU by default) and **Low memory** (sliced attention and VAE, on for CPU and Apple GPU by default). These models now open in the Playground Image tab, start image included.
- CrispASR TTS downloads now queue verified voices and dictionaries; missing helpers have localized errors and a download action.
- CrispASR TTS indirmeleri artık doğrulanmış sesleri ve sözlükleri sıraya alır; eksik yardımcı dosyalar için yerelleştirilmiş hata ve indirme düğmesi sunulur.
- Pocket TTS language restrictions now name CrispASR and its English-only voice support.
- Pocket TTS dil kısıtlamaları artık CrispASR motorunu ve yalnız İngilizce ses desteğini belirtir.
- Settings describe CrispASR recognition and Kokoro / Pocket speech generation in both languages.
- Ayarlar her iki dilde CrispASR konuşma tanıma ve Kokoro / Pocket ses üretimini açıklar.
- Windows Hub variants requiring unavailable eSpeak-ng are blocked; failed speech workers stop and report localized errors.
- Windows Hub listesinde eSpeak-ng bulunmadığında ilgili modeller engellenir; başarısız ses işçileri durdurulur ve yerelleştirilmiş hata gösterilir.
- Mixed audio.cpp family repositories no longer recommend one large variant across unrelated families.
- Birden fazla audio.cpp ailesi içeren depolarda farklı aileler arasından tek bir büyük model önerilmez.
- Experimental CrispASR TTS: Kokoro af_heart and Pocket TTS alba presets in Hub, library and Playground. Verified companion downloads, owned voice/cache folders, speech-only routing, strict JSON/voice validation and bounded generation; cloning remains disabled.
- Deneysel CrispASR metinden konuşma: Hub, kütüphane ve Playground listelerinde Kokoro af_heart ve Pocket TTS alba hazır sesleri. Doğrulanmış yardımcı dosyalar, yönetilen ses/önbellek klasörleri, yalnız konuşma yolu, sıkı JSON/ses doğrulaması ve sınırlı üretim; ses kopyalama kapalıdır.
- Managed CrispASR v0.8.43 (MIT) for CrispASR-format ASR GGUF models, with Hub variants, library detection, Playground Speech > Transcribe and `harness transcribe`. Each run uses a fresh environment-only key and loopback binding; the gateway rebuilds multipart with a strict field/route allowlist. Whisper, sherpa-onnx and ONNX CTC keep their engines. transcribe.cpp exports are unsupported except Canary 180M Flash. Translation runs as a transcription request with `translate=true` and is offered only for architectures that translate (Canary, Cohere, Voxtral, Granite Speech); other models get a 400. The worker never downloads anything: language detection and punctuation helper models (pinned revision, SHA-256) come through the verified downloader and Settings > Runtime > Installed Runtime > Other Runtimes > CrispASR Helper Models, each launch passes `--cache-dir` under the data folder, and the worker's proxy variables point at a closed port. Downloading or removing a helper restarts an idle gateway worker on its next request so it picks up the change.
- CrispASR v0.8.43: yalnız CrispASR biçimli konuşma tanıma GGUF modelleri; her çalıştırmaya özel ortam anahtarı, yalnız yerel bağlantı ve sıkı ağ geçidi alan/yol izin listesi. Hub, kütüphane, Playground ve CLI desteği. İşçi ağdan hiçbir şey indirmez: dil algılama ve noktalama yardımcı modelleri doğrulamalı indirici ile gelir (Ayarlar > Model Motoru > Kurulu Model Motoru > Diğer Model Motorları > CrispASR Yardımcı Modelleri), önbellek klasörü veri klasörü altındadır. Yardımcı model indirilince ya da kaldırılınca boştaki ağ geçidi işçisi sonraki istekte yeni ayarla yeniden başlar.
- Zero-shot object detection on the ONNX engine: OWL-ViT and OWLv2 exports find the labels a request names and return boxes on `/v1/hf/{model}`. OWLv2's square padding is applied. The Playground Tasks tab draws the boxes, and the Hub lists the task.
- Beam search for encoder-decoder models on the ONNX engine: `num_beams` from the model's config or the request (at most 8, sharing 1024 decoder rows) with transformers' length penalty and early stopping. BART summaries and Marian translations now match the quality of the transformers pipeline defaults.
- Summarization, translation and text2text generation on the ONNX engine: T5, BART, Marian, Pegasus and Blenderbot exports answer `/v1/hf/{model}` with greedy decoding through the shared decoder. The Playground Tasks tab takes a text, and the Hub marks the tasks as runnable.
- Image captioning on the ONNX engine: ViT + GPT-2 style encoder-decoder exports answer `image-to-text` on `/v1/hf/{model}`. Greedy decoding uses the KV cache, and `max_new_tokens` is capped at 256. The downloader keeps each encoder with its decoder, and the Playground Tasks tab shows the caption.
- Image feature extraction on the ONNX engine: bare image backbones (DINOv2, ViT, ConvNeXt, Swin and others) answer `image-feature-extraction` on `/v1/hf/{model}` with their hidden states, or with the pooled vector when `pool` is set. The Playground Tasks tab shows the result, and the Hub marks the task as runnable.
- Audio classification on the ONNX engine: AST (AudioSet) and wav2vec2 / HuBERT / WavLM classifier exports answer `audio-classification` on `/v1/hf/{model}` with ranked labels, with no Python. The AST filter banks are computed in Rust. The Playground Tasks tab takes an audio file (WAV, MP3, FLAC, OGG), and the Hub marks the task as runnable.
- MLX audio engine (mlx-audio) for Apple Silicon (macOS 14+): Qwen3-TTS text-to-speech models from mlx-community speak through the gateway's /v1/audio/speech, with their preset voices and speed. OpenAI voice names get the default voice. It shares the MLX vision engine's environment, and the Model Hub marks those repos "Speaks with MLX". There is no voice cloning, and the worker never reads a file path from a request.
- MLX vision engine (mlx-vlm) for Apple Silicon (macOS 14+): chat with images using MLX-format vision-language models from mlx-community (Qwen3-VL, Gemma 3 and others) through the gateway and the Playground. It installs from Settings > Runtime > Installed Runtime > Other Runtimes as its own managed Python environment (about 760 MB); the Model Hub marks those repos "Runs with MLX (vision)". Images are accepted only inline (data: URLs). The worker never fetches a link or reads a file path a request names; it listens on 127.0.0.1 only and asks for each run's key.
- Text to speech: Meta MMS voices as sherpa-onnx exports them (`vits-mms-<language>` folders: English, German, French, Spanish, Russian, Ukrainian, Thai, Min Nan) are recognized in the model folders. They are not offered for download, since their license is non-commercial.
- MLX engine for Apple Silicon (macOS 14+): MLX-format chat models from mlx-community, including ones with no GGUF, run through the gateway with chat, streaming and tool calls. It installs from Settings > Runtime > Installed Runtime > Other Runtimes as a managed Python environment (about 330 MB); the Model Hub marks those repos "Runs with MLX". The worker listens on 127.0.0.1 only and asks for each run's key; macOS has no network isolation for it.
- ONNX task models without Python: text classification, zero-shot classification, named entity recognition, fill-mask, and question answering. They run through the gateway at `/v1/hf/{model}` in the Hugging Face Inference API format and in the new Playground **Tasks** tab (Advanced). The Model Hub marks these tasks as runnable with ONNX Runtime.
- Speech to text: NVIDIA Nemotron 3.5 ASR streaming (35 languages including Turkish, with punctuation) and Cohere Transcribe (14 languages) from the sherpa-onnx catalog. Streaming transducer folders are no longer skipped; they run with sherpa-onnx's online decoder.
- Speech to text: speaker diarization for the sherpa-onnx models. `response_format=diarized_json` (or the `speakers` field) on `/v1/audio/transcriptions`, and `harness transcribe --diarize [--speakers N]`, say who spoke each segment (A, B...); SRT and VTT carry the speaker too. Two small models (35 MB) download on first use.
- Playground: audio input for llama.cpp models whose projector (mmproj) accepts audio (for example Ultravox, Qwen2.5-Omni, Voxtral). Attach a WAV, MP3 or FLAC file (up to 4 MB, two per message) or record from the microphone; the clip is sent as an OpenAI `input_audio` part, plays back in the chat and is kept with the conversation. The capability check reports whether a model accepts audio, and Model Hub explains which speech models run.
- Video generation through the gateway: OpenAI's videos API (`POST /v1/videos`, `GET /v1/videos`, `GET /v1/videos/{id}`, `GET /v1/videos/{id}/content`, `DELETE /v1/videos/{id}`) on stable-diffusion.cpp's Wan models. A video is a job (it takes minutes): one runs at a time, finished files stay in the data folder for 7 days, and a token sees only its own jobs. The Wan2.1 T2V 1.3B bundle downloads its VAE and text encoder with it. New token endpoint group `videos` (existing restricted tokens do not get it).
- ONNX vision models: image classification, object detection, semantic segmentation, and depth estimation (ViT, DETR, SegFormer, Depth Anything and similar transformers.js exports). The Playground **Tasks** tab takes an image, draws the detected boxes, overlays the masks, and shows the depth map.
- ONNX zero-shot image classification: CLIP and SigLIP score an image against labels you name (`candidate_labels`), through `/v1/hf/{model}` and the Playground **Tasks** tab. The Model Hub offers the joint graph of these repos, not their single towers.
- Python engine (`python-hf`, optional): Hugging Face task models with no GGUF or ONNX build (classification, NER, question answering, summarization, translation, image classification, detection, segmentation, depth, captioning, visual question answering, speech recognition, audio classification, text to speech, and diffusers text to image) run on transformers from a managed CPU environment you install in one click (Settings > Runtime > Installed Runtime > Other Runtimes, about 1.1 GB). They are listed in the library with their task, run through the gateway at `/v1/hf/{model}`, `/v1/embeddings` and `/v1/rerank`, and the Model Hub marks such repositories as runnable. Remote code is never run, and on Linux the model runs without network access.
- Gateway: `/v1/audio/transcriptions` (and `/v1/audio/translations`), `/v1/audio/speech` and `/v1/images/generations` also serve Python engine models (speech recognition, text to speech, diffusers text to image). whisper.cpp, sherpa-onnx and stable-diffusion.cpp models keep their own engines; a sherpa-onnx voice wins over a Python model of the same name.
- Speech recognition with Hugging Face CTC models (wav2vec2, HuBERT, WavLM, Data2Vec Audio, UniSpeech) exported to ONNX, for example `Xenova/wav2vec2-base-960h`. Model Hub recognizes them and offers the FP32 and quantized exports. They run in the ONNX sidecar (DirectML, CUDA or CPU) behind the gateway's `/v1/audio/transcriptions`, in Playground > Speech, in chat dictation and in `harness transcribe`. Long recordings are split into 20-second pieces with overlap, and segment times break at words or pauses. Source-only speech repositories show "Needs Conversion".

### Changed
- Model Hub: CTranslate2 Whisper exports (faster-whisper) now point to the matching whisper.cpp build, with an Open whisper.cpp Build button that preselects the right file.
- Model Hub: CTranslate2 Whisper çıktıları (faster-whisper) artık eşleşen whisper.cpp sürümünü gösterir; whisper.cpp Sürümünü Aç düğmesi doğru dosyayı seçili getirir.
- Documentation: Windows uninstall data retention (Delete the application data option), registry key, full-removal steps (CLI login service, Credential Manager), HIP/Vulkan fallback, DirectML on Intel, sign-in autostart wording, and NSIS/MSI notes corrected in the READMEs, guides, and locale strings.
- Dokümantasyon: Windows kaldırma veri koruması (Uygulama verilerini sil seçeneği), kayıt defteri anahtarı, tam kaldırma adımları (CLI oturum servisi, Kimlik Bilgileri Yöneticisi), HIP/Vulkan yedeği, Intel'de DirectML, oturum açınca başlatma ifadesi ve NSIS/MSI notları README'lerde, kılavuzlarda ve yerel metinlerde düzeltildi.
- App update (Windows): running servers, Lemonade, MCP servers and pending analytics are now stopped cleanly after the update is downloaded and verified and before the installer starts; previously the updater exited the app first and only the Job Object ended them.
- Uygulama güncellemesi (Windows): çalışan sunucular, Lemonade, MCP sunucuları ve bekleyen analitik verileri, güncelleme indirilip doğrulandıktan sonra ve kurucu başlamadan önce temiz biçimde durdurulur; önceden güncelleyici uygulamayı önce kapatıyor, yalnızca Job Object bunları sonlandırıyordu.
- Labels: the download queue's resume button reads "Devam Et" (Title Case) in Turkish; the tool workbench help names the real "Generate Answer" button.
- Etiketler: indirme kuyruğundaki devam düğmesi "Devam Et" olarak yazılır; araç çalışma alanı yardımı gerçek "Yanıtı Üret" düğmesini anar.
- Model Hub: repositories no engine can run now show a precise reason (own package, unsupported architecture, safetensors only, CTranslate2 export, Apple only, pipeline config only, component only) instead of a generic "no GGUF file".
- Model Hub: hiçbir motorun çalıştıramadığı depolar artık genel "GGUF dosyası yok" yerine kesin bir neden gösterir (kendi paketi, desteklenmeyen mimari, yalnızca safetensors, CTranslate2 dışa aktarımı, yalnızca Apple, yalnızca boru hattı ayarı, yalnızca parça).
- Model Hub: cards show a short refusal label (for example "Needs Its Own Package"); the full reason stays in the tooltip and the drawer.
- Model Hub: kartlar kısa bir ret etiketi gösterir (ör. "Kendi Paketi Gerekli"); tam gerekçe ipucunda ve çekmecede kalır.
- Gateway loads on small cards and CUDA: a model the gateway loads gets 4 concurrent requests whenever its context is 8k or more (it was context / 8k, so a 4 GB card's Qwen3-4B at 12k ran 4 streams one after another: 54 t/s in total, up to 14.5 s to the first token; now 148 t/s and 0.8 s); the gateway still holds requests that do not fit the shared pool, and each request can still use the whole context. On CUDA a unified pool keeps llama.cpp's host prompt cache on (it clears idle slots; without it a single stream after concurrent use ran 10-40 % slower, the 0.6B 157.7 against 183 t/s); other backends keep `--cache-ram 0`.
- Playground: with tools on, a new conversation on a model a local Harness server has loaded sends its fixed prefix (system prompt, tool guidance, tool definitions; about 2.2k tokens) once ahead with one output token, so llama-server's prompt cache holds it and the first message processes only the user's text (Qwen3-4B on a CPU: first token 61 s → 0.5 s). Only to loopback servers, once per conversation; the first message waits for a warm-up still in flight and is never slower. Tool definitions are listed in a fixed order (built-ins first, then by name), so the prefix does not change with registration order. The warm-up is not counted in the Dashboard, Analytics or traces: the gateway drops the record of a one-token, non-streamed request marked `x-harness-purpose: warmup`, and only for Harness's own client token.
- Speech to text on Linux (whisper.cpp ships a CPU-only build there): onboarding and the Hub variant drawer recommend whisper `small` before `large-v3-turbo` (an 11 s clip: 4.2 s instead of 18.9 s with the language given). The Playground Speech tab starts with the interface language as the audio language, so whisper skips language detection (it doubles the time on a CPU: 7.3 s vs 4.2 s with `small`); clearing it detects the language.
- Linux: CrispASR, audio.cpp, the ONNX sidecar and the sherpa-onnx speech tools run without network (a new user + network namespace, as the Python engine does). The servers only listen on TCP, so Harness runs them behind a small bridge inside the namespace: it brings up the namespace's own loopback, starts the server there and passes one private Unix socket to it, which Harness relays the server's port to. Nothing inside reaches the internet, the LAN or services on this machine's loopback. Where unprivileged user namespaces are off (some containers and hardened kernels), they run as before and the server log says so.
- Playground: with a model under 5B parameters, tools are offered with short descriptions (first sentence; names, parameters and required fields unchanged). Qwen3-0.6B picked the right tool in 49 of 60 test prompts instead of 36, with about 440 fewer prompt tokens; Qwen3-4B stayed at 60 of 60.

### Fixed
- Settings shows each ONNX Runtime build's download size and size on disk as two named figures; the CUDA build no longer appears as ~2.1, 1.8 and 2.5 GB in three places. The engine card shows ONNX Runtime as installed after the first ONNX model installs it automatically. A damaged ONNX Runtime install (files missing or changed in size) is named, with an Install Again button, instead of reading as not installed.
- Ayarlar her ONNX Runtime yapısının indirme boyutunu ve disk boyutunu iki ayrı, adlandırılmış değer olarak gösterir; CUDA yapısı artık üç yerde ~2,1, 1,8 ve 2,5 GB olarak görünmez. İlk ONNX modeli ONNX Runtime'ı otomatik kurduktan sonra motor kartı onu kurulu gösterir. Bozuk ONNX Runtime kurulumu (eksik ya da boyutu değişmiş dosyalar) "kurulu değil" sayılmak yerine dosya adlarıyla ve Yeniden Kur düğmesiyle gösterilir.
- Dashboard: the first memory reading after start is no longer shown as 0 B on a busy machine.
- Dashboard: yoğun bir makinede başlangıçtan sonraki ilk bellek okuması artık 0 B görünmez.
- Windows: the start-at-login entry quotes the app path (the default install folder has a space), is always written for the current user (never all users, even when the app runs elevated), and an earlier unquoted entry is rewritten at launch. A start-at-login status that cannot be read is shown as an error in Settings instead of as off.
- Windows: oturum açılışında başlatma girdisi uygulama yolunu tırnak içine alır (varsayılan kurulum klasöründe boşluk var), yönetici olarak çalışsa bile her zaman yalnız geçerli kullanıcı için yazılır (tüm kullanıcılar için asla) ve önceki tırnaksız girdi açılışta yeniden yazılır. Okunamayan oturum açılışı durumu Ayarlar'da kapalı yerine hata olarak gösterilir.
- Closing the window again while the "Close Harness Studio?" question is open now only brings the window forward instead of quitting the app.
- "Harness Studio kapatılsın mı?" sorusu açıkken pencereyi yeniden kapatmak artık uygulamadan çıkmaz, yalnızca pencereyi öne getirir.
- Model paths with non-ASCII characters (for example Turkish İ and ı) are no longer truncated when saved as portable paths; the relative part is now cut by path components.
- ASCII olmayan karakterler içeren model yolları (örneğin Türkçe İ ve ı) taşınabilir yol olarak kaydedilirken artık kesilmez; göreli kısım yol bileşenlerine göre ayrılır.
- Installing the background service from a network (UNC) executable path now keeps the correct `\server\share\...` path.
- Arka plan hizmeti ağ (UNC) üzerindeki bir çalıştırılabilir dosyadan kurulurken doğru `\server\share\...` yolu korunur.
- NVIDIA CUDA and Vulkan driver detection on Windows now uses the real system directory instead of a fixed `C:\Windows`.
- Windows'ta NVIDIA CUDA ve Vulkan sürücü algılaması artık sabit `C:\Windows` yerine gerçek sistem dizinini kullanır.
- The built-in shell tool now reports a clear error for a network (UNC) working directory instead of running in a different directory.
- Yerleşik kabuk aracı, ağ (UNC) çalışma dizininde farklı bir dizinde çalışmak yerine açık bir hata bildirir.
- Python engines: on Windows a data folder whose path would exceed MAX_PATH (long paths off) is refused before any download with a clear message instead of a traceback at the verify step.
- Python motorları: Windows'ta yolu MAX_PATH'i aşacak veri klasörü (uzun yol desteği kapalıyken) indirmeden önce, doğrulama adımında çıkan hata yerine açık bir iletiyle reddedilir.
- Model Hub: original VibeVoice packages without the Transformers layout show a conversion requirement.
- Model Hub: Transformers düzeni olmayan özgün VibeVoice paketleri dönüştürme gereksinimini gösterir.
- Model Hub: AuK listings report the unsupported multi-GGUF package instead of advertising runnable variants.
- Model Hub: AuK listeleri çalışabilir varyantlar göstermek yerine desteklenmeyen çoklu GGUF paketini açıklar.
- Model Hub: SDXL refiner uses its Diffusers folder; native single-file refiner checkpoints are unsupported.
- Model Hub: SDXL refiner Diffusers klasörünü kullanır; yerel tek dosyalı refiner modelleri desteklenmez.
- Image generation: Qwen Image Edit 2511 bundles include the required vision projector.
- Görsel üretimi: Qwen Image Edit 2511 paketleri gerekli görüntü projektörünü içerir.
- Model Hub: text encoder and VAE folders are excluded from main model recommendations.
- Model Hub: metin kodlayıcı ve VAE klasörleri ana model önerilerinden çıkarılır.
- Settings > Gateway: typing a default idle time no longer crashes the window; the value is saved.
- Ayarlar > Ağ Geçidi: varsayılan boşta kalma süresi yazınca pencere çökmüyor; değer kaydediliyor.
- Starting a second server while another one holds GPU memory: when no context keeps the whole model on the GPU, it takes only the layers the free memory holds (the rest on the CPU) and says so in its log. Before, it took every layer the empty card would hold; Windows then paged both servers on every switch (1.3-2 s on a 4 GB card) and drivers that cannot page failed the load.
- Başka bir sunucu GPU belleğini tutarken ikinci sunucu başlatma: hiçbir bağlam modelin tamamını GPU'da tutamıyorsa yalnız boş belleğin aldığı kadar katman GPU'ya gider (kalanı CPU'da) ve log bunu söyler. Önce boş kartın alacağı kadar katman alıyordu; Windows her geçişte iki sunucuyu da sayfalıyordu (4 GB kartta 1,3-2 sn), sayfalayamayan sürücülerde yükleme başarısız oluyordu.
- Model Hub: VieNeu v3 Turbo is listed as not runnable because it needs the SEA-G2P text frontend, which is not supported yet.
- Model Hub: VieNeu v3 Turbo, henüz desteklenmeyen SEA-G2P metin ön işleyicisi gerektiği için çalıştırılamaz olarak listelenir.
- Speech: audio.cpp worker errors include the actual runtime error in server logs and response details.
- Konuşma: audio.cpp işçi hataları, sunucu günlüklerinde ve yanıt ayrıntılarında gerçek çalışma zamanı hatasını içerir.
- Model Hub: native Nemotron ASR folders take precedence over unsupported GGUF conversions and show a precise refusal because the Python engine does not include the required librosa dependency (also applies to Parakeet processors).
- Model Hub: yerel Nemotron ASR klasörleri desteklenmeyen GGUF dönüşümlerinden önce seçilir ve Python motorunda gerekli librosa bağımlılığı bulunmadığından kesin bir çalıştırılamama nedeni gösterilir (Parakeet işlemcileri için de geçerlidir).
- Model Hub: full Lightning image checkpoints are offered separately from LoRAs, with complete Qwen Edit 2511 companions and settings.
- Model Hub: tam Lightning görsel ağırlıkları LoRA dosyalarından ayrı sunulur; Qwen Edit 2511 yardımcı dosyaları ve ayarları tamamlanır.
- Model Hub: exact repository searches include non-GGUF repositories alongside existing results.
- Model Hub: tam depo aramaları mevcut sonuçların yanında GGUF dışındaki depoları da gösterir.
- Model Hub: task support includes installed Python image and speech engines and refreshes when returning to the Hub.
- Model Hub: görev desteği kurulu Python görsel ve konuşma motorlarını kapsar ve Hub'a dönüşte yenilenir.
- Image and video generation: a request without a seed (the Playground's "Random" seed, OpenAI clients) gets a random one. sd-server used its fixed default 42, so the same prompt always gave the same image.
- Görsel ve video üretimi: tohumsuz istek (Playground'da "Rastgele" tohum, OpenAI istemcileri) rastgele tohum alıyor. sd-server sabit varsayılanı 42'yi kullanıyordu; aynı istem hep aynı görseli veriyordu.
- Windows: Python engine environments (python-hf, conversion) install on machines where uv cannot follow its own Python link under %APPDATA% (seen on Windows 11 25H2); the venv is made from the unpacked Python directly. Before, the install failed every time with "Missing expected target directory for Python minor version link".
- Windows: Python motor ortamları (python-hf, dönüştürme), uv'nin %APPDATA% altındaki kendi Python bağlantısını izleyemediği makinelerde de kuruluyor (Windows 11 25H2'de görüldü); sanal ortam açılmış Python'dan doğrudan kuruluyor. Önce kurulum her seferinde "Missing expected target directory for Python minor version link" ile düşüyordu.
- On a GPU smaller than an image model (Z-Image Turbo with its text encoder on a 4 GB card), the gateway no longer refuses the generation. stable-diffusion.cpp keeps what does not fit in system memory, so the model loads and runs more slowly.
- Görsel modelden küçük bir GPU'da (4 GB kartta metin kodlayıcısıyla Z-Image Turbo) ağ geçidi üretimi artık reddetmiyor. stable-diffusion.cpp sığmayanı sistem belleğinde tutuyor; model yükleniyor ve daha yavaş çalışıyor.
- When the GPU memory is short, the gateway no longer tells you to stop a server that runs on the CPU and frees no GPU memory.
- GPU belleği yetmediğinde ağ geçidi artık CPU'da çalışan ve GPU belleği boşaltmayacak bir sunucuyu durdurmanızı önermiyor.
- The Hub's fit column for image models (stable-diffusion.cpp) now counts the VAE and text encoder the model loads with and follows the file size. A model larger than the GPU shows "Partial offload" instead of a misleading "Fits in VRAM".
- Hub'ın donanım uyumu sütunu görsel modellerde (stable-diffusion.cpp) artık modelle yüklenen VAE ve metin kodlayıcıyı sayıyor ve dosya boyutuyla tutarlı. GPU'dan büyük model yanıltıcı "VRAM'e sığıyor" yerine "Kısmi aktarım" gösteriyor.
- An image model larger than the GPU is loaded with part of its weights in system memory only when that memory is free and the installed stable-diffusion.cpp build supports it. A GPU-memory error no longer names servers known to run on the CPU, and still names those whose GPU use is unknown.
- GPU'dan büyük bir görsel model, ağırlıklarının bir kısmı sistem belleğinde olacak şekilde yalnız o bellek boşsa ve kurulu stable-diffusion.cpp derlemesi bunu destekliyorsa yükleniyor. GPU belleği hatası artık CPU'da çalıştığı bilinen sunucuları göstermiyor; GPU kullanımı bilinmeyenleri göstermeye devam ediyor.
- The Hub no longer shows "Partial offload" for an image model too large for the GPU when the installed stable-diffusion.cpp build cannot keep weights in system memory. The fit column no longer flickers back to an estimate that leaves out the model's VAE and text encoder.
- Kurulu stable-diffusion.cpp derlemesi ağırlıkları sistem belleğinde tutamıyorsa Hub, GPU'ya sığmayan görsel model için artık "Kısmi aktarım" göstermiyor. Uyum sütunu da modelin VAE ve metin kodlayıcısını saymayan eski bir tahmine geri dönmüyor.
- Updates: when the installer cannot start after the running servers were stopped, the error says so and asks to start them again.
- Güncellemeler: çalışan sunucular durdurulduktan sonra yükleyici başlatılamazsa hata bunu belirtir ve sunucuların yeniden başlatılmasını ister.
- Linux updater: a .deb install was offered "Install Update" like the AppImage. The updater then downloaded the AppImage and rejected it, or asked for pkexec/sudo dpkg. A .deb install now says it cannot update itself and opens the releases page instead. An AppImage updated after a login start no longer comes back hidden in the tray (`--minimized` is not passed to the new version).
- Linux güncelleyici: .deb kurulumuna AppImage gibi "Güncellemeyi Kur" sunuluyordu; güncelleyici AppImage'ı indirip reddediyor ya da pkexec/sudo dpkg istiyordu. .deb kurulumu artık kendini güncelleyemeyeceğini söylüyor ve sürümler sayfasını açıyor. Oturum açılışında başlamış bir AppImage güncellemeden sonra tepside gizli açılmıyor (`--minimized` yeni sürüme geçirilmiyor).
- Linux, AMD APUs: GPU memory showed only the BIOS carve-out (for example 467/512 MB), and a fully offloaded llama-server about 15 MB, because an APU's GPU buffers live in GTT. On integrated AMD GPUs the Dashboard total and the per-process breakdown now count VRAM plus GTT (from `mem_info_gtt_*` and the fdinfo `drm-resident-gtt`). Discrete cards are unchanged.
- Linux, AMD APU'lar: GPU belleği yalnız BIOS ayırmasını (ör. 467/512 MB), tamamen GPU'ya yüklü bir llama-server'ı da yaklaşık 15 MB gösteriyordu; APU'da GPU tamponları GTT'de durur. Tümleşik AMD GPU'larda Dashboard toplamı ve süreç dökümü artık VRAM ile GTT'yi birlikte sayıyor (`mem_info_gtt_*` ve fdinfo `drm-resident-gtt`). Ayrık kartlar değişmedi.
- Gateway: when other programs hold every port of 11440-11499, loading a model answers "no free port for its server in 11440-11499" (localized in the app) instead of the raw key `servers.no_free_port` under a stale code of an earlier run such as `servers.crashed`.
- Ağ geçidi: 11440-11499 aralığındaki portların hepsini başka programlar tutarken model yüklemesi, önceki bir çalışmadan kalan `servers.crashed` gibi bir kodla ham `servers.no_free_port` anahtarı yerine "bu aralıkta boş port yok" diyor (uygulamada Türkçe).
- Downloader: after a restart, paused downloads show the bytes already on disk (partial file and finished shards) instead of 0 B.
- İndirici: yeniden başlatmadan sonra duraklatılmış indirmeler 0 B yerine diskteki bayt miktarını (kısmi dosya ve biten parçalar) gösterir.
- Model Hub: the recommended variant is never a vision projector, a draft / MTP / assistant model, an image VAE or text encoder, or a LoRA adapter; the simplified drawer no longer falls back to one of them.
- Model Hub: önerilen varyant artık görüntü projektörü, taslak / MTP / asistan modeli, görüntü VAE'si veya metin kodlayıcısı ya da LoRA bağdaştırıcısı olmaz; basit çekmece de bunlara geri düşmez.
- Model Hub: safetensors repos whose architecture mistral.rs does not serve (GPT-2, GPT-NeoX, draft models) are no longer listed as runnable on mistral.rs.
- Model Hub: mistral.rs'in desteklemediği mimarideki (GPT-2, GPT-NeoX, taslak modeller) safetensors depoları artık mistral.rs üzerinde çalışır olarak gösterilmez.
- Model Hub: downloading a main variant of a vision repo now also queues its best matching projector (mmproj: F16, then BF16, F32, any) into the same folder, so the model can see images after a clean download.
- Model Hub: görsel (vision) bir deponun ana varyantını indirmek artık en uygun projektörünü (mmproj: F16, sonra BF16, F32, herhangi biri) de aynı klasöre indirmeye ekler; temiz bir indirmeden sonra model görselleri görebilir.
- Image companions kept in a repo sub-folder (the FLUX.2 VAE, the Wan 2.1 VAE, the Qwen-Image VAEs in `split_files/vae/` or `vae/`) download again: the bundle looked them up in the Hub variant list, which only holds root files, and failed with "variant not found".
- Depo alt klasöründeki görsel yardımcı dosyaları (`split_files/vae/` veya `vae/` içindeki FLUX.2 VAE, Wan 2.1 VAE ve Qwen-Image VAE'leri) yeniden iniyor: paket bunları yalnızca kök dosyaları içeren Hub varyant listesinde arıyor ve "varyant bulunamadı" hatası veriyordu.
- Model Hub: Qwen-Image (2.1, 2.1 Turbo, 2512, Edit 2509/2511), Z-Image, FLUX.2 dev and FLUX.1 Kontext downloads now bring the VAE and text encoder stable-diffusion.cpp needs (image bundles), instead of the diffusion model alone.
- Model Hub: Qwen-Image (2.1, 2.1 Turbo, 2512, Edit 2509/2511), Z-Image, FLUX.2 dev ve FLUX.1 Kontext indirmeleri artık yalnız difüzyon modelini değil, stable-diffusion.cpp'nin ihtiyaç duyduğu VAE ve metin kodlayıcısını da getirir (görsel paketleri).
- audio.cpp on integrated GPUs: on a Ryzen 4700U, Vulkan processing timed out the amdgpu compute ring and the GPU was reset, and Process Audio only said "invalid or unsafe audio result". Auto now uses the CPU when there is no discrete GPU (Vulkan can still be chosen per model). A lost GPU is reported as "The GPU driver stopped the job; try the CPU backend". Long audio gets a longer processing timeout (at most an hour), and a timeout is shown as a timeout. Process Audio errors are shown in the app's language.
- Tümleşik GPU'da audio.cpp: Ryzen 4700U'da Vulkan ile ses işleme amdgpu hesaplama kuyruğunu zaman aşımına uğratıyor ve GPU sıfırlanıyordu; Sesi İşle yalnız "invalid or unsafe audio result" diyordu. Ayrık GPU yoksa otomatik seçim artık CPU (Vulkan model başına yine seçilebilir). GPU kaybı "GPU sürücüsü işlemi durdurdu; CPU arka ucunu deneyin" diye bildiriliyor. Uzun seste işleme zaman aşımı uzuyor (en fazla bir saat) ve zaman aşımı zaman aşımı olarak gösteriliyor. Sesi İşle hataları uygulamanın dilinde.
- Model Hub: a repo with only float files (FP16/BF16/FP32 safetensors) now recommends the FP16/BF16 file that fits instead of the largest FP32 or unlabelled one, and a file with an unparsed precision no longer beats a known quantization.
- Model Hub: yalnızca kayan nokta dosyası (FP16/BF16/FP32 safetensors) olan bir depo artık en büyük FP32 ya da etiketsiz dosya yerine sığan FP16/BF16 dosyasını önerir; hassasiyeti çözülemeyen dosya bilinen bir nicemlemeyi artık geçemez.
- Model Hub: a whole-repo download keeps one weight set (the default `model.safetensors`), no longer adding fp32 or alternative-layout copies of the same model (whisper-large-v3, csm-1b).
- Model Hub: tüm depo indirmesi tek bir ağırlık takımı tutar (varsayılan `model.safetensors`); aynı modelin fp32 veya alternatif düzen kopyaları artık eklenmez (whisper-large-v3, csm-1b).
- Model Hub: repos with PrismML-fork quants (PQ2_0 / PTQ1_0, e.g. Ternary Bonsai) show a note in the variant drawer that the PrismML llama.cpp build is needed, with a button to Settings > Registry. They stay runnable.
- Model Hub: PrismML fork kuantlarına (PQ2_0 / PTQ1_0, örneğin Ternary Bonsai) sahip depolarda varyant çekmecesi PrismML llama.cpp derlemesinin gerektiğini söyleyen bir not ve Ayarlar > Registry düğmesi gösterir. Depolar çalıştırılabilir kalır.
- Model Hub: repos with PrismML-fork quants (PQ2_0 / PTQ1_0, e.g. Ternary Bonsai) show a note in the variant drawer that the PrismML llama.cpp build is needed, with a button to Settings > Runtime > Registry. They stay runnable.
- Model Hub: PrismML fork kuantlarına (PQ2_0 / PTQ1_0, örneğin Ternary Bonsai) sahip depolarda varyant çekmecesi PrismML llama.cpp derlemesinin gerektiğini söyleyen bir not ve Ayarlar > Model Motoru > Registry düğmesi gösterir. Depolar çalıştırılabilir kalır.
- Model Hub: PrismML-fork quants (PQ2_0 / PTQ1_0) are now parsed from file names instead of showing as unknown.
- Model Hub: PrismML fork kuantları (PQ2_0 / PTQ1_0) artık dosya adından okunuyor, "bilinmiyor" görünmüyor.
- Model Hub: IP-Adapter repos (h94/IP-Adapter, h94/IP-Adapter-FaceID) are marked as adapters that don't run here, like LoRAs.
- Model Hub: IP-Adapter depoları (h94/IP-Adapter, h94/IP-Adapter-FaceID) LoRA gibi burada çalışmayan adaptör olarak işaretlenir.
- Linux AppImage: programs Harness starts (model engines, Python, uv, terminals, agent CLIs, shell commands) inherited the AppImage's `LD_LIBRARY_PATH`, `PATH`, GTK, GStreamer and GDK paths and could load its bundled Ubuntu 22.04 libraries instead of their own. They now get the environment without the AppImage mount; the user's own entries are kept.
- Linux AppImage: Harness'ın başlattığı programlar (model motorları, Python, uv, terminaller, ajan CLI'ları, kabuk komutları) AppImage'ın `LD_LIBRARY_PATH`, `PATH`, GTK, GStreamer ve GDK yollarını devralıyor ve kendi kitaplıkları yerine paketteki Ubuntu 22.04 kitaplıklarını yükleyebiliyordu. Artık AppImage bağlama noktası çıkarılmış ortamla başlıyorlar; kullanıcının kendi girdileri korunuyor.
- Dashboard memory bars: the legend of a GPU no longer lists servers with 0 B on it (on a laptop with an NVIDIA card and an Intel iGPU, the iGPU legend listed every running server at 0 B · 0%).
- Gateway: a load refused for GPU memory reports what admission needed, the estimate plus the memory reserve, so the message no longer reads "needs about 11989 MiB, 12903 MiB free" for a model that does not fit.
- Ağ geçidi: GPU belleği yüzünden reddedilen yüklemede bildirilen ihtiyaç artık kabulde karşılaştırılan değer (tahmin ve bellek payı); sığmayan bir model için "needs about 11989 MiB, 12903 MiB free" gibi çelişkili bir mesaj çıkmıyor.
- Downloader: GGUF magic is checked only for `.gguf` files; companion files download with consistent file counters and byte progress.
- İndirici: GGUF sihirli baytları yalnız `.gguf` dosyalarında denetlenir; yardımcı dosyalar tutarlı dosya sayacı ve bayt ilerlemesiyle indirilir.
- Model Hub: the results heading follows the active search while its listing loads.
- Model Hub: sonuç başlığı, liste yüklenirken etkin aramayı izler.
- Settings > Updates: release dates use the interface language's date format.
- Ayarlar > Güncellemeler: sürüm tarihleri arayüz dilinin tarih biçiminde gösterilir.
- Library: the delete dialog can stop all servers using a file before enabling deletion.
- Kütüphane: silme iletişim kutusu, silmeyi etkinleştirmeden önce dosyayı kullanan tüm sunucuları durdurabilir.
- ONNX engine on NVIDIA (Windows): when the CUDA provider cannot be created or the model cannot be loaded on it (a missing or broken cuDNN, an unsupported GPU), the server no longer falls back to the CPU silently. harness-onnx exits with code 86 and Harness restarts the model once on DirectML, saying so in the server log.
- Header: at a 1280 px wide window the generation speed badge (t/s) was drawn under the Stop button. The hardware label now gives way first.
- Başlık: 1280 px genişliğindeki pencerede üretim hızı rozeti (t/s) Durdur düğmesinin altına çiziliyordu. Artık önce donanım etiketi daralıyor.
- Linux: drop-down lists were drawn as GTK's light grey buttons, ignoring the theme. They now take the theme's background and border with a matching arrow.
- Linux: açılır listeler temayı yok sayıp GTK'nin açık gri düğmesiyle çiziliyordu. Artık temanın arka planını ve kenarlığını, uyumlu bir okla kullanıyor.
- Linux: on a desktop without a tray host (stock GNOME without the AppIndicator extension) the tray icon was created but never shown, and closing the window could hide it with no way back. The tray now counts only while a StatusNotifierWatcher is on the session bus; without one, closing quits.
- Linux: tepsi sunucusu olmayan masaüstünde (AppIndicator eklentisi olmayan standart GNOME) tepsi simgesi oluşturuluyor ama hiç görünmüyordu ve pencereyi kapatmak onu geri getirilemez biçimde gizleyebiliyordu. Tepsi artık yalnız oturum veri yolunda bir StatusNotifierWatcher varken sayılıyor; yoksa kapatmak uygulamadan çıkar.
- Create > Auto Tools: the run counters and the queue filter buttons share one help (?) per group instead of one beside every button, so the counter grid and the filter row stay aligned.
- Oluştur > Otomatik Araçlar: çalıştırma sayaçları ve kuyruk filtre düğmeleri her düğmenin yanında ayrı bir yardım (?) yerine grup başına tek yardım kullanır; sayaç ızgarası ve filtre satırı hizalı kalır.
- Create tool generators use this instance's running gateway address, follow port changes, and offer gateway settings without sending requests while it is off.
- Oluştur araç üreticileri bu örneğin çalışan ağ geçidi adresini kullanır, port değişikliklerini izler ve ağ geçidi kapalıyken istek göndermeden ayarları açmayı sunar.
- audio.cpp: the standard release archives are built for the release runner's AVX-512 CPU, so the managed install crashed with SIGILL (exit 132, shown as "The audio.cpp server did not answer") on CPUs without AVX-512, such as AMD Zen 2/3 and most consumer Intel. The portable archives are now preferred on Linux and Windows; they choose the CPU code at run time. Reinstall audio.cpp to get the portable build.
- audio.cpp: standart sürüm arşivleri derleme makinesinin AVX-512 işlemcisine göre derlendiği için yönetilen kurulum AVX-512 olmayan işlemcilerde (AMD Zen 2/3, çoğu masaüstü Intel) SIGILL ile çöküyordu (çıkış 132, "The audio.cpp server did not answer"). Linux ve Windows'ta artık işlemci kodunu çalışırken seçen taşınabilir (portable) arşivler tercih ediliyor. Taşınabilir sürüm için audio.cpp'yi yeniden kurun.
- Model Hub: the details card of a repo that runs on another engine (ONNX embeddings such as all-MiniLM-L6-v2 and bge, Python, mistral.rs) no longer says "Not runnable: no GGUF file"; it shows the same verdict as the list.
- Model Hub: başka bir motorda çalışan deponun (all-MiniLM-L6-v2 ve bge gibi ONNX gömme modelleri, Python, mistral.rs) ayrıntı kartı artık "Çalışmaz: GGUF dosyası yok" demiyor; listedeki kararın aynısını gösteriyor.
- First-run wizard, purpose step: after "Install selected" the button read "Install selected (0)" and each model only said "Already installed or downloading", so it was not clear whether anything was still downloading. Each model row now shows its download (queued, percentage and size, verifying) and a check mark once it is downloaded; the button says "Downloading (n)" while models download and "Selected items installed" afterwards.
- OpenTelemetry: export failed on a system without a keychain (Linux without a Secret Service collection, a headless server) even with no headers configured, because the empty header list was still read from the keychain. The keychain is now touched only when headers were saved.
- OpenTelemetry: anahtarlığı olmayan sistemde (Secret Service koleksiyonu olmayan Linux, ekransız sunucu) başlık ayarlanmamış olsa da dışa aktarım başarısız oluyordu; boş başlık listesi yine anahtarlıktan okunuyordu. Anahtarlığa artık yalnız kayıtlı başlık varsa başvuruluyor.
- Model Hub: each card's help button (?) sits inside the card's top-right corner instead of beside it, so the grid columns stay aligned.
- Python engine: a model that fails to load stops its worker at once with the reason, instead of waiting for the 300 s load timeout. SpeechT5 and FastSpeech2 are no longer offered for text-to-speech (their pipeline downloads a vocoder at load, which the offline worker cannot), and root SentencePiece files such as `spm_char.model` are downloaded.
- Library: CrispASR and audio.cpp speech models show their real engine; audio.cpp processing models show their audio task.
- Kütüphane: CrispASR ve audio.cpp seslendirme modelleri gerçek motorunu, audio.cpp işleme modelleri ses görevini gösterir.
- Library: Image Generation includes Python diffusers text-to-image and image-to-image folders.
- Kütüphane: Görsel Üretimi, Python diffusers metinden görsele ve görselden görsele klasörlerini içerir.
- Hub: completed downloads of every model kind refresh the Library and Playground model lists, including speech models.
- Hub: her model türünün tamamlanan indirmesi, ses modelleri dahil Kütüphane ve Playground model listelerini yeniler.
- Library: opening a diffusers image model in Playground selects that model.
- Kütüphane: diffusers görsel modeli Playground’nda açıldığında o model seçilir.
- Model sizes switch to GB at 1000 MB in the Hub and Library, keeping locale decimal separators.
- Hub ve Kütüphane’de model boyutları 1000 MB’den itibaren GB olarak, dilin ondalık ayracı korunarak gösterilir.
- Dashboard and the Playground load settings showed llama.cpp settings (`-c`, `-ngl`, KV cache, flash attention) for models other engines run, and the ONNX accelerator could not be chosen anywhere. The form now follows the model's engine: ONNX, whisper.cpp, sd.cpp, python-hf, MLX and the other engines show their own settings from their catalogs (ONNX: Accelerator auto/cpu/directml/cuda, batch size, pooling...), the engine field names the engine that runs the model, and the llama.cpp web UI button is hidden for them.
- Python engine:
  - **Speech recognition:** audio at another rate than the model's (a 44.1 kHz WAV for wav2vec2 / Whisper) failed with "torchaudio is required to resample". The worker now resamples it with SciPy, which is in the locks.
  - **Task models served right away:** a downloaded task model (TTS, ASR, classifier...) gets its definition at once, so `/v1/audio/speech`, `/v1/audio/transcriptions` and `/v1/hf` serve it. Until now they answered 404 until the Playground Tasks tab was opened.
  - **Hub:** a speech repo made for its own package (Qwen3-TTS, VibeVoice streaming, Kokoro tuners...) is no longer listed as runnable on the Python engine. Its `config.json` model type is checked against those the locked transformers 5.19 loads for that task.
  - **Hub:** an empty result in a GGUF-only listing offers "Also show non-GGUF models".
- Python workers no longer inherit `TORCH_FORCE_NO_WEIGHTS_ONLY_LOAD` / `TORCH_FORCE_WEIGHTS_ONLY_LOAD` from the user's environment (the first turns off `torch.load`'s `weights_only` protection for PyTorch `.bin` weights); the worker also drops them itself before torch loads.
- ONNX engine: a dynamically quantized text model (transformers.js `model_quantized` of BERT / DistilBERT and similar, with `MatMulInteger`) no longer crashes the sidecar with an access violation on DirectML (seen on an NVIDIA GPU). It runs on the CPU, as quantized vision and speech models already did; the FP32 / FP16 file runs on the GPU.
- Model Hub and Library: ONNX repos no longer say "onnxruntime (not installed)" after ONNX Runtime was installed. Its builds live in their own folder, not in the runtime registry, so they were never counted.
- Speech to text with whisper.cpp: `verbose_json` answers (the Playground Speech tab uses them) no longer pay for an extra language detection pass that whisper-server runs only to report the language probability, which OpenAI's format does not have. On a CPU build this took a third of the request time (an 11 s clip: 54 s, now 36 s).
- Library: Python-engine folders (diffusers pipelines, transformers task, embedding and ranking models) are recorded with the python-hf engine, not llama.cpp; the library shows that engine, or that it needs installing. A diffusers text-to-image folder opens in the Playground Image tab instead of Tasks with no model, and is labeled "Image generation (Python, diffusers)"; other Python task models are labeled "Task model (Python)" instead of ONNX.
- MLX model folders are listed in the library with format MLX and their MLX engine, not as GGUF for llama.cpp. Deleting one removes the whole folder. MLX speech models (whisper, Qwen3-TTS, Kokoro conversions) are no longer taken for chat models.
- Playground > Speech: the transcript now appears right under the Transcribe button instead of below the recommended model list.
- Python engine: the model's settings (device, number type, threads from the catalog) now reach the worker; they were left out of its command line as "runtime unknown".
- The toast after a finished download ("Load and Chat" / "Open in Dashboard") no longer squeezes its text to one word per line; the buttons move to their own row instead.
- GGUF conversion: a repository that is already quantized with a method the converter can't read (AWQ, bitsandbytes, ...) is refused in the plan, before anything is downloaded, and the message names the base model to use instead. It used to download the whole checkpoint and fail at the end. GPTQ, FP8, compressed-tensors, MXFP4 and NVFP4 (ModelOpt) checkpoints still convert.
- An ONNX model on a server is shown by name in the Playground's model picker and in the sidebar's running servers: the picker shows the definition's alias, the sidebar the repo folder. Both used to show the file name of the export, such as `model_quantized.onnx`.

- Image edits with FLUX.1 Kontext, FLUX.2 and Qwen-Image-Edit now apply the instruction (full strength by default) instead of returning the picture almost unchanged; a repo's own vision projector is used and downloaded; Lightning fp8 exports the native engine cannot load are marked unsupported. HF ONNX: timm models use their own preprocessing and a GPU that returns invalid numbers falls back to the CPU. My Models shows Token Rejected for remote Harness links.
- FLUX.1 Kontext, FLUX.2 ve Qwen-Image-Edit ile görsel düzenleme artık talimatı uyguluyor (varsayılan tam güç), resmi neredeyse değiştirmeden geri vermiyor; deponun kendi görü projektörü kullanılıp indiriliyor; yerel motorun yükleyemediği Lightning fp8 dışa aktarımları desteklenmiyor olarak işaretleniyor. HF ONNX: timm modelleri kendi ön işlemesini kullanıyor, geçersiz sayı döndüren GPU'da CPU'ya geçiliyor. Kütüphanem, uzak Harness bağlantıları için Token Reddedildi durumunu gösteriyor.
- Image generation on Apple Silicon: FLUX.2 [klein] models in mflux's format draw and edit images through `/v1/images/generations`, `/v1/images/edits` and the Playground, with a memory check that picks or refuses the size, a time limit and stop on disconnect.
- Apple Silicon'da görsel üretme: mflux biçimindeki FLUX.2 [klein] modelleri `/v1/images/generations`, `/v1/images/edits` ve Playground üzerinden görsel çiziyor ve düzenliyor; boyutu seçen ya da reddeden bir bellek kontrolü, zaman sınırı ve bağlantı kopunca durdurma var.
- `harness serve` and the app no longer crash when Windows temporarily runs out of system resources while starting a model's log readers; they retry briefly and otherwise report an error.
- `harness serve` ve uygulama, bir modelin günlük okuyucuları başlatılırken Windows'un sistem kaynakları geçici olarak tükendiğinde artık çökmüyor; kısa süre yeniden deniyor, olmazsa hata bildiriyor.
- An interrupted `harness pull` now fails with a clear message and a non-zero exit code; the partial file is kept and the next pull resumes it.
- Kesilen bir `harness pull` artık açık bir mesajla ve sıfırdan farklı çıkış koduyla başarısız oluyor; yarım dosya korunuyor ve sonraki pull kaldığı yerden devam ediyor.
- My Models no longer lists the component files of a Diffusers pipeline folder (such as unet) as separate models; old automatic entries for them are cleaned up.
- Kütüphanem artık bir Diffusers klasörünün bileşen dosyalarını (unet gibi) ayrı model olarak listelemiyor; bunlar için eski otomatik kayıtlar temizleniyor.
- Hub: WhisperKit Core ML repos link to their whisper.cpp build, pyannote diarization pipelines explain that Harness diarizes with sherpa-onnx models, and speech GGUFs made for another engine show a precise refusal.
- Hub: WhisperKit Core ML depoları whisper.cpp sürümüne yönlendiriyor, pyannote diyarizasyon boru hatları Harness'in konuşmacı ayrımını sherpa-onnx modelleriyle yaptığını söylüyor, başka bir motor için hazırlanmış konuşma GGUF'ları açık bir ret nedeni gösteriyor.
- Process Audio lists the diarization models (Nemotron 3 Diarization, Sortformer) and shows speaker turns as a timeline and table, with Copy and Download JSON; a recording with no speech gives an empty result instead of an error.
- Ses İşleme, konuşmacı ayrımı modellerini (Nemotron 3 Diarization, Sortformer) listeliyor ve konuşmacı bölümlerini zaman çizelgesi ve tablo olarak, JSON Kopyala ve JSON İndir ile gösteriyor; konuşma olmayan bir kayıt hata yerine boş sonuç veriyor.
- At a high interface scale or in a small window, the servers table keeps names readable and its actions in view, the model picker's help button is no longer covered, and the Performance description and the endpoint list no longer cut off text.
- Yüksek arayüz ölçeğinde ya da küçük pencerede sunucu tablosu adları okunur, işlem düğmelerini görünür tutuyor. Model seçicinin yardım düğmesi artık örtülmüyor. Performans açıklaması ve uç nokta listesi artık metni kesmiyor.
- The first-open tour now appears on the first Dashboard visit right after setup (it used to wait for a later launch); download toasts no longer cover the setup wizard's buttons.
- İlk açılış turu artık kurulumdan hemen sonra Dashboard'a ilk girişte açılıyor (önceden sonraki açılışı bekliyordu); indirme bildirimleri kurulum sihirbazının düğmelerini artık kapatmıyor.
- At large interface sizes and small windows, dialogs scroll instead of being cut off, the command palette stays on screen, the model file list keeps its Download buttons reachable, and truncated server, runtime and MCP names show a tooltip.
- Büyük arayüz boyutlarında ve küçük pencerelerde diyaloglar kesilmek yerine kayıyor, komut paleti ekranda kalıyor, model dosya listesindeki İndir düğmelerine ulaşılabiliyor; kesilen sunucu, motor ve MCP adları ipucu gösteriyor.
- New speech recognition engine: transcribe.cpp (Parakeet, Canary, Cohere Transcribe, Qwen3-ASR, Whisper, Moonshine and more from handy-computer's GGUF exports), with a CUDA build on NVIDIA GPUs, in Playground Transcribe, the gateway audio API, `harness transcribe` and the Hub.
- Yeni konuşma tanıma motoru: transcribe.cpp (handy-computer GGUF dışa aktarımlarından Parakeet, Canary, Cohere Transcribe, Qwen3-ASR, Whisper, Moonshine ve diğerleri). NVIDIA GPU'larda CUDA sürümüyle; Playground Yazıya Dök, ağ geçidi ses API'si, `harness transcribe` ve Hub'da.
- Text to speech with llama.cpp: Qwen3-TTS GGUF models speak on `/v1/audio/speech` and in the Speech panel through the runtime's `llama-tts` (10 languages, the model's own voice), and the Hub shows them as runnable.
- llama.cpp ile metinden konuşma: Qwen3-TTS GGUF modelleri, çalışma zamanının `llama-tts` aracıyla `/v1/audio/speech` uç noktasında ve Konuşma panelinde konuşuyor (10 dil, modelin kendi sesi). Hub bunları çalışır olarak gösteriyor.

## [0.1.0] - 2026-10-12

First public release. Windows installer (NSIS) and signed, notarized macOS DMGs (Apple Silicon and Intel) on GitHub Releases; a Linux `.deb` package as a preview.

### Model discovery and library
- Model Hub: Hugging Face search with filters, model cards, quantization variants, and a "fits your hardware" indication.
- Resumable download queue with pause and resume, SHA-256 verification, split (multi-file) models, and an update badge for models you already have.
- Library: scan several model folders, import from Ollama and LM Studio, detect duplicates and missing files, and save model definitions (aliases, load settings, presets).
- Convert safetensors repositories to GGUF from the Hub (managed, temporary Python environment; quantization with `llama-quantize`).
- Optional engines (whisper.cpp, stable-diffusion.cpp, sherpa-onnx, ONNX Runtime, Lemonade) install with one click right where a model needs them (library card, Model Hub, Playground errors), and are listed with their install state under Settings > Runtime > Installed Runtime at both interface levels.
- Image, speech (Whisper), embedding, and reranker files that were already in a library folder (not downloaded through Model Hub) now show up in the Playground and the gateway without creating a definition first. Their library cards offer **Open in Playground**, also at the Simple level.
- Sign in to Hugging Face with OAuth or a pasted token; tokens are kept in the operating system keychain.

### Running models
- Dashboard with several simultaneous servers, saved launch profiles, automatic ports, and live telemetry (slots, throughput, KV cache, memory).
- Memory estimates before loading, automatic GPU layer count, automatic KV cache precision, and a fit check against free VRAM and shared memory.
- Clear diagnosis and recovery suggestions for failed loads (out of memory, missing libraries, port conflicts).
- Parameter catalog gated by the selected runtime build; Simple and Advanced interface levels.
- Multi-LoRA adapters with live scaling, chat template editor, RoPE/YaRN options, speculative decoding, and router mode.
- On Apple Silicon Macs with 16 GB of memory or less, the automatic context leaves room for macOS and other apps (a small cap on the KV cache and the free memory), and the fit labels say "memory" instead of "VRAM".
- Managed llama.cpp runtime downloads with integrity checks, background updates, and support for extra runtimes (forks, local folders). Backends: CUDA, ROCm/HIP, SYCL, Vulkan, CPU, and Metal.
- Other engines: whisper.cpp and sherpa-onnx (speech to text, voice activity detection, speaker separation, text to speech), stable-diffusion.cpp (image generation), ONNX Runtime (embeddings, reranking, ONNX chat), and optional Lemonade and mistral.rs.

### Playground
- Streaming chat with saved conversations, branching and regeneration from any message, model comparison, Markdown and Mermaid rendering.
- Image input for vision models, audio dictation, read-aloud, and image generation.
- Local document search (PDF, DOCX, text, code) with cited answers.
- Reversible context compaction for long conversations.
- Inline load settings, token usage display, and optional token probabilities (logprobs).
- A tool approval card that opens below the visible part of the chat scrolls into view and focuses its Approve button.

### Gateway and APIs
- Optional gateway with one endpoint for many models: OpenAI, Anthropic, and Ollama compatible APIs (chat, responses, embeddings, rerank, images, speech, model management).
- Models load on first use and unload when idle; admission control, request queueing, and tool-call recovery for models that emit calls as plain text.
- Authenticated Control API with OpenAPI schema and scoped tokens; Prometheus `/metrics` and optional OpenTelemetry export.
- Network access is off by default; enabling it requires tokens.

### Tools and MCP
- Built-in tools for files, shell, search, web, and weather, with an approval policy per tool.
- MCP client (stdio, HTTP, SSE; resources; OAuth) and Harness itself as an MCP server (`harness mcp`).
- Create your own sandboxed JavaScript tools with a local model; tool-call diagnosis for models that call tools poorly.

### Integrations
- Guided setup for coding agents and tools: Claude Code, Codex, OpenCode, Aider, Goose, Continue, Zed, VS Code, Open WebUI, Cline, JetBrains, and others, with diff preview, backups, and rollback.
- Launch targets that start a client already pointed at your local model.

### CLI
- Messages that point to the Control API now name the right place, Settings > Developer > Control API.
- `harness` command: `serve`, `search`, `pull`, `load`, `run`, `ps`, `launch`, `runtime`, `doctor`, and more, with `--json` output and a headless mode that needs no window.

### Observability and benchmarks
- Request analytics (latency percentiles, tokens, errors), searchable logs, and benchmark runs with comparison.
- Measured comparison against LM Studio, Ollama, and llama.cpp.

### Interface
- English and Turkish, light, dark, and system themes, high contrast, color-vision-friendly palette, interface scaling.
- System tray, launch at login, and background mode.
- On macOS the menu bar follows the interface language (Turkish or English).
- The Control API settings page no longer shows a stray help icon above the first setting.

### First run and reliability
- First-run wizard: the Chat purpose always finds a general instruction model that fits your hardware (for example Qwen3-4B-Instruct-2507 on a 4 GB card); coding, math, vision, embedding, and reasoning-distilled models are not offered for chat. "Add Command-Line Tool to PATH" is off by default.
- After the wizard the app opens the Model Hub when you have no model yet, otherwise the Playground, which offers **Start {model}** when no server is running.
- The Dashboard form shows what the automatic launch really uses (context, GPU layers, concurrent slots, KV cache types, flash attention), and `harness show` prints the same default load and memory estimate.
- On Apple Silicon Macs with 8 to 16 GB, the automatic launch tries a q8_0 KV cache with flash attention before lowering the context (Qwen3-4B on an 8 GB Mac: 12k context instead of 8k, in less memory).
- Image generation that does not fit in GPU memory says how much memory it needs and how much is free, instead of a bare HTTP 500.
- A regenerated answer is always listed after the original, also when messages are written within the same millisecond.
- Gateway: parallel requests to a one-slot server no longer fail now and then with `upstream_unreachable` (502).
- A tool call right after an MCP server crashed reconnects and is retried once.
- GGUF conversion size estimates count the token embedding and the wider fallback types `llama-quantize` uses (SmolLM2-135M Q4_K_M: 103.2 MB estimated, 105.5 MB converted).
- macOS: the sherpa-onnx one-click install works (its shared libraries are installed next to the binaries); Lemonade is described with its Metal backend, as the Ryzen AI NPU is Windows and Linux only.

### Known limitations
- The Windows installer is unsigned; SmartScreen may show a warning (**More info > Run anyway**).
- The Linux packages are a preview, tested on one Ubuntu 24.04 machine so far.

[0.1.2]: https://github.com/struklabs/harness-studio/releases/tag/v0.1.2
[0.1.1]: https://github.com/struklabs/harness-studio/releases/tag/v0.1.1
[0.1.0]: https://github.com/struklabs/harness-studio/releases/tag/v0.1.0
