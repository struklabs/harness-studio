# Changelog

All notable changes to Harness Studio are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

Release notes with system requirements and known limitations (English and Turkish): [RELEASE_NOTES_0.1.0.md](en/RELEASE_NOTES_0.1.0.md).

## [Unreleased]

- Image generation: GPU memory failures show localized guidance, blocking server names when available, and a Manage Servers action.
- Görsel üretimi: GPU belleği hataları yerelleştirilmiş yönlendirme, varsa belleği tutan sunucu adları ve Sunucuları Yönet eylemi gösterir.
- Playground: the Image tab header model chip follows the image model selection.
- Oyun Alanı: Görsel sekmesinin başlığındaki model etiketi seçili görsel modelini izler.
- Model Hub: engine installation failures appear below the banner with expandable, scrollable output.
- Model Hub: motor kurulumu hataları banner altında genişletilebilir ve kaydırılabilir çıktıyla gösterilir.
- Model Hub: exact repository IDs are preserved in search and ranked first regardless of letter case.
- Model Hub: tam depo kimlikleri aramada korunur ve büyük/küçük harften bağımsız olarak ilk sıraya alınır.
- Model Hub: variant recommendations prefer current files over old, legacy and deprecated folders.
- Model Hub: varyant önerileri old, legacy ve deprecated klasörleri yerine güncel dosyaları tercih eder.

- Playground: per-chat Thinking levels and optional request budgets, model-aware OpenAI/Ollama translation, and copyable reasoning with saved duration and inline `<think>` handling. Presets remain independent; split view keeps each side's preset.
- Playground: sohbete özel Düşünme düzeyleri ve isteğe bağlı istek bütçesi, modele göre OpenAI/Ollama çevirisi, kayıtlı süre ve satır içi `<think>` desteğiyle kopyalanabilir düşünme metni. Ön ayarlar bağımsızdır; bölünmüş görünüm her tarafın ön ayarını kullanır.

- Experimental audio separation and enhancement with audio.cpp, behind the existing off-by-default switch. Playground > Speech has a new Process Audio panel (upload or record a WAV, play and save each output channel), and Model Hub marks the seven supported families as Experimental: BS-RoFormer, Mel-Band RoFormer, SAM Audio, Sidon, Apollo, AudioSR (GPU backend only) and UniverSR. The gateway's new `POST /v1/audio/process` accepts a WAV as multipart or base64, enforces size and duration limits, rejects unknown and path fields, and rebuilds the request for the worker; outputs come back inline and the worker's task, model and UI routes stay private. See [family details](en/FEATURES.md#experimental-audiocpp-speech-and-processing).
- Ses ayırma ve iyileştirme (deneysel, varsayılan kapalı audio.cpp anahtarının arkasında): Playground > Konuşma'da yeni Sesi İşle paneli (WAV yükleme veya kayıt, her çıktı kanalını dinleme ve kaydetme). Model Hub, desteklenen yedi aileyi Deneysel olarak işaretler: BS-RoFormer, Mel-Band RoFormer, SAM Audio, Sidon, Apollo, AudioSR (yalnız GPU arka ucu) ve UniverSR. Gateway'in yeni `POST /v1/audio/process` ucu WAV'ı multipart veya base64 olarak alır, boyut ve süre sınırı uygular, bilinmeyen ve yol alanlarını reddeder ve isteği işçi için yeniden kurar; çıktılar gövdede döner, işçinin görev, model ve arayüz uçları dışarı açılmaz.

- audio.cpp is experimental and off by default. Settings > Runtime > Installed Runtime > Other Runtimes requires risk confirmation: its local server has no authentication or Host/Origin checks and accepts server-side file paths. Managed platform builds serve TTS GGUF families through `/v1/audio/speech` with Playground voice selection from companion embeddings. The gateway exposes validated speech and processing requests only. Speech uses discovered voices and inline base64 references up to 5 MiB. Workers stop after five idle minutes.
- audio.cpp deneyseldir ve varsayılan olarak kapalıdır. Ayarlar > Model Motoru > Kurulu Model Motoru > Diğer Model Motorları altında risk onayıyla etkinleştirilir. Yönetilen platform derlemeleri TTS GGUF ailelerini `/v1/audio/speech` üzerinden çalıştırır; Playground yardımcı ses gömmelerini listeler. İşçiler beş dakika boş kalınca durur.
- Security: the gateway (OpenAI/Anthropic port, Ollama-compatible port) and the Control API now refuse browser requests from any origin other than the Harness window or the user's allowed browser origins (`403 forbidden_origin` / `api.forbidden_origin`), including preflights, `Origin: null` and cross-site requests without `Origin`. A web page can no longer make the machine load models with a "simple" cross-site POST. Clients without an `Origin` header (curl, SDKs, CLI) are unchanged.
- Güvenlik: ağ geçidi (OpenAI/Anthropic portu, Ollama uyumlu port) ve Control API, Harness penceresi ile kullanıcının izin verdiği tarayıcı origin'leri dışındaki tarayıcı isteklerini reddeder (`403 forbidden_origin` / `api.forbidden_origin`); ön uçuş, `Origin: null` ve `Origin` göndermeyen siteler arası istekler dahil. Bir web sayfası artık "basit" bir siteler arası POST ile makineye model yükletemez. `Origin` göndermeyen istemciler (curl, SDK'lar, CLI) değişmez.

Planned work is tracked in the [roadmap](en/ROADMAP.md).

### Added
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
- Playground: with tools on, a new conversation on a model a local Harness server has loaded sends its fixed prefix (system prompt, tool guidance, tool definitions; about 2.2k tokens) once ahead with one output token, so llama-server's prompt cache holds it and the first message processes only the user's text (Qwen3-4B on a CPU: first token 61 s → 0.5 s). Only to loopback servers, once per conversation; the first message waits for a warm-up still in flight and is never slower. Tool definitions are listed in a fixed order (built-ins first, then by name), so the prefix does not change with registration order. The warm-up is not counted in the Dashboard, Analytics or traces: the gateway drops the record of a one-token, non-streamed request marked `x-harness-purpose: warmup`, and only for Harness's own client token.
- Speech to text on Linux (whisper.cpp ships a CPU-only build there): onboarding and the Hub variant drawer recommend whisper `small` before `large-v3-turbo` (an 11 s clip: 4.2 s instead of 18.9 s with the language given). The Playground Speech tab starts with the interface language as the audio language, so whisper skips language detection (it doubles the time on a CPU: 7.3 s vs 4.2 s with `small`); clearing it detects the language.
- Linux: CrispASR, audio.cpp, the ONNX sidecar and the sherpa-onnx speech tools run without network (a new user + network namespace, as the Python engine does). The servers only listen on TCP, so Harness runs them behind a small bridge inside the namespace: it brings up the namespace's own loopback, starts the server there and passes one private Unix socket to it, which Harness relays the server's port to. Nothing inside reaches the internet, the LAN or services on this machine's loopback. Where unprivileged user namespaces are off (some containers and hardened kernels), they run as before and the server log says so.
- Playground: with a model under 5B parameters, tools are offered with short descriptions (first sentence; names, parameters and required fields unchanged). Qwen3-0.6B picked the right tool in 49 of 60 test prompts instead of 36, with about 440 fewer prompt tokens; Qwen3-4B stayed at 60 of 60.

### Fixed
- Linux updater: a .deb install was offered "Install Update" like the AppImage. The updater then downloaded the AppImage and rejected it, or asked for pkexec/sudo dpkg. A .deb install now says it cannot update itself and opens the releases page instead. An AppImage updated after a login start no longer comes back hidden in the tray (`--minimized` is not passed to the new version).
- Linux güncelleyici: .deb kurulumuna AppImage gibi "Güncellemeyi Kur" sunuluyordu; güncelleyici AppImage'ı indirip reddediyor ya da pkexec/sudo dpkg istiyordu. .deb kurulumu artık kendini güncelleyemeyeceğini söylüyor ve sürümler sayfasını açıyor. Oturum açılışında başlamış bir AppImage güncellemeden sonra tepside gizli açılmıyor (`--minimized` yeni sürüme geçirilmiyor).
- Linux, AMD APUs: GPU memory showed only the BIOS carve-out (for example 467/512 MB), and a fully offloaded llama-server about 15 MB, because an APU's GPU buffers live in GTT. On integrated AMD GPUs the Dashboard total and the per-process breakdown now count VRAM plus GTT (from `mem_info_gtt_*` and the fdinfo `drm-resident-gtt`). Discrete cards are unchanged.
- Linux, AMD APU'lar: GPU belleği yalnız BIOS ayırmasını (ör. 467/512 MB), tamamen GPU'ya yüklü bir llama-server'ı da yaklaşık 15 MB gösteriyordu; APU'da GPU tamponları GTT'de durur. Tümleşik AMD GPU'larda Dashboard toplamı ve süreç dökümü artık VRAM ile GTT'yi birlikte sayıyor (`mem_info_gtt_*` ve fdinfo `drm-resident-gtt`). Ayrık kartlar değişmedi.
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
- Measured comparison against LM Studio, Ollama, and llama.cpp: [COMPARISON.md](en/COMPARISON.md) and [benchmarks](benchmarks/).

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

[Unreleased]: https://github.com/struklabs/harness-studio/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/struklabs/harness-studio/releases/tag/v0.1.0
