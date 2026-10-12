# Bağlam sıkıştırma: araştırma ve ölçümler

[English](../en/COMPACTION.md)

Playground'daki bağlam sıkıştırmanın (context compaction) dayandığı araştırma ve seçilen yöntemin ölçümleri. Tasarım ve şema: [ARCHITECTURE.md §9.1](ARCHITECTURE.md#91-bağlam-sıkıştırma-harness_corecompaction). Kaynaklar Ekim 2026'da okundu. **[doğrulanamadı]** işaretli bilgiler resmi belge ya da kaynak kodunda bulunamadı.

## 1. Başka araçlar ne yapıyor

| Araç | Tetik | Ne tutuluyor | Not |
|---|---|---|---|
| Claude Code | Otomatik: pencere dolmadan önce. 1M modelde ~967K (≈33K pay); `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` eşiği yalnız düşürebilir. Elle: `/compact [talimat]` | Sistem istemi, CLAUDE.md, son düzenlenen 5 dosya yeniden yüklenir; gerisi yapılandırılmış özet | Özet talimatı `/compact <talimat>` ya da CLAUDE.md "Compact instructions". `/rewind` ile "buraya kadar özetle" [1][2][3] |
| Anthropic API | `clear_tool_uses_20250919`: varsayılan 100K girdi token'ında eski araç sonuçlarını siler, son 3'ü tutar, `clear_at_least` önbelleği kırmaya değsin diye. `compact_20260112`: 150K'da (en az 50K) özet | Son araç kullanımları; özet `compaction` bloğu | Araç sonucu temizlemek "en hafif dokunuş" [4][5][6] |
| OpenAI Codex CLI | Bağlamın %90'ı (`model_auto_compact_token_limit` en çok %90'a sıkıştırılır) | Son kullanıcı mesajları 20K token'a kadar aynen | "CONTEXT CHECKPOINT COMPACTION… handoff summary": ilerleme, kararlar, kısıtlar, sonraki adımlar [7][8] |
| OpenAI Responses API | `/responses/compact` ya da `compact_threshold` | Opak, şifreli `compaction` öğesi | Varsayılan eşik belgelenmemiş [9] |
| Claude.ai | Sınıra yaklaşınca önceki mesajları özetliyor (kod çalıştırma açıksa) | Tam geçmiş saklanıyor | [10] |
| ChatGPT | **[doğrulanamadı]** eski mesajlar sessizce düşüyor, sonra "maksimum uzunluk" | — | [11] |
| Cursor | Sınırda otomatik, `/summarize` elle (1.6) | — | Özeti daha küçük modelle yazdığı söyleniyor **[doğrulanamadı]** [12] |
| OpenCode | Girdi sınırı − `reserved` (10K) ≈ %96–99 | `prune`: son 40K token araç çıktısı korunur, eskiler "[Old tool result content cleared]" | Özet şablonu: Hedef, Talimatlar, Bulgular, Yapılanlar, Dosyalar [13][14] |
| Open WebUI | Varsayılan kapalı; 80 000 token eşiği | Mesajların %40'ı tutulur | Özet modeli sohbet modeli [15] |
| LM Studio | `contextOverflowPolicy`: `stopAtLimit`, `truncateMiddle` (sistem + ilk mesaj kalır), `rollingWindow` | — | Özet yok, kırpma [16] |
| SillyTavern Summarize | Her 10 mesajda | Özet istemin içine sistem mesajı olarak | Önceki özeti genişletir [17] |
| Roo Code / Cline | Roo: eşik kaydırıcısı, varsayılan %100; Cline: ~%80 **[resmi değil]** | — | Özel özet istemi, elle "Condense" [18][19] |
| Aider | Geçmiş bütçesi: girdi sınırı/16 (1024–8192) | Bütçenin yarısı kadar son mesaj aynen | Baştaki kısım özyinelemeli özetlenir, "zayıf model" ile [20] |
| LangChain | `ConversationSummaryBufferMemory` 2000 token; `SummarizationMiddleware` son 20 mesaj | En eski mesajlar özete taşınır | [21][22] |
| LlamaIndex | `ChatSummaryMemoryBuffer`: bağlamın %75'i | — | "Write a concise summary…" [23] |

Bulgular:
- **Bağlam büyüdükçe kalite düşüyor** (context rot, 18 model) ve ortadaki bilgi en kötü hatırlanıyor (lost in the middle). Sıkıştırma yalnız sığdırmak için değil, doğruluk için de yararlı [24][25][26].
- **Özet kalitesinde zayıf nokta somut ayrıntılar** (hangi dosya, hangi değer): Factory'nin 36 611 mesajlık değerlendirmesinde en düşük puan "artifact trail" [27]. Anthropic "önce recall, sonra precision" için ayarlamayı öneriyor [24].
- **llama.cpp istem önbelleği önek eşleşmesi:** `cache_prompt` açık (varsayılan); sunucu yeni istemi öncekiyle karşılaştırıp yalnız farklılaşan son kısmı işliyor. Baştaki mesajları özetle değiştirmek önbelleğin neredeyse tamamını geçersiz kılar. **Seyrek ve büyük parçalarla sıkıştırmak** gerekir. Bağlam kaydırma (`--context-shift`) varsayılan kapalı; kapalıyken dolan bağlam isteği hatayla bitirir [28].
- llama-server `POST /tokenize` (`content`) token sayar; `GET /props` `default_generation_settings.n_ctx` slot başına bağlamı verir; `/v1/messages/count_tokens` şablonla birlikte sayar (gateway bunu geçiriyor) [28].

## 2. Seçilen yöntem ve varsayılanlar

- **Yuvarlanan özet + son turlar aynen:** en eski turlar tek bir özete, son N tur olduğu gibi. Sonraki sıkıştırma önceki özeti yeni özete katar. Tur birimi (kullanıcı mesajı + tüm asistan/araç turları): araç çağrısı ile sonucu ayrılmaz.
- **Eşik %80 (varsayılan; yüzde ya da sabit token seçilebilir):**
  - Yerel modelde özeti çoğunlukla aynı model, aynı bağlamda yazar. Özetlenecek kısım (eşik − kalan turlar) + talimat + özet çıktısı tek isteğe sığmalı. %80'de 4096 token'lık bağlamda ~3,3K girdi + 512 özet sığıyor. Codex'in %90'ı ya da Claude Code'un ~%95'i büyük bağlamlı bulut modelleri içindir; küçük yerel bağlamda parçalı özet gerektirir (o da var, ama yavaş).
  - llama-server'da bağlam kaydırma varsayılan kapalı: dolarsa istek hata verir. %20 pay yeni mesaja ve yanıta yer bırakıyor.
  - Open WebUI (80K), LlamaIndex (%75), Cline (~%80) ile aynı bölgede; context rot'a karşı da erken davranmak iyi.
- **Son 2 tur aynen:** konuşmanın son konusu ve yanıt üslubu kayıpsız kalır. Bu turlar bağlamın %40'ını geçerse daha azı tutulur (Aider bütçenin yarısını, Open WebUI %40'ı tutuyor).
- **Otomatik sıkıştırma en az bağlamın %10'u kadar metni özetler:** daha azı için istem önbelleğini kırmaya değmez (Anthropic `clear_at_least` mantığı). Daha azsa ileti olduğu gibi gönderilir.
- **Uzun araç sonuçları kısaltılır (varsayılan açık):** özetleyiciye ilk 300 karakter + kaç karakter atıldığı. Hangi aracın neyle çağrıldığı kalır (Anthropic, OpenCode).
- **Özet bütçesi:** bağlamın 1/8'i, 256–2048 token (Anthropic alt ajan özetleri 1–2K).
- **Özet sistem mesajında:** `<conversation_summary>` bloğu sistem isteminin sonuna eklenir. Bazı Jinja şablonları (Gemma, Mistral) art arda iki kullanıcı mesajını reddediyor, sistem mesajı her şablonda başta kabul ediliyor.
- **Belgeler özetlenmez:** her turda yeniden getiriliyor; özet isteği yalnız adlarını alır.
- **Kazanç yoksa uygulanmaz:** özet + kalan turlar özgün istemden kısa değilse sıkıştırma atılır (`compaction.no_gain`). Ekranda bulundu: tek tur özetlenince 940 token'lık özet istemi 2.108 → 2.243 token büyütmüştü.
- **Geri alınabilir:** özgün mesajlar silinmez; geri alınan sıkıştırma bu dalda otomatik sıkıştırmayı durdurur (yoksa bir sonraki mesajda yeniden yapılırdı).

## 3. Strateji karşılaştırması (gerçek model)

`cargo test -p harness-core compaction::e2e::compare_summary_strategies -- --ignored --nocapture` (`HARNESS_E2E_RUNS=3`). llama-server b11312 (Vulkan), Qwen3-4B-Instruct-2507 Q4_K_M, `-c 4096`, tek slot. Konuşma modelin kendisiyle kuruldu: 8 olgu (kod adı, bütçe, toplantı tarihi/saati, ekip lideri, IP, kedi adı, rapor biçimi, teslim tarihi), ardından bağlamın %75'ine kadar uzun teknik yanıtlar (27 mesaj, 3224 token). Her strateji sohbetin bir kopyasında son 2 turu tutarak sıkıştırdı. Sonra 8 soru ayrı ayrı soruldu (sıcaklık 0); yanıt beklenen değeri içeriyorsa "hatırlandı".

| Strateji | Hatırlanan | İstem (token) | Özet (token) | Süre | İstek |
|---|---|---|---|---|---|
| Sıkıştırma yok (tam geçmiş) | 8/8 | 3246 | — | — | — |
| Kırpma (özetsiz, son 2 tur) | 1/8 | 879 | 0 | — | — |
| `brief` (tek paragraf, "concisely") | 1/8 · 1/8 · 1/8 | 1257–1295 | 338–376 | 4,6–7,0 sn | 1 |
| **`structured`** (bölümlü, somut ayrıntılar aynen) | **8/8 · 8/8 · 8/8** | 1351–1394 | 432–475 | 6,2–9,0 sn | 1 |
| `incremental` (aynı istem, bağlamın 1/4'ü kadar parçalar) | 8/8 · 8/8 · 8/8 | 1432–1433 | 512 (bütçe doldu) | 16,4–21,3 sn | 3 |

- `brief` teknik konuları özetleyip olguları düşürüyor; kırpmadan farkı yok. Yalnız "rapor PDF" gibi genel bir tercih kalıyor.
- `structured` olguların hepsini koruyor, istemi %58 küçültüyor ve tek istekte bitiyor. **Varsayılan bu.**
- `incremental` aynı doğrulukta ama 2–3 kat yavaş, özet bütçeyi doldurup kesiliyor. Yalnız özetlenecek kısım tek isteğe sığmadığında (çok büyük sabit eşik ya da küçük bağlamlı özet modeli) parçalı yola düşülüyor; bu durumda `structured` istemi parçalar arasında özeti taşıyarak çalışıyor.

Otomatik akış (`auto_compaction_with_a_real_model`): 7. dolgu turundan sonra kullanım 3310 + 30 ≥ 3276 (%80) → sıkıştırma tetiklendi. 26 mesaj (13 tur) özetlendi, 2 tur kaldı; 5,9 sn, tek istek. Hesaplanan istem 3312 → 1353 token (sunucunun tokenizer'ıyla), sonraki gerçek istem 1373 token. Model özetlenmiş kısımdaki "KESTREL-47" ve "Zeynep Arıkan"ı doğru yanıtladı. Geri alınca istek 31 mesaj ve 3314 token'a döndü.

## 4. Yanıt turu içinde (uzun araç döngüsü)

Tek bir yanıt, araç turlarıyla bağlamı doldurabilir; yeni kullanıcı mesajından önceki tetik buna yetişmez. Playground her araç turundan sonra, sonraki istekten önce aynı eşiği yoklar ve gerekirse sıkıştırır (ARCHITECTURE.md §9.1, "Yanıt turu içinde sıkıştırma"). Yöntem, [Anthropic'in araç sonucu temizleme](#kaynaklar) yaklaşımından farklı olarak sonuçları silmez: eski turlar ve sonuçları özetlenir (sonuçlar özetleyiciye ilk 300 karakteriyle verilir), son tur ve soru aynen kalır, işaretleyici geri alınabilir.

Gerçek model ve gerçek araçla (`compaction::e2e::compaction_in_the_middle_of_a_tool_loop`, Qwen3-4B-Instruct-2507 Q4_K_M, `--jinja`; model `read_file` aracıyla `docs/ROADMAP.md`, `docs/ARCHITECTURE.md`, `docs/FEATURES.md`'yi art arda okuyor, sonra her dosyanın başlığını söylüyor; her okuma ~2.400 token):

| Deneme | Bağlam / eşik | Sonuç |
|---|---|---|
| İlk sürüm: soru özete girmiyor, özet "yapılan çağrılar"ı saymıyor | 8192 / %45 | Her turda sıkıştırma tetiklendi. Özet "kullanıcı bir özet istedi" diye soruyu yanlış anladı ve "hiçbir araç çağrılmadı" yazdı; model aynı dosyaları yeniden okudu, 9 turda bitiremedi. |
| Soru özetleyiciye bağlam olarak veriliyor, talimat çağrıları saydırıyor; soru istekte aynen duruyor | 12288 / %40 | Tur 1'den sonra bir sıkıştırma: 4385 → 2240 token (özet 300 token). Model kalan iki dosyayı okudu, 4 istekte bitirdi; ilk dosyanın başlığı (özetlenmiş turdan) doğru söylendi. |

Bulgular: (1) Yanıtlanan soru özetin içinde değil, özetin yanında aynen gitmeli ve özetleyici onu görmeli; yoksa özet yanlış hedefe yazılıyor. (2) Özet "hangi araç çağrıldı" bilgisini açıkça taşımalı; yoksa model işi tekrarlıyor. (3) Eşik dosya başına birkaç bin token'lık sonuçlara göre çok sıkıysa her turda sıkıştırmak kazanç sağlamaz; `compaction.no_gain` ve "yanıt başına bir başarısız deneme" korumaları bu yüzden var.

## Kaynaklar

1. https://code.claude.com/docs/en/context-window
2. https://code.claude.com/docs/en/costs
3. https://code.claude.com/docs/en/model-config ve https://code.claude.com/docs/en/env-vars
4. https://platform.claude.com/docs/en/docs/build-with-claude/context-editing
5. https://platform.claude.com/docs/en/build-with-claude/compaction-threshold
6. https://platform.claude.com/docs/en/build-with-claude/compaction
7. https://raw.githubusercontent.com/openai/codex/main/codex-rs/protocol/src/openai_models.rs
8. https://raw.githubusercontent.com/openai/codex/main/codex-rs/core/src/compact.rs ve https://raw.githubusercontent.com/openai/codex/main/codex-rs/prompts/templates/compact/prompt.md
9. https://developers.openai.com/api/docs/guides/compaction
10. https://support.claude.com/en/articles/11647753-how-do-usage-and-length-limits-work
11. https://www.techradar.com/ai-platforms-assistants/chatgpt/i-pushed-chatgpt-toward-its-hidden-chat-limit-heres-what-actually-happens-when-you-reach-it
12. https://cursor.com/changelog/1-6
13. https://opencode.ai/docs/config/
14. https://gist.github.com/badlogic/cd2ef65b0697c4dbe2d13fbecb0a0a5f (kaynak kod incelemesi, üçüncü taraf)
15. https://docs.openwebui.com/troubleshooting/context-window/
16. https://lmstudio.ai/docs/typescript/api-reference/llm-prediction-config-input
17. https://raw.githubusercontent.com/SillyTavern/SillyTavern/release/public/scripts/extensions/memory/index.js
18. https://roocodeinc.github.io/Roo-Code/features/intelligent-context-condensing
19. https://cline-cline.mintlify.app/models/context-windows ve https://github.com/cline/cline/issues/14329
20. https://raw.githubusercontent.com/Aider-AI/aider/main/aider/models.py ve https://raw.githubusercontent.com/Aider-AI/aider/main/aider/prompts.py
21. https://raw.githubusercontent.com/langchain-ai/langchain/langchain==0.3.27/libs/langchain/langchain/memory/summary_buffer.py
22. https://raw.githubusercontent.com/langchain-ai/langchain/master/libs/langchain_v1/langchain/agents/middleware/summarization.py
23. https://raw.githubusercontent.com/run-llama/llama_index/main/llama-index-core/llama_index/core/memory/chat_summary_memory_buffer.py
24. https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents
25. https://www.trychroma.com/research/context-rot
26. https://arxiv.org/abs/2307.03172
27. https://factory.com/news/evaluating-compression
28. https://raw.githubusercontent.com/ggml-org/llama.cpp/master/tools/server/README.md
