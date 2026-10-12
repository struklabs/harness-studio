# Changelog

All notable changes to Harness Studio are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

Release notes with system requirements and known limitations (English and Turkish): [RELEASE_NOTES_0.1.0.md](en/RELEASE_NOTES_0.1.0.md).

## [Unreleased]

Planned work is tracked in the [roadmap](en/ROADMAP.md).

- Fixed: a tool call right after an MCP server crashed could fail instead of restarting the server; the call now reconnects and is retried once.

## [0.1.0] - 2026-10-12

First public release. Windows installer (NSIS) on GitHub Releases; Linux and macOS builds are coming soon.

### Model discovery and library
- Model Hub: Hugging Face search with filters, model cards, quantization variants, and a "fits your hardware" indication.
- Resumable download queue with pause and resume, SHA-256 verification, split (multi-file) models, and an update badge for models you already have.
- Library: scan several model folders, import from Ollama and LM Studio, detect duplicates and missing files, and save model definitions (aliases, load settings, presets).
- Convert safetensors repositories to GGUF from the Hub (managed, temporary Python environment; quantization with `llama-quantize`).
- Optional engines (whisper.cpp, stable-diffusion.cpp, sherpa-onnx, ONNX Runtime, Lemonade) install with one click right where a model needs them (library card, Model Hub, Playground errors), and are listed with their install state under Settings > Runtime > Installed Runtime at both interface levels.
- Sign in to Hugging Face with OAuth or a pasted token; tokens are kept in the operating system keychain.

### Running models
- Dashboard with several simultaneous servers, saved launch profiles, automatic ports, and live telemetry (slots, throughput, KV cache, memory).
- Memory estimates before loading, automatic GPU layer count, automatic KV cache precision, and a fit check against free VRAM and shared memory.
- Clear diagnosis and recovery suggestions for failed loads (out of memory, missing libraries, port conflicts).
- Parameter catalog gated by the selected runtime build; Simple and Advanced interface levels.
- Multi-LoRA adapters with live scaling, chat template editor, RoPE/YaRN options, speculative decoding, and router mode.
- Managed llama.cpp runtime downloads with integrity checks, background updates, and support for extra runtimes (forks, local folders). Backends: CUDA, ROCm/HIP, SYCL, Vulkan, CPU, and Metal.
- Other engines: whisper.cpp and sherpa-onnx (speech to text, voice activity detection, speaker separation, text to speech), stable-diffusion.cpp (image generation), ONNX Runtime (embeddings, reranking, ONNX chat), and optional Lemonade and mistral.rs.

### Playground
- Streaming chat with saved conversations, branching and regeneration from any message, model comparison, Markdown and Mermaid rendering.
- Image input for vision models, audio dictation, read-aloud, and image generation.
- Local document search (PDF, DOCX, text, code) with cited answers.
- Reversible context compaction for long conversations.
- Inline load settings, token usage display, and optional token probabilities (logprobs).

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
- `harness` command: `serve`, `search`, `pull`, `load`, `run`, `ps`, `launch`, `runtime`, `doctor`, and more, with `--json` output and a headless mode that needs no window.

### Observability and benchmarks
- Request analytics (latency percentiles, tokens, errors), searchable logs, and benchmark runs with comparison.
- Measured comparison against LM Studio, Ollama, and llama.cpp: [COMPARISON.md](en/COMPARISON.md) and [benchmarks](benchmarks/).

### Interface
- English and Turkish, light, dark, and system themes, high contrast, color-vision-friendly palette, interface scaling.
- System tray, launch at login, and background mode.

### Known limitations
- The Windows installer is unsigned; SmartScreen may show a warning (**More info > Run anyway**).
- Linux and macOS builds are not yet verified on real machines and are not offered as downloads.

[Unreleased]: https://github.com/struklabs/harness-studio/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/struklabs/harness-studio/releases/tag/v0.1.0
