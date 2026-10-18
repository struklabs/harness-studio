# Arayüz Düzeyi (Basit / Gelişmiş) ve Bilgi Hiyerarşisi

[English](../en/UI_LEVELS.md)

## 1. Arka Plan

- **Özellik yüzeyi geniş, katman yok.** 10 ana ekran; yalnız Dashboard'da 9, Ayarlar'da 22, Kütüphane'de 11 alt sayfa. Model seçip sohbet etmek isteyen kullanıcı da `-np`, `--fit`, `--cpu-moe`, KV önbellek kuantı, router modu, model motoru seçimi, gateway uç noktası tablosu ve MCP sunucularıyla aynı düzeyde karşılaşıyor.
- **Metin ile kontrol aynı görsel ağırlıkta.** Dashboard `Field` etiketi 12 px `font-medium`, altındaki açıklama 12 px `muted-foreground`: aradaki tek fark ağırlık ve renk tonu. `SettingRow` açıklaması her zaman satır içinde. 52 `SettingRow`, 13 `Field` satır içi açıklama taşıyor; bunlara ek olarak bileşenlere elle yazılmış 233 `text-xs text-muted-foreground` paragraf var (en çok Bağlantılar, Araç, Analitik, Ayarlar > Ağ Geçidi).
- **Teknik meta veri vurgu renginde.** "Backend'e bağlı", "Modele bağlı", "Model motorunda var" rozetleri bakır (primary) çerçeveliydi; bu, tasarım sisteminin "tek vurgu rengi" kuralına aykırıydı ve gözü asıl kontrolden alıyordu. Bayrak adları (`-c`, `--fit`) her etiketin yanında görünüyordu.
- **Altyapı hazırdı.** Alan yardımı ortak (`InfoTip`, `HelpPopover`, `FieldHelp`, `describeFields`), sayfa hiyerarşisi bildirimsel (`screenSections.ts`, `settings/sections.ts`, `Sidebar.svelte` grupları), ayarlar tek dosyada (`AppSettings`, serde varsayılanlı). Bu yüzden iki ayrı arayüz yerine tek yerden yönetilen bir görünürlük katmanı mümkündü.

## 2. Karar

İki ayrı görünüm yerine **tek arayüz, iki bilgi düzeyi** seçildi (kademeli açılım, progressive disclosure):

- İki ayrı ekran ağacı yazılmaz. Her ekran tek bileşendir. Yalnız bazı sayfa, bölüm ve alanlar **Gelişmiş** olarak işaretlenir.
- Düzey yalnız **görünürlüğü** değiştirir. Hiçbir değeri değiştirmez, sıfırlamaz ya da kaydetmez. Gizli bir ayar, Basit düzeyde de aynen uygulanır. Böylece geçişte ayar kaybı olmaz ve iki düzey arasında tutarsızlık oluşmaz.
- Metin yoğunluğu ayrı bir moda bırakılmaz, **iki düzeyde de** aynı katman kurallarıyla çözülür (bölüm 4).

Neden iki ayrı görünüm değil: Bakım yükü iki katına çıkardı. Her yeni özellik iki yerde yazılırdı ve davranışlar zamanla ayrışırdı. Kullanıcı düzey değiştirince başka bir uygulamaya geçmiş gibi olurdu. Kademeli açılımda öğe aynı yerinde kalır, Gelişmiş düzeyde yalnız fazlası görünür.

## 3. Arayüz Düzeyi

### Kurallar

1. **Tek kaynak:** `AppSettings.ui_level` (`"simple"` | `"advanced"`, yoksa `null`). `null` = **Gelişmiş**: mevcut kullanıcılar güncellemeden sonra hiçbir şeyi kaybetmez. Yeni kurulumda sihirbaz sorar; önerilen seçenek **Basit**'tir.
2. **Gizlenen yer çıkmaz sokak olmaz.** Komut paleti, derin bağlantılar (`#settings/...`), hata iletisindeki "Ayarlar > X" bağlantıları ve ekranlar arası bağlantılar Gelişmiş sayfaları Basit düzeyde de açar. Bu durumda sayfanın üstünde ince bir şerit görünür: "Bu sayfa Gelişmiş düzeyde · Gelişmiş Düzeye Geç". Sayfa normal çalışır.
3. **Görünmeyen etkin ayar söylenir.** Basit düzeyde gizli kalan bir parametre varsayılandan farklıysa (ör. Dashboard'da KV önbellek tipi), ilgili yerde tek satır gösterilir: "2 gelişmiş ayar etkin · Göster". Kullanıcı neden farklı davrandığını bulabilir.
4. **Tek tıkla geçiş:** ana gezinmenin altında (Ayarlar'ın üstünde) düzey anahtarı, Ayarlar > Görünüm ve Başlangıç'ta "Arayüz düzeyi" satırı, komut paletinde "Gelişmiş Düzeye Geç" / "Basit Düzeye Geç". Geçiş anında uygulanır ve hemen kaydedilir (tema ve dil gibi).
5. **Bildirimsel işaret:** Düzey bilgisi verinin yanında durur.
   - Sayfa ve gezinme öğeleri: `SectionPage.level`, `Sidebar` `NavItem.level`, Ayarlar alt sayfaları `level`.
   - Bölüm ve alanlar: `<AdvancedOnly>` sarmalayıcısı, `SettingRow` / `SwitchRow` / `SettingGroup` / `Field` için `advanced` prop'u.
   - Varsayılan: işaretsiz öğe iki düzeyde de görünür. Yeni özellik eklerken tek karar verilir: "Bu, ilk kez yerel yapay zekâ kullanan biri için gerekli mi?" Gerekli değilse `advanced` işaretlenir.
6. **Düzey değiştirmek gezinmeyi değiştirmez.** Gelişmiş bir sayfadayken Basit'e geçilirse kullanıcı aynı sayfada kalır, sayfanın üstünde 2. kuraldaki şerit belirir. Menüden başka bir sayfaya geçince gizli sayfa listede görünmez. Kayıtlı "son sayfa" tercihi silinmez. Ekran açılırken son sayfa Basit düzeyde gizliyse ilk görünür sayfa açılır.

### Basit Düzey Haritası

| Ekran | Basit düzeyde görünür | Yalnız Gelişmiş |
|---|---|---|
| Ana gezinme | Dashboard, Model Bul, Kütüphanem, Sohbet ve Medya, Uygulama Bağla; altta Ayarlar; yalnızca varsa çalışan sunucular | Sunucular, Araç (Oluştur), Performans, Analitik |
| Dashboard | Model seçme, model bulma, sohbet ve uygulama bağlama kısayollarıyla Özet; Model Seç (model seçici, sığma göstergesi); Sohbet Uzunluğu (yalnız bağlam boyutu) | Ağ, GPU, Örnekleme, Şablon ve Reasoning, Araçlar, Diğer Parametreler; Router modu, model motoru seçici, bayrak sayısı satırı, eşzamanlı istek, KV önbellek ve tüm "Bellek ve KV Cache" bölümü; llama.cpp Web Arayüzü ve "Sunucu Olarak Kaydet" düğmeleri |
| Dashboard sağ panel | Telemetri (bellek çubukları); Sunucu Logları **kapalı katlanmış** ("Logları Göster"), hata olunca kendiliğinden açılır | Log süzgeçleri ve araç çubuğu açık |
| Playground | Sohbet, konuşma ve görsel sekmeleri; sesi yazıya çevirme ve ses oluşturma; Gelişmiş Ayarlar altında isteğe bağlı konuşma ve görsel kontrolleri; giriş satırında Web ve Araçlar anahtarı, dikte (başlat/durdur, süre ve iptal); sağ panel: Model ve Preset, Sistem İstemi, Araçlar (izinler) | Embedding, Rerank ve Pipeline sekmeleri; ses işleme ve etkin olmayan karşılaştırma kontrolü; Örnekleme, Çıktı Sınırları, Geçmiş ve Compaction, RAG ayarları; giriş satırındaki token sayacı |
| Kütüphanem | Modeller grubu (Tümü, Sohbet, Görsel Anlama, Konuşma, Görsel Üretimi, Metinden Sese), İçe Aktarma | Embedding ve Reranker kategorileri, yinelenen dosya bulucu, Yardımcı Dosyalar (Draft, Adapter, Projektör), Model Tanımları |
| Model Hub | Arama, dört yalın görev kısayolu (Sohbet, Sesi Yazıya Çevir, Ses Oluştur, Görsel Oluştur), tüm görevleri açma, model kartları, indirme | Ayrıntılı süzgeç paneli, kuant listesindeki teknik ayrıntılar |
| Bağlantılar | İlk ziyarette Otomatik Yapılandırma, Adım Adım Kurulum, Harness Link, Uç Nokta Listesi | Ollama Uç Noktası, Port Notları, SDK ve Kod Örnekleri, Harness MCP, MCP Sunucuları grubu, Test grubu |
| Ayarlar | Kurulu Model Motoru, Dizinler, Hugging Face Hesabı, Görünüm ve Başlangıç, Güncellemeler, Gizlilik | Registry, Ollama, Ağ, Güvenlik, Ağ Geçidi, Araçlar, Geliştirici, OpenTelemetry, Tanılama |

Basit düzeyde ana hedefler tek tık uzaktadır. Dashboard > Özet üzerinden model seçiciye bir tıkla ulaşılır, sonraki tıkla model seçilir; Başlat düğmesi sabit kalır. Model bulma, sohbet ve uygulama bağlama doğrudan kısayollardır. İndirme, kurulum ve uygulama yapılandırması ek seçimler gerektirebilir; 1–2 tık hedefi akışa ulaşmak içindir.

Gelişmiş düzey tüm hedefleri ve kontrolleri mevcut görev gruplarında korur. Basit düzeyde teknik etiket yerine gerektiğinde `SectionPage.simpleLabel` kullanılır. Bölüm menüleri, arama ve sohbet ayarları tek tıkla Gelişmiş Ayarlar erişimi sunar. İsteğe bağlı konuşma ve görsel kontrolleri genel düzey değiştirilmeden yerel olarak da açılabilir. Geçersiz görsel boyutu ilgili kontrolleri kendiliğinden açar. Zorunlu girdiler, hatalar ve görsel düzenleme ekleri görünür kalır.

Düzey geçişi model ve form değerlerini korur. Etkin bir Gelişmiş Playground sekmesi Basit düzeye geçildiğinde sekme çubuğunda kalır; etkin karşılaştırma kapatılabilir. Yeni Basit ziyaretlerde bu ek kontroller gösterilmez. Dar panellerde gezinme etiketleri kesilmek yerine alt satıra geçer.

## 4. Bilgi Hiyerarşisi (İki Düzeyde de Geçerli)

Metin silinmez. Her metin bir katmana atanır ve o katmanın görünümünü alır.

| Katman | Ne | Görünüm |
|---|---|---|
| K1 Sayfa başlığı | Seçili alt sayfanın adı | `text-base font-semibold`; sayfa açıklaması varsa tek satır `text-xs muted`, uzunsa başlığın yanındaki (?) içinde |
| K2 Bölüm başlığı | Grup adı | `text-sm font-semibold` |
| K3 Alan etiketi | Kontrolün adı | `text-sm font-medium text-foreground` (Dashboard `Field` 12 px'ten 13 px'e çıkar; etiket ile açıklama arasındaki fark büyür) |
| K4 Durum ve sonuç | Uyarı, hata, "VRAM'e sığıyor", kaydedilmemiş değişiklik, öneri ("OpenCode için en az 48k") | Her zaman satır içinde; durum rengi ve simgeyle |
| K5 Açıklama | Alanın ne işe yaradığı | **Basit:** satır içinde (`text-xs muted`), çünkü az alan kalır ve yeni kullanıcının buna ihtiyacı vardır. **Gelişmiş:** etiketin yanındaki (?) içinde. Erişilebilir açıklama (`aria-describedby`) iki düzeyde de korunur. |
| K6 Teknik ayrıntı | Bayrak adı (`-c`, `--fit`), kaynak rozetleri ("Modele bağlı", "Backend'e bağlı"), bayrak sayısı | Yalnız Gelişmiş düzeyde; nötr renkte (`text-muted-foreground`, çerçevesiz ya da `border-border`). Primary renk kullanılmaz. |

Bileşen API'si:

- `hint` = K5 açıklama: düzeye göre satır içinde ya da (?) içinde.
- `note` = K4 bağlamsal bilgi: her zaman satır içinde. Yalnız karar değiştiren ya da sonucu anlatan metinde kullanılır.
- `help` = uzun açıklama: her zaman (?) içinde.
- `flag` = K6: yalnız Gelişmiş'te, nötr mono.
- Bileşenlere elle yazılmış açıklama paragrafları (`<p class="text-xs text-muted-foreground">`) mümkün olduğunca bu üç prop'a ya da `PageIntro` bileşenine taşınır. Böylece kural tek yerden uygulanır.

## 5. Doğrulama

`npm run check`, `npm run check:locales` ve yalnızca ilgili `node --test scripts/test-ui-levels.mjs` regresyon paketini çalıştırın. Paket tam Gelişmiş menülerini, azaltılmış Basit menülerini, gruptan gelen görünürlüğü, kayıtlı sayfa geri dönüşünü ve derin bağlantı hedeflerinin korunmasını denetler. Masaüstü arka ucu gerektirmez.

Etkileşimli incelemede iki dili ve düzeyi, dar panelleri, model seçme ve başlatmayı, Hub arama ve indirmeyi, konuşma ve görsel işlemlerini, uygulama bağlamayı, düzenlenmiş ayarlarla düzey geçişini ve Gelişmiş derin bağlantıları denetleyin. Geçersiz boyutla görsel seçeneklerini açıp doğrulamanın görünür kaldığını kontrol edin. Ön izleme rotalarını, taklit verileri, üretilen ekran görüntülerini ve `output/` dosyalarını commit'e eklemeyin. Bu otomatik kontroller masaüstü akışlarının tamamının doğrulandığı anlamına gelmez.
