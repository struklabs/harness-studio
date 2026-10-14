# Changelog

All notable changes to Harness Studio are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

Release notes with system requirements and known limitations (English and Turkish): [RELEASE_NOTES_0.1.0.md](en/RELEASE_NOTES_0.1.0.md).

## [Unreleased]

- audio.cpp TTS is experimental and off by default. Settings > Installed Runtime > Other Runtimes requires risk confirmation: its local server has no authentication or Host/Origin checks and accepts server-side file paths. The gateway forwards only validated speech requests with discovered voices and inline base64 references up to 5 MiB. Workers stop after five idle minutes.

Planned work is tracked in the [roadmap](en/ROADMAP.md).

### Added
- Python engine image generation (diffusers): image-to-image and inpainting on the same text-to-image model through `/v1/images/edits` (OpenAI's multipart form or JSON; an OpenAI mask's transparent areas are repainted), with `strength`. Requests can pick the sampler (`euler`, `euler_a`, `heun`, `dpm2`, `dpm++2m`, `ddim`, `pndm`, `lms`, `unipc`, `lcm`, with `scheduler: karras` where it applies) and use the gateway's `steps` / `cfg_scale`. Two new model settings: **Largest image** (megapixels per request, 1 on the CPU and 4 on a GPU by default) and **Low memory** (sliced attention and VAE, on for the CPU by default). These models now open in the Playground Image tab, start image included.
- Managed audio.cpp native text-to-speech engine: platform-aware release installation, resident model servers on loopback, gateway `/v1/audio/speech`, and Playground voice selection from companion embeddings. Audio.cpp GGUF metadata is kept out of llama.cpp routing; Hub package variants retain each model folder's companions. Non-TTS audio.cpp families are detected but not supported yet.
- Zero-shot object detection on the ONNX engine: OWL-ViT and OWLv2 exports find the labels a request names and return boxes on `/v1/hf/{model}`. OWLv2's square padding is applied. The Playground Tasks tab draws the boxes, and the Hub lists the task.
- Image captioning on the ONNX engine: ViT + GPT-2 style encoder-decoder exports answer `image-to-text` on `/v1/hf/{model}`. Greedy decoding uses the KV cache, and `max_new_tokens` is capped at 256. The downloader keeps each encoder with its decoder, and the Playground Tasks tab shows the caption.
- Image feature extraction on the ONNX engine: bare image backbones (DINOv2, ViT, ConvNeXt, Swin and others) answer `image-feature-extraction` on `/v1/hf/{model}` with their hidden states, or with the pooled vector when `pool` is set. The Playground Tasks tab shows the result, and the Hub marks the task as runnable.
- Audio classification on the ONNX engine: AST (AudioSet) and wav2vec2 / HuBERT / WavLM classifier exports answer `audio-classification` on `/v1/hf/{model}` with ranked labels, with no Python. The AST filter banks are computed in Rust. The Playground Tasks tab takes an audio file (WAV, MP3, FLAC, OGG), and the Hub marks the task as runnable.
- MLX audio engine (mlx-audio) for Apple Silicon (macOS 14+): Qwen3-TTS text-to-speech models from mlx-community speak through the gateway's /v1/audio/speech, with their preset voices and speed. OpenAI voice names get the default voice. It shares the MLX vision engine's environment, and the Model Hub marks those repos "Speaks with MLX". There is no voice cloning, and the worker never reads a file path from a request.
- MLX vision engine (mlx-vlm) for Apple Silicon (macOS 14+): chat with images using MLX-format vision-language models from mlx-community (Qwen3-VL, Gemma 3 and others) through the gateway and the Playground. It installs from Settings > Installed Runtime > Other Runtimes as its own managed Python environment (about 760 MB); the Model Hub marks those repos "Runs with MLX (vision)". Images are accepted only inline (data: URLs). The worker never fetches a link or reads a file path a request names; it listens on 127.0.0.1 only and asks for each run's key.
- Text to speech: Meta MMS voices as sherpa-onnx exports them (`vits-mms-<language>` folders: English, German, French, Spanish, Russian, Ukrainian, Thai, Min Nan) are recognized in the model folders. They are not offered for download, since their license is non-commercial.
- MLX engine for Apple Silicon (macOS 14+): MLX-format chat models from mlx-community, including ones with no GGUF, run through the gateway with chat, streaming and tool calls. It installs from Settings > Installed Runtime > Other Runtimes as a managed Python environment (about 330 MB); the Model Hub marks those repos "Runs with MLX". The worker listens on 127.0.0.1 only and asks for each run's key; macOS has no network isolation for it.
- ONNX task models without Python: text classification, zero-shot classification, named entity recognition, fill-mask, and question answering. They run through the gateway at `/v1/hf/{model}` in the Hugging Face Inference API format and in the new Playground **Tasks** tab (Advanced). The Model Hub marks these tasks as runnable with ONNX Runtime.
- Speech to text: NVIDIA Nemotron 3.5 ASR streaming (35 languages including Turkish, with punctuation) and Cohere Transcribe (14 languages) from the sherpa-onnx catalog. Streaming transducer folders are no longer skipped; they run with sherpa-onnx's online decoder.
- Speech to text: speaker diarization for the sherpa-onnx models. `response_format=diarized_json` (or the `speakers` field) on `/v1/audio/transcriptions`, and `harness transcribe --diarize [--speakers N]`, say who spoke each segment (A, B...); SRT and VTT carry the speaker too. Two small models (35 MB) download on first use.
- Playground: audio input for llama.cpp models whose projector (mmproj) accepts audio (for example Ultravox, Qwen2.5-Omni, Voxtral). Attach a WAV, MP3 or FLAC file (up to 4 MB, two per message) or record from the microphone; the clip is sent as an OpenAI `input_audio` part, plays back in the chat and is kept with the conversation. The capability check reports whether a model accepts audio, and Model Hub explains which speech models run.
- Video generation through the gateway: OpenAI's videos API (`POST /v1/videos`, `GET /v1/videos`, `GET /v1/videos/{id}`, `GET /v1/videos/{id}/content`, `DELETE /v1/videos/{id}`) on stable-diffusion.cpp's Wan models. A video is a job (it takes minutes): one runs at a time, finished files stay in the data folder for 7 days, and a token sees only its own jobs. The Wan2.1 T2V 1.3B bundle downloads its VAE and text encoder with it. New token endpoint group `videos` (existing restricted tokens do not get it).
- ONNX vision models: image classification, object detection, semantic segmentation, and depth estimation (ViT, DETR, SegFormer, Depth Anything and similar transformers.js exports). The Playground **Tasks** tab takes an image, draws the detected boxes, overlays the masks, and shows the depth map.
- ONNX zero-shot image classification: CLIP and SigLIP score an image against labels you name (`candidate_labels`), through `/v1/hf/{model}` and the Playground **Tasks** tab. The Model Hub offers the joint graph of these repos, not their single towers.
- Python engine (`python-hf`, optional): Hugging Face task models with no GGUF or ONNX build (classification, NER, question answering, summarization, translation, image classification, detection, segmentation, depth, captioning, visual question answering, speech recognition, audio classification, text to speech, and diffusers text to image) run on transformers from a managed CPU environment you install in one click (Settings > Installed Runtime > Other Runtimes, about 1.1 GB). They are listed in the library with their task, run through the gateway at `/v1/hf/{model}`, `/v1/embeddings` and `/v1/rerank`, and the Model Hub marks such repositories as runnable. Remote code is never run, and on Linux the model runs without network access.
- Gateway: `/v1/audio/transcriptions` (and `/v1/audio/translations`), `/v1/audio/speech` and `/v1/images/generations` also serve Python engine models (speech recognition, text to speech, diffusers text to image). whisper.cpp, sherpa-onnx and stable-diffusion.cpp models keep their own engines; a sherpa-onnx voice wins over a Python model of the same name.
- Speech recognition with Hugging Face CTC models (wav2vec2, HuBERT, WavLM, Data2Vec Audio, UniSpeech) exported to ONNX, for example `Xenova/wav2vec2-base-960h`. Model Hub recognizes them and offers the FP32 and quantized exports. They run in the ONNX sidecar (DirectML, CUDA or CPU) behind the gateway's `/v1/audio/transcriptions`, in Playground > Speech, in chat dictation and in `harness transcribe`. Long recordings are split into 20-second pieces with overlap, and segment times break at words or pauses. Source-only speech repositories show "Needs Conversion".

### Changed
- Playground: with a model under 5B parameters, tools are offered with short descriptions (first sentence; names, parameters and required fields unchanged). Qwen3-0.6B picked the right tool in 49 of 60 test prompts instead of 36, with about 440 fewer prompt tokens; Qwen3-4B stayed at 60 of 60.

### Fixed
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
