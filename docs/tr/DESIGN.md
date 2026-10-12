# Harness Studio — Tasarım Sistemi

[English](../en/DESIGN.md)

Arayüz **shadcn-svelte + Tailwind v4** ile yazılır. Bileşenler `src/lib/components/ui/` altında projeye aittir; tema `src/app.css` içindedir.

## Referanslar

| Kaynak | Aldığımız |
|---|---|
| Raycast | Kompaktlık, klavye önceliği (⌘K), sade koyu mod, düşük görsel gürültü |
| Linear | Boşluk disiplini, tipografi, hizalama, hover/seçili durum kalitesi |
| OrbStack / Docker Desktop | Sol sidebar'lı "control center"; çalışan sunucular canlı liste olarak |
| Grafana / Datadog | Metrik hiyerarşisi: bir birincil değer büyük, diğerleri sakin; okunur grafikler |
| IBM Carbon / Atlassian | Form, ayarlar ve tablo düzeni; erişilebilirlik |

## Palet — "Graphite & Copper"

Grafit yüzeyler, tek vurgu rengi bakır (neon değil). Aydınlık mod soğuk kâğıt gri; **saf beyaz yok**.

| Token | Koyu | Aydınlık | Kullanım |
|---|---|---|---|
| `background` | `#0f1115` | `#eceef1` | Sayfa |
| `card` | `#15181d` | `#f6f7f9` | Kart/panel |
| `muted` | `#1b1f25` | `#e3e6ea` | Pasif yüzey, input arka planı |
| `border` | `#252a31` | `#d3d8de` | Tüm çizgiler |
| `primary` | `#e27c3a` | `#b95a1c` | **Tek** vurgu: birincil eylem, seçili öğe, odak |
| `success / warning / destructive / info` | yeşil / sarı / kırmızı / mavi | | Yalnızca durum anlamı taşır |

Kurallar:
- Renk yalnızca anlam taşır. Dekoratif gradient, neon parlama, renkli kart arka planı yok.
- Ekranda en fazla **bir** `primary` dolu buton. Diğerleri `outline`, `secondary` veya `ghost`.
- Hard-code renk (`#3b82f6`, `rgba(...)`) yazılmaz; her zaman token (`bg-card`, `text-muted-foreground`, `border-border`, `bg-primary/10` …).

## Tipografi

- **Geist** (arayüz), **IBM Plex Sans** (sayılar: port, boyut, sürüm, token sayısı, hız, süre, tarih), **Geist Mono** (kod, komut, bayrak, yol, URL, log, JSON, kimlik). Yerel paketler (Plex Sans yalnız Latin + Latin Genişletilmiş, değişken ağırlık), internet gerekmez.
- Taban 13 px (kökte; `rem` 13 px'tir). Yazı ölçeği `src/app.css` `@theme` içinde piksel olarak sabit: `text-xs` 12, `text-sm` 13, `text-base` 14, `text-lg` 16, `text-xl` 18, `text-2xl` 22, `text-3xl` 28. **12 px'ten küçük yazı yok**; `text-[11px]` gibi keyfi boyut yazılmaz.
- Hiyerarşi boyut ve ağırlıkla kurulur: sayfa başlığı başlık çubuğunda (sayfada tekrar edilmez); bölüm başlığı `text-sm font-semibold text-foreground`; liste grubu etiketi `text-xs font-medium text-muted-foreground`; ikincil bilgi `text-xs text-muted-foreground`.
- BÜYÜK HARF ve `tracking-wide` etiket yok. Her yerde cümle düzeni (sentence case), TR ve EN.
- Sayılar: `font-num` (IBM Plex Sans + `tnum`, hizalı rakamlar; token `--font-num`, `src/app.css`). Geist Mono'nun noktalı sıfırı 12 px'te "8080"i "8888" gibi okutuyordu. `font-mono` yalnız kopyalanan ya da kod olan metinde kalır: kod blokları, komutlar ve bayraklar (`-ngl`), yollar, URL'ler (port içerse de), loglar, JSON, takma ad / model / kuantizasyon adları, kimlikler. Sayı ile metni karışık satırlar ("3 sunucu", "12,4 t/s") `font-num`.

## Yerleşim ve yoğunluk

- 4 px ızgara. Panel iç boşluğu `p-4`, kart içi `p-3`, form satırları arası `gap-3`.
- Kontrol yüksekliği 28–32 px (`h-7`/`h-8`). Radius `rounded-md` (6 px), kartlar `rounded-lg`.
- Minimum pencere 900×600: hiçbir ekranda yatay taşma olmamalı (grid'lerde `minmax(0,1fr)`, `min-w-0`, `truncate`).
- Kart içinde kart yok. Bir panelin içindeki bölümler çizgiyle (`border-b`) ayrılır; kutu yalnız uyarı/hata ya da tıklanabilir öğe içindir. Sol kenar vurgu şeridi (`border-l-4 border-l-primary`) kullanılmaz.
- Rozet (`Badge`) yalnız durum için: tür, mimari, kuant, bağlam gibi olgular tek satır metin olarak (`LLM · 8B · Q4_K_M · 4,9 GB`) yazılır; ikincil olgular "Ayrıntılar" altına.
- Açıklama paragrafları formun altına yığılmaz; gerekiyorsa tek satır `text-xs text-muted-foreground` ya da `Tooltip`.
- "AI slop" kalıpları kullanılmaz: 12 px+ köşe ve halkalı (`ring`) kartlar; eşit KPI kartı ızgarası (yerine tek düz ölçüm şeridi); sohbette yuvarlak konuşma balonu (yerine düz transkript satırı); yatay kart şeritleri (carousel) üst üste; `Sparkles`/sihirli değnek simgeleri; simge için ayrı renkli kutu.
- Boş durum (empty state) = ne yapılacağını söyleyen tek cümle + tek eylem butonu (`ui/empty`). Sunucu çalışmıyorken ölçüm ekranları `0.0`, `–`, "Kullanılamıyor" dolu kartlar ya da boş eksenler göstermez; `common/ServerOffState` kullanılır.

## Bileşen eşlemesi

| İhtiyaç | Kullan |
|---|---|
| Buton | `ui/button` (`default` = birincil, `outline`, `secondary`, `ghost`, `destructive`; boyut `sm`/`default`/`icon-sm`) |
| Metin girişi | `ui/input`, `ui/textarea`, `ui/input-group`, etiket için `ui/label` / `ui/field` |
| Seçim | `ui/select`; 2–5 seçenek için `ui/toggle-group` (segmented) |
| Aç/kapa | `ui/switch` (ayar), `ui/checkbox` (liste seçimi) |
| Sayı aralığı | `ui/slider` + yanında mono değer |
| Diyalog | `ui/dialog` (odak tuzağı, Esc hazır); yan panel için `ui/sheet` |
| Sekmeler | `ui/tabs` |
| İpucu | `ui/tooltip` (eski `common/Tooltip`, `HelpTooltip` kaldırılacak) |
| Bildirim | `svelte-sonner` `toast()` (Toaster kabukta hazır) |
| Tablo | `ui/table` |
| Durum etiketi | `ui/badge` (`outline` + durum rengi metni) |
| Yükleniyor | `ui/skeleton`, `ui/progress` |
| Kısayol | `ui/kbd` |

## Alan yardımı

- `common/InfoTip`: kısa, tarafsız bilgi için **i** simgesi; hover ve klavye odağında tooltip. `text` ve gerektiğinde alanı tanımlayan `label` verilir.
- `common/HelpPopover`: kullanım adımları, birimler, sınırlar ve etkiler için **?** simgesi; tıklama/Enter açar, Escape kapatıp odağı döndürür. İsteğe bağlı `href` belge bağlantısıdır.
- `common/FieldHelp`: her zaman gerekli kısa açıklamada `text` ile bağlamsal metin; çocuk kontrol verildiğinde kontrolün yanında yardım popover'ı ve kalıcı erişilebilir açıklama oluşturur. `id` isteğe bağlıdır.
- Dashboard `Field` ve Settings `SettingRow` mevcut `hint` açıklamalarını bu ortak mekanizmayla bağlar; yeterli açıklamayı ikinci kez yazmayın. `ui/field` içindeki `Field.Description` da otomatik bağlanır. Özel düzenlerde `aria-describedby` ile mevcut açıklamanın kimliğini kullanın.
- Açıklamalar TR/EN birlikte yazılır; birim, varsayılan, aralık ve etkiler gerçek uygulama davranışını anlatır. Hata açıklamalarının kimlikleri korunur. Yardım butonları odaklanabilir, dokunma hedefi 28 px; portal katmanları yerleşimi kaydırmaz ve tema token'larını kullanır.
- `node scripts/check-field-help.mjs --strict` kapsamdaki düzenlenebilir kontrol noktalarını tarar; `--baseline <commit>` karşılaştırma, `--json` envanter verir. Statik referans kontrolü, metin doğruluğu ve canlı erişilebilirlik incelemesinin yerine geçmez. Döngülerde bir kaynak kontrolü bir kez sayılır.

## Arayüz düzeyi ve metin katmanları

- Tek kaynak `AppSettings.ui_level`; boş değer mevcut kullanıcılar için Gelişmiş'tir. Yeni kurulumda amaç adımı Basit'i önerir. Geçiş anında uygulanır ve en güncel ayarlar üzerine kaydedilir; başka ayarları veya açık sayfayı değiştirmez.
- Sayfa, gezinme öğesi ve grup verisine `level: 'advanced'` eklenir. Menüler `visibleGroups` / `isVisible` ile süzülür; boş gruplar düşer. Komut paleti ve derin bağlantılar tüm hedefleri açar. Basit'te açılan Gelişmiş sayfa `AdvancedPageNotice` gösterir. Açılışta gizli son sayfa yerine ilk görünür sayfa seçilir, kayıtlı tercih korunur.
- Sayfa içindeki bölüm için `<AdvancedOnly>`, `SettingRow`, iki `SwitchRow`, `SettingGroup` ve Dashboard `Field` için `advanced` prop'u kullanılır. Gizlemek değeri değiştirmez; görünmeyen ayar uygulanmaya devam eder.
- `hint` açıklamadır: Basit'te satır içinde, Gelişmiş'te etiket yanındaki (?) içinde. `help` her zaman (?) içindedir; ikisi birlikteyse önce hint, sonra help gelir. `note` karar veya sonuç bildiren bağlamsal metindir, her zaman satır içinde kalır. Erişilebilir açıklama iki düzeyde de korunur.
- `flag` teknik bayrak adıdır: yalnız Gelişmiş'te, nötr mono metin. Kaynak rozetleri de yalnız Gelişmiş'te gösterilir; primary kullanılmaz. Alan etiketi `text-sm font-medium text-foreground`, açıklama ve note en az `text-xs` olur.
- Yeni özellikte karar: “Bu, ilk kez yerel yapay zekâ kullanan biri için gerekli mi?” Gerekli değilse sayfa için `level`, bölüm veya alan için `advanced` işaretlenir. Metinde açıklama `hint`, uzun yardım `help`, sonucu veya öneriyi anlatan bilgi `note` olur. Tam model ve sınıflandırma [UI_LEVELS.md](UI_LEVELS.md) içindedir.

## Etkileşim

- Her eylemin görünür geri bildirimi olur: yükleniyor durumu, başarıda toast, hatada ne olduğunu ve nasıl düzeleceğini söyleyen mesaj. Hata asla sadece `console.error`'a gitmez.
- Odak her zaman görünür (`focus-visible` halkası). Tüm işlevler klavyeyle yapılabilir.
- Animasyon kısa (150–200 ms) ve yalnızca durum değişimini anlatır; `prefers-reduced-motion` desteklenir.
- Tüm metinler `t()` ile; tr ve en birlikte eklenir (`npm run check:locales`).

## Metin dili

- Kullanıcının kontrol ettiği şeyin adıyla konuş: "Sunucuyu Başlat", "Modeli indir".
- Buton ne yapacağını söyler; aynı eylem akış boyunca aynı adı taşır ("Kaydet" → "Kaydedildi").
- Hata mesajı özür dilemez, belirsiz değildir: ne oldu + ne yapmalı.

## Sayfa ve menü hiyerarşisi

Bağlantılar ve Ayarlar aynı `SectionSidebar` / `ResizablePane` düzenini kullanır: bölüm başlıkları altında alt sayfalar, seçili sayfa işareti, klavye odağı, dar pencerede ikon şeridi ve açılır panel. Bağlantılar son alt sayfayı hatırlar; ziyaret edilen bileşenler sayfa içinde bağlı kalır. Otomatik yapılandırma araç entegrasyonlarının gerçek listesidir; ayrı bir boş “Araç Entegrasyonları” sayfası yoktur. MCP istemci durumu ve canlı araç izinleri mevcut bileşendeki ilişkili işler olduğundan aynı alt sayfadadır.

Ekran bazlı uygulama tamamlandı: Dashboard (9), Playground sağ paneli (7), Kütüphane (11), Performans (6), Analitik (5) ve Ayarlar (22 alt sayfa). Son alt sayfa ekran bazında hatırlanır; URL hash/sorgusu, komut paleti ve ekranlar arası bağlantılar ilgili alt sayfayı seçer. Playground menüsü sağ panelin üstündeki gruplu seçicidir; sohbet yüzeyi korunur. Model Hub ve Sunucular mevcut sade düzeninde kalır; aşağıdaki iki öneri henüz uygulanmaz.

| Ekran / mevcut yoğunluk | Önerilen başlık → alt sayfa hiyerarşisi |
|---|---|
| **Tamamlandı** — Dashboard: model, sunucu formu ve gelişmiş parametreler aynı kontrol panelinde | Sunucu → Özet, Model ve Model Motoru, Ağ; Çıkarım → Bağlam ve bellek, GPU, Örnekleme; Gelişmiş → Şablon ve reasoning, Araçlar, Diğer parametreler. Başlat/durdur ortak alt çubukta kalır. |
| Model Hub: tek keşif listesi, çok sayıda filtre ve depo/dosya ayrıntısı | Keşfet → Tüm modeller, Sohbet, Görsel, Embedding ve reranker, Konuşma; İndirmeler → Aktif, Tamamlanan. Filtreler listede kalır; seçilen depo → Genel bilgi, Dosyalar, Donanıma uygunluk. Mevcut ekran uzun form değil; önceliği düşük. |
| **Tamamlandı** — Kütüphane: model türleri yatay sekmelerde, tanım ve yerel dosya işlemleri birlikte | Modeller → Tümü, Sohbet, Görsel, Embedding, Reranker, Konuşma; Yardımcı dosyalar → Draft, Adapter, Projektör; Yönetim → İçe aktarma, Model tanımları. Seçili model → Genel bilgi, Dosyalar, Çalıştırma tanımı. |
| Sunucular: şu an sade liste; genişleyen sunucu kartları ileride yoğunlaşabilir | Sunucular → Tümü, Çalışan, Durdurulmuş; Seçili sunucu → Durum, Yapılandırma, Günlükler. Listeye hemen panel eklemek yerine sunucu ayrıntısı büyüyünce uygulanır. |
| **Tamamlandı** — Playground sağ panel: model/preset, sistem istemi, örnekleme, araçlar ve diğer alanlar tek kaydırma alanında | Oturum → Model ve preset, Sistem istemi; Üretim → Örnekleme, Çıktı sınırları; Bağlam → Geçmiş ve compaction, RAG; Araçlar → Seçim ve izinler. Sohbet/embedding/rerank/transcription çalışma yüzeyleri ayrı kalır. |
| **Tamamlandı** — Performans: Canlı ve Benchmark sekmeleri; grafikler ve ölçüm ayarları uzun içerik | Canlı → Özet, Üretim hızı ve gecikme, Bellek ve GPU; Benchmark → Yeni ölçüm, Sonuçlar, Karşılaştırma. Ölçüm işi gezinme sırasında korunur. |
| **Tamamlandı** — Analitik: Genel bakış ve İstekler sekmeleri; saklama ayarları ayrıca açılır | Analitik → Genel bakış, Model ve istemci kırılımı; İstekler → Günlük, Seçili istek ayrıntısı; Saklama → Kayıt ve saklama politikası. Ortak tarih filtresi korunur. |
| **Tamamlandı** — Ayarlar: bölüm yan paneli mevcut, bazı alt sayfalar hâlâ birden çok uzun grup içeriyor | Model motoru → Kurulu runtime, Registry, Ollama; Depolama → Dizinler, Hugging Face hesabı; Ağ → Dinleme, Erişim; Güvenlik → API anahtarları, İzinler; Ağ geçidi → Uç nokta, Yönlendirme, Responses; Araçlar → Yerleşik araçlar, Kurulu araçlar; Geliştirici → Control API, CLI; Tanılama → Sağlık, Raporlar. Görünüm, Güncellemeler, Gizlilik ve OTel kısa kaldıkları sürece tek sayfa olur. Ortak taslak/kaydet çubuğu korunur. |

Her geçişte mevcut derin bağlantılar, devam eden işlemler, TR/EN etiketler ve dar pencere davranışı birlikte doğrulanır; ekranlar tek tek taşınır.
