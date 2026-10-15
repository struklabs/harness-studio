# Roadmap

This is the public roadmap of Harness Studio. It shows direction, not promises: priorities shift with feedback. To suggest or discuss something, open a thread in [GitHub Discussions](https://github.com/struklabs/harness-studio/discussions) or an [issue](https://github.com/struklabs/harness-studio/issues). Shipped changes are in the [changelog](../CHANGELOG.md); how Harness compares with similar tools is in [FEATURES.md](FEATURES.md) and [COMPARISON.md](COMPARISON.md).

**Product goal.** A local AI control center that manages text, image, speech, embedding, reranking, and tool workloads in one place. llama.cpp is the default runtime; others are modules you install when you need them.

**Coverage goal.** Every task and library in the Hugging Face model filter (multimodal, computer vision, NLP, audio, tabular, reinforcement learning, graph) runs in Harness: on native engines (llama.cpp, ONNX Runtime, stable-diffusion.cpp, whisper.cpp, sherpa-onnx, ggml audio engines) where they exist, and on optional, managed Python engines where they do not.

**Principles.**
- Do what LM Studio and Ollama do, without hiding llama.cpp's advanced controls.
- Be better at observability and at agent use, with low resource use.
- Local first, cloud optional.
- The app, the CLI, the Control API, and MCP all use the same Rust services.

## Now

- **Hugging Face task coverage.** Measured on the most downloaded models of each task (October 2026): text generation runs 295 of the top 300 and vision-language models 98%, but text-to-speech is at 29%, speech recognition 39%, image-to-image 45%, and audio-to-audio, speaker diarization, and NLP and computer-vision classification are not covered yet. Work in progress:
  - Python-free pipelines on ONNX Runtime behind `/v1/hf/{model}` (Hugging Face Inference API format). In main: text classification, zero-shot, token classification (NER), fill-mask, question answering, image classification, object detection, segmentation, depth estimation, zero-shot image classification (CLIP, SigLIP), image feature extraction, audio classification, and CTC speech recognition (wav2vec2, HuBERT, WavLM). In progress: image-to-text, then text-to-text generation, summarization, and translation (T5, BART, Marian).
  - ggml audio engines: audio.cpp text to speech is in main as an experimental engine, off by default (it has no authentication of its own, so it runs on a random loopback port behind the gateway's speech route only). In progress: CrispASR speech recognition, then transcribe.cpp and llama.cpp audio input and output (LFM2-Audio, Qwen3-TTS).
  - In main: image editing (`/v1/images/edits`, FLUX.2 klein), video generation (Wan, as an asynchronous `/v1/videos` job API in the OpenAI Videos format), and more sherpa-onnx families (speaker diarization, Nemotron, Cohere Transcribe, MMS voices).
  - Model Hub compatibility marks that name the engine that runs a repository, and refuse formats nothing local can run (AWQ, FP8, NVFP4 checkpoints) before downloading.
- **Platform verification.** 0.1.0 ships the Windows installer (NSIS), signed and notarized macOS DMGs, and a Linux `.deb` package as a preview. A Linux AppImage with the GStreamer media plugins and automatic updates is in main and ships in the next release. Verify Linux on more machines (AMD, NVIDIA, Intel) and distributions, then drop the preview label.
- **First-run polish.** An end-to-end first-run flow (wizard, find a model, download, start, chat) on empty data folders on every platform.
- **Benchmarks on more hardware.** Run the comparison suite (`bench/compare/`) on Linux, and re-measure the other tools in the same session. NVIDIA/CUDA on Windows is done: Harness against plain llama.cpp on an RTX 3050 Ti ([docs/benchmarks/2026-10-15-nvidia](../benchmarks/2026-10-15-nvidia/summary.md)).
- **Accessibility checks.** Small windows, high interface scale, and Windows forced-colors mode.
- **Linux desktop details.** Native `<select>` styling in WebKitGTK, tray icon click behavior, and GPU memory reporting (NVML, fdinfo) on real hardware.

## Next

- **Server mode.** Run Harness on your own (often headless Linux) server and manage everything from the browser:
  - A web interface served by `harness serve`, with password or token sessions.
  - A Server Management area: domain and HTTPS, access and ports, users and tokens, service status, updates, logs, backup and restore.
  - Automatic SSL from the interface (Let's Encrypt, with DNS and port checks).
  - A Linux server package (tar.gz, .deb) with a one-line installer and a systemd service.
  - Docker images for CUDA, ROCm, Vulkan, and CPU.
- **Python engines (optional).** A managed, removable Python environment per engine (uv, hash-pinned locks, nothing system-wide), so every Hugging Face task and library can run: a `python-hf` worker for all transformers pipelines and diffusers text-to-image (a fallback where no GGUF or ONNX build exists), then vLLM and SGLang as OpenAI-compatible servers behind the gateway. `python-hf` is in main, and its speech and image models also answer the OpenAI audio and image routes; diffusers models also edit images (image-to-image and inpainting), with sampler choice and a per-request image size limit; CUDA vLLM moves to the Linux NVIDIA stage, since vLLM does not run natively on Windows. Workers listen on loopback only, load models offline with `trust_remote_code` off by default, and on Linux run without network interfaces (`unshare -rn`; Unix sockets such as D-Bus stay reachable); on Windows and macOS isolation is environment-based (offline Hub, no proxy).
  - CPU vLLM was evaluated and is deferred (2026-10-14, v0.31.0 on a Ryzen 7 4700U, AVX2). The official CPU wheel runs offline in a network namespace, but the environment is about 3 GB, the first request waits about 2.5 minutes while triton-cpu compiles kernels with the system C compiler (gcc is needed at run time), and a 0.5B model makes about 12 tokens per second, well behind llama.cpp on the same laptop. vLLM targets CUDA instead.
- **Harness Link.** Add a remote Harness (address and token) and use its models in your library, Playground, and gateway.
- **Decision models.** `/v1/decisions` and `/v1/systemone` endpoints for yes/no, choice, and scoring questions with constrained output and probabilities.
- **Control API SDKs.** TypeScript and Python clients generated from the OpenAPI schema.
- **MLX engine** for Apple Silicon: chat (mlx-lm), vision chat (mlx-vlm), text to speech (mlx-audio, Qwen3-TTS), and speech recognition (mlx-audio Whisper and Parakeet) are in main and ship in the next release.
- **Auto-update on every platform.** The signed in-app updater ships for Windows in 0.1.0 and for macOS from 0.1.1; Linux packages follow.
- **Signed Windows installer.** A code-signed Windows installer (the macOS app is already signed and notarized).

## Later

- **Tool and MCP registry.** A community index (`struklabs/harness-registry`) of tools and MCP servers: manifests with permissions, publisher, source, version, and SHA-256; CI validation; install into the sandbox with hash verification; publish from inside the app; verified and community labels.
- **ComfyUI bridge** to a ComfyUI instance you already run.
- **Tool router** that selects relevant tools per request when many are installed (using the embedding and rerank stack).
- **More Create pages** for image, code, and writing generation, and for audio runtimes.
- **In-app coding workspace** powered by local models.
- **Undecided:** OpenVINO and MLC.

## Not planned

- Pulling from the Ollama registry (`hf.co/...` references are supported).
- Engines that need Python for normal use when a native engine covers the same models (text-embeddings-inference, faster-whisper). Python engines stay optional add-ons, never required for the core app.
