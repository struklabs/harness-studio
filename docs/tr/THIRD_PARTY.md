# Üçüncü Taraf İncelemesi

[English](../en/THIRD_PARTY.md) · İnceleme: 2026-10-20, 0.2 W3.

Harness Studio çekirdeği MIT lisanslıdır. Uygulamaya gömülü kod ile kullanıcının isteğiyle indirilen bileşenler ayrıdır; isteğe bağlı indirme lisans yükümlülüklerini kaldırmaz. Aşağıdaki lisanslar kod içindir; model ağırlığı, veri kümesi, marka, hizmet ve GPU sürücüsü haklarını kapsamaz. Tam sürüm/kaynak bağlantıları [İngilizce tabloda](../en/THIRD_PARTY.md) bulunur.

| Bileşen | Lisans | Dağıtım ve Atıf |
| --- | --- | --- |
| transcribe.cpp 0.4.0 | MIT | Hash ile sabitlenmiş Python/yerel wheel isteğe bağlı indirilir; Harness işçisi uygulamada gelir. © 2026 The transcribe.cpp authors. MIT bildirimi korunur; CUDA/cuBLAS için NVIDIA koşulları ayrıca incelenir. |
| liquid-audio 1.3.0 ve LFM2.5-Audio ağırlıkları | LFM Open License v1.0 (Apache-2.0 tabanlı, ticari kullanım sınırlı) | Hash ile sabitlenmiş Python wheel'leri (PyTorch CPU ve `liquid-audio.lock`) isteğe bağlı indirilir; Harness işçisi uygulamada gelir; ağırlıkları kullanıcı Hugging Face'ten indirir. © Liquid AI, Inc. Ticari kullanım yalnızca yıllık geliri 10 milyon ABD dolarının altındaki tüzel kişilere lisanslıdır (kâr amacı gütmeyenlerin ticari olmayan kullanımı muaf); üstü için Liquid AI ile ayrı sözleşme gerekir. Yeniden dağıtımda lisans kopyası ve değiştirilen dosyalar için bildirim gerekir. Paket NVIDIA NeMo (Apache-2.0) kaynaklı kod, canary-180m-flash türevi kontrol noktası (CC-BY 4.0) ve Kyutai Mimi (MIT) içerir. |
| Hermes Agent | MIT | Onaylı upstream kurucu veya mevcut kurulum; uygulamaya gömülmez. © 2025 Nous Research. Kurucu upstream stable izler; bağımlılıkların tamamı Harness tarafından sabitlenmez. Paketlemeden önce kurulu sürüm incelenir. |
| llama.cpp / whisper.cpp | MIT | İsteğe bağlı sürüm indirmesi veya yerel ikili. ggml yazarları; MIT ve GPU kitaplığı bildirimleri korunur. Özel fork ayrıca incelenir. |
| stable-diffusion.cpp | MIT | İsteğe bağlı indirme; © 2023 leejet. Ağırlık lisansı ayrıdır. |
| mistral.rs | MIT | İsteğe bağlı indirme; © 2024 Eric Buehler. Bağımlılık bildirimleri korunur. |
| sherpa-onnx | Apache-2.0 | İsteğe bağlı indirme; lisans/NOTICE korunur, dağıtılan değişiklikler belirtilir. Ses/model lisansları ayrıdır. |
| ONNX Runtime | MIT | İsteğe bağlı yerel kitaplık; Harness bağdaştırıcısı uygulamada. Microsoft katkıcıları; seçilen ikilinin ThirdPartyNotices dosyası korunur. |
| Lemonade | Apache-2.0 ve üçüncü taraf bildirimleri | İsteğe bağlı/yönetilen veya haricî kurulum; motor ve GPU kitaplıkları ayrıca incelenir. |
| MLX, mlx-lm, mlx-vlm, mflux | MIT | Apple Silicon için isteğe bağlı kilitli Python ortamları; Apple katkıcıları, Prince Canuma, Filip Strand ve ilgili katkıcıların bildirimleri korunur. |
| Hugging Face Python ortamları | Paket başına | İsteğe bağlı kilitli indirmeler: PyTorch BSD türü; Transformers/Diffusers Apache-2.0; NumPy/SciPy BSD türü; Pillow HPND. Her bağımlılık ayrıca incelenir. |
| uv / yönetilen CPython | MIT OR Apache-2.0 / PSF-2.0 ve ek bildirimler | İsteğe bağlı bootstrap/motor; python-build-standalone bildirimleri dahil sürüm lisansları korunur. |
| rquickjs üzerinden QuickJS | MIT | Rust çekirdeğine gömülü; Fabrice Bellard, Charlie Gordon, Ben Noordhuis, Saúl Ibarra Corretgé ve rquickjs katkıcıları. İkiliyle lisans bildirimi dağıtılır. |
| Tauri / Svelte / SQLite / yazı tipleri | MIT OR Apache-2.0 / MIT / kamu malı / OFL-1.1 | Gömülü bağımlılıklar; Cargo.lock/package-lock.json sürümleri belirler. Geist/IBM Plex bildirimleri korunur. Bu tablo tam SBOM değildir. |
| Resmî Kayıt Defteri | Öğe başına | Kaynak/sürüm/lisans gösterilir. Mevcut test örnekleri JSON Formatter, Release Notes ve Microsoft Learn MCP yapılandırması MIT bildirir. Microsoft hizmetinin veya gelecekteki tüm öğelerin lisansı sayılmaz. Gerçek revizyon ayrıca incelenir. |
| Kullanıcı yapımı araçlar / yerel MCP | Kullanıcı/upstream lisansı | Kullanıcı kurulumu; manifest beyanı eser sahipliği veya ticari hak kanıtı değildir. |

## Yeniden Dağıtım, Ticari Kullanım, Gömme ve Barındırma

MIT, telif/izin bildirimi korunduğunda ticari kullanım, gömme, değiştirme ve yeniden dağıtıma izin verir. Apache-2.0 için lisans, NOTICE, değişiklik ve patent koşulları geçerlidir. Bunlar marka onayı, model/veri kümesi, API hesabı veya GPU kitaplığı hakları vermez. Barındırmada da bileşen/hizmet/model koşulları incelenir. Microsoft Learn MCP yapılandırmasının MIT olması uzak hizmeti lisanslamaz. Model lisansları; atıf, ticari olmayan kullanım, erişim onayı ve kullanım koşulları açısından ayrı kontrol edilir.

Sürüm yayımlamadan veya isteğe bağlı indirmeyi paketlemeye çevirmeden önce tam tag/hash kaydedin; LICENSE/NOTICE ve ikili bağımlılık bildirimlerini koruyun; ilgili kilit/arşivdeki tüm wheel/kitaplıkları inceleyin; amaçlanan ticari kullanım/gömme/barındırma haklarını doğrulayın; çözümlenmemiş ikilileri yeniden dağıtmayın. Ana projenin MIT etiketi tüm bağımlılıklarını onaylamaz. Çıkarılan motor arşivlerinin sıradan dosyaları korunur; yerel ikilinin atıfları mevcut kurulumun sorumluluğundadır. Değişken Hermes kurucusu ve kayıt defteri revizyonları güncellemede yeniden incelenir.

## Hakkında Atıfları

Ayarlar > Güncellemeler altındaki Harness Studio Hakkında / Üçüncü Taraf Lisansları bölümü transcribe.cpp ve Hermes Agent atıflarını, inceleme bağlantısını gösterir. macOS yerel Hakkında penceresi de bu atıfları içerir. Yeniden dağıtılan ikilinin tam bildirimleri ikiliyle verilmelidir; Hakkında bağlantısı tek başına yeterli değildir.
