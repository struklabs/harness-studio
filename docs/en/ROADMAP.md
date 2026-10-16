# Roadmap

This is the public roadmap of Harness Studio. It shows direction, not promises: priorities shift with feedback. To suggest or discuss something, open a thread in [GitHub Discussions](https://github.com/struklabs/harness-studio/discussions) or an [issue](https://github.com/struklabs/harness-studio/issues). Shipped changes are in the [changelog](../CHANGELOG.md); how Harness compares with similar tools is in [FEATURES.md](FEATURES.md) and [COMPARISON.md](COMPARISON.md).

**Product goal.** A local AI control center that manages text, image, speech, embedding, reranking, and tool workloads in one place. llama.cpp is the default runtime; others are modules you install when you need them.

**Coverage goal.** Every task and library in the Hugging Face model filter (multimodal, computer vision, NLP, audio, tabular, reinforcement learning, graph) runs in Harness: on native engines (llama.cpp, ONNX Runtime, stable-diffusion.cpp, whisper.cpp, sherpa-onnx, ggml audio engines) where they exist, and on optional, managed Python engines where they do not.

**Principles.**
- Do what LM Studio and Ollama do, without hiding llama.cpp's advanced controls.
- Be better at observability and at agent use, with low resource use.
- Local first, cloud optional.
- The app, the CLI, the Control API, and MCP all use the same Rust services.

**Release 0.1.1.** Its focus is the stability of existing features, bug fixes, performance, and UX. It ships once the thinking selector (per-chat levels Off, Light, Normal, Deep, in main) and the rest of the roadmap below are done. Server mode and the tools, MCP, and skills catalog are planned for 0.2.0, not 0.1.1.

## Now

- **Hugging Face task coverage.** Measured on the most downloaded models of each task (October 2026): text generation runs 295 of the top 300 and vision-language models 98%, but text-to-speech is at 29%, speech recognition 39%, image-to-image 45%, and audio-to-audio, speaker diarization, and NLP and computer-vision classification are not covered yet. Work in progress:
  - Python-free pipelines on ONNX Runtime behind `/v1/hf/{model}` (Hugging Face Inference API format). In main: text classification, zero-shot, token classification (NER), fill-mask, question answering, image classification, object detection, segmentation, depth estimation, zero-shot image classification (CLIP, SigLIP), image feature extraction, audio classification, and CTC speech recognition (wav2vec2, HuBERT, WavLM). Also in main: image-to-text (ViT + GPT-2 captioning), zero-shot object detection (OWL-ViT, OWLv2), and text-to-text generation, summarization, and translation (T5, BART, Marian) with beam search.
  - ggml audio engines: audio.cpp text to speech is in main as an experimental engine, off by default (it has no authentication of its own, so it runs on a random loopback port behind the gateway's speech route only). CrispASR speech recognition (Canary, wav2vec2-family models and more) and text to speech (Kokoro, Pocket TTS) are in main, and audio.cpp also does audio separation and enhancement (Process Audio). llama.cpp audio input is in main: a chat model whose projector hears (LFM2.5-Audio, Qwen3-ASR, Voxtral...) also transcribes on `/v1/audio/transcriptions` and in Transcribe, in windows of at most 25 s. LFM2.5-Audio's audio output waits on upstream llama.cpp (its server has no audio out yet). transcribe.cpp speech recognition (handy-computer's GGUFs) is in main, in a managed Python environment with a CUDA build. In progress: llama.cpp text to speech (Qwen3-TTS).
  - In main: image editing (`/v1/images/edits`, FLUX.2 klein), video generation (Wan, as an asynchronous `/v1/videos` job API in the OpenAI Videos format), and more sherpa-onnx families (speaker diarization, Nemotron, Cohere Transcribe, MMS voices).
  - In main: decision endpoints (`/v1/decisions` and `/v1/systemone`) with a decision-model task filter in the Model Hub.
  - Real-inference runs on Windows AMD: ONNX models behind `/v1/hf/{model}` on DirectML and CPU, the 35 most downloaded across 11 vision and NLP tasks, 49/70 → 70/70 attempts. Audio-to-audio and diarization run through the Model Hub and `/v1/audio/process`: RE-USE, HTDemucs (including 6-stem), Nemotron 3 Diarization, and Sortformer.
  - Real-inference runs on Apple Silicon, top 30 Mac-relevant models per task (a different measure from the Hub-verdict percentages above): text to speech 2/30 → 13/30, speech recognition 7/30 → 14/30, and audio-to-audio 0/30 → 2/30 (vocal separation with Mel-Band-RoFormer).
  - Model Hub compatibility marks that name the engine that runs a repository, and refuse formats nothing local can run (AWQ, FP8, NVFP4 checkpoints) before downloading.
- **Platform verification.** 0.1.0 ships the Windows installer (NSIS), signed and notarized macOS DMGs, and a Linux `.deb` package as a preview. A Linux AppImage with the GStreamer media plugins and automatic updates is in main and ships in the next release. Verify Linux on more machines (AMD, NVIDIA, Intel) and distributions, then drop the preview label. Done so far: the full Windows NVIDIA round (install and wizard, the updater across several release candidates including its handoff to the installer, chat and tool approval, out-of-memory recovery, the browser-origin guard, GPU memory telemetry, gateway load, idle-unload, and eviction, stable-diffusion.cpp and diffusers images through the gateway, audio engines on CUDA, the ONNX CUDA to DirectML fallback, and a clean uninstall that leaves only the documented data); NVML telemetry on NVIDIA; the browser-origin guard on macOS; on Windows, start-at-login through our own quoted per-user Run entry, the updater handoff, and read-only long-path handling.
- **First-run polish.** An end-to-end first-run flow (wizard, find a model, download, start, chat) on empty data folders on every platform. Verified on Windows with an NVIDIA GPU, including the first-run tour, and on macOS (Apple Silicon); the other platforms remain.
- **Benchmarks on more hardware.** Run the comparison suite (`bench/compare/`) on Linux, and re-measure the other tools in the same session. NVIDIA/CUDA on Windows is done, with the gateway tuned from the results: Harness against plain llama.cpp on an RTX 3050 Ti ([docs/benchmarks/2026-10-15-nvidia](../benchmarks/2026-10-15-nvidia/summary.md)).
- **Accessibility checks.** Small windows, high interface scale, and Windows forced-colors mode. Small windows and forced-colors (high contrast) are done on Windows, as are screen-reader and keyboard fixes (slider and repeated-button names, command palette, heading order, model picker focus, scrollable code blocks) and a section-menu drawer for narrow windows and high display scales; a VoiceOver pass on macOS, with fixes (the command palette, the app behind the first-run wizard, the window title, the palette's result list name); checks of high interface scale and Linux remain.
- **Linux desktop details (done).** Native `<select>` styling now follows the theme, the tray icon works through StatusNotifierItem, GPU memory reporting counts GTT on AMD APUs (fdinfo), NVML telemetry is verified on NVIDIA, and OpenTelemetry export works without a keychain. The gray strip once reported on Linux was not Harness (it was the Ubuntu installer window), so that report is closed.
- **Harness Link (done).** Add a remote Harness (address and token) under Connections and use its models in the Playground, the local gateway (as `remote:<link>/<model>`), and a read-only Remote group in My Models. Tokens live in the OS keychain; `http` is limited to local or private networks with explicit consent, and HTTPS goes through a reverse proxy until server mode exists. Chat, completions, responses, embeddings, rerank, and messages work; image, audio, and video do not.

## Next

- **Server mode (planned for 0.2.0).** Run Harness on your own (often headless Linux) server and manage everything from the browser:
  - A web interface served by `harness serve`, with password or token sessions.
  - A Server Management area: domain and HTTPS, access and ports, users and tokens, service status, updates, logs, backup and restore.
  - Automatic SSL from the interface (Let's Encrypt, with DNS and port checks).
  - A Linux server package (tar.gz, .deb) with a one-line installer and a systemd service.
  - Docker images for CUDA, ROCm, Vulkan, and CPU.
- **Python engines (optional).** A managed, removable Python environment per engine (uv, hash-pinned locks, nothing system-wide), so every Hugging Face task and library can run: a `python-hf` worker for all transformers pipelines and diffusers text-to-image (a fallback where no GGUF or ONNX build exists), then vLLM and SGLang as OpenAI-compatible servers behind the gateway. `python-hf` is in main, and its speech and image models also answer the OpenAI audio and image routes; diffusers models also edit images (image-to-image and inpainting), with sampler choice and a per-request image size limit; CUDA vLLM moves to the Linux NVIDIA stage, since vLLM does not run natively on Windows. Workers listen on loopback only, load models offline with `trust_remote_code` off by default, and on Linux run without network interfaces (`unshare -rn`; Unix sockets such as D-Bus stay reachable); on Windows and macOS isolation is environment-based (offline Hub, no proxy).
  - CPU vLLM was evaluated and is deferred (2026-10-14, v0.31.0 on a Ryzen 7 4700U, AVX2). The official CPU wheel runs offline in a network namespace, but the environment is about 3 GB, the first request waits about 2.5 minutes while triton-cpu compiles kernels with the system C compiler (gcc is needed at run time), and a 0.5B model makes about 12 tokens per second, well behind llama.cpp on the same laptop. vLLM targets CUDA instead.
- **Control API SDKs.** TypeScript and Python clients generated from the OpenAPI schema are in main (`sdk/`) and tested in CI against a running server. The TypeScript SDK is published on npm as `@struklabs/harness-studio-sdk` 0.1.0, with provenance, through a manually triggered workflow (`workflow_dispatch`) that uses npm Trusted Publishing (OIDC, no token). The Python SDK goes to PyPI later, after Trusted Publishing is set up.
- **MLX engine** for Apple Silicon: chat (mlx-lm), vision chat (mlx-vlm), text to speech (mlx-audio: Qwen3-TTS, Chatterbox Turbo, Soprano, VoxCPM2, Spark-TTS), and speech recognition (mlx-audio Whisper and Parakeet) are in main and ship in the next release. Chatterbox Turbo, Soprano, VoxCPM2, and Spark-TTS now speak on Apple Silicon, and OuteTTS, Orpheus, and Dia download their codec (DAC or SNAC) with the model. Also in main: macOS out-of-memory (jetsam) detection, a memory pre-check that refuses MLX loads that cannot fit the GPU share of unified memory, and a byte-bounded prompt cache that stops long chats from exhausting memory. CSM now speaks on Apple Silicon, with its Mimi codec.
- **Auto-update on every platform.** The signed in-app updater ships for Windows in 0.1.0 and for macOS from 0.1.1; Linux packages follow. The Windows updater has been verified end to end, including stopping servers before the installer starts.
- **Signed Windows installer.** A code-signed Windows installer (the macOS app is already signed and notarized).

## Later

- **Tool and MCP registry (planned for 0.2.0, together with a skills catalog).** A community index (`struklabs/harness-registry`) of tools and MCP servers: manifests with permissions, publisher, source, version, and SHA-256; CI validation; install into the sandbox with hash verification; publish from inside the app; verified and community labels.
- **ComfyUI bridge** to a ComfyUI instance you already run.
- **Tool router** that selects relevant tools per request when many are installed (using the embedding and rerank stack).
- **More Create pages** for image, code, and writing generation, and for audio runtimes.
- **In-app coding workspace** powered by local models.
- **Under consideration (not committed):**
  - A faster-whisper engine, deferred until there is an NVIDIA benchmark. For now the Model Hub points CTranslate2 Whisper exports to their whisper.cpp builds.
  - VieNeu text to speech, parked because it needs the third-party SEA-G2P native library.
- **Undecided:** OpenVINO and MLC.

## Not planned

- Pulling from the Ollama registry (`hf.co/...` references are supported).
- Engines that need Python for normal use when a native engine covers the same models (text-embeddings-inference; faster-whisper is under consideration, see Later). Python engines stay optional add-ons, never required for the core app.
