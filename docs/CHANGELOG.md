# Changelog

All notable changes to Harness Studio are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

Release notes with system requirements and known limitations (English and Turkish): [RELEASE_NOTES_0.1.0.md](en/RELEASE_NOTES_0.1.0.md).

## [Unreleased]

Planned work is tracked in the [roadmap](en/ROADMAP.md).

### Added
- Text to speech: Meta MMS voices as sherpa-onnx exports them (`vits-mms-<language>` folders: English, German, French, Spanish, Russian, Ukrainian, Thai, Min Nan) are recognized in the model folders. They are not offered for download, since their license is non-commercial.
- Speech to text: NVIDIA Nemotron 3.5 ASR streaming (35 languages including Turkish, with punctuation) and Cohere Transcribe (14 languages) from the sherpa-onnx catalog. Streaming transducer folders are no longer skipped; they run with sherpa-onnx's online decoder.
- Speech to text: speaker diarization for the sherpa-onnx models. `response_format=diarized_json` (or the `speakers` field) on `/v1/audio/transcriptions`, and `harness transcribe --diarize [--speakers N]`, say who spoke each segment (A, B...); SRT and VTT carry the speaker too. Two small models (35 MB) download on first use.
- Playground: audio input for llama.cpp models whose projector (mmproj) accepts audio (for example Ultravox, Qwen2.5-Omni, Voxtral). Attach a WAV, MP3 or FLAC file (up to 4 MB, two per message) or record from the microphone; the clip is sent as an OpenAI `input_audio` part, plays back in the chat and is kept with the conversation. The capability check reports whether a model accepts audio, and Model Hub explains which speech models run.
- ONNX task models without Python: text classification, zero-shot classification, named entity recognition, fill-mask, and question answering. They run through the gateway at `/v1/hf/{model}` in the Hugging Face Inference API format and in the new Playground **Tasks** tab (Advanced). The Model Hub marks these tasks as runnable with ONNX Runtime.
- Video generation through the gateway: OpenAI's videos API (`POST /v1/videos`, `GET /v1/videos`, `GET /v1/videos/{id}`, `GET /v1/videos/{id}/content`, `DELETE /v1/videos/{id}`) on stable-diffusion.cpp's Wan models. A video is a job (it takes minutes): one runs at a time, finished files stay in the data folder for 7 days, and a token sees only its own jobs. The Wan2.1 T2V 1.3B bundle downloads its VAE and text encoder with it. New token endpoint group `videos` (existing restricted tokens do not get it).

### Changed
- Playground: with a model under 5B parameters, tools are offered with short descriptions (first sentence; names, parameters and required fields unchanged). Qwen3-0.6B picked the right tool in 49 of 60 test prompts instead of 36, with about 440 fewer prompt tokens; Qwen3-4B stayed at 60 of 60.

### Fixed
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
