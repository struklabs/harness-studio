# Roadmap

This is the public roadmap of Harness Studio. It shows direction, not promises: priorities shift with feedback. To suggest or discuss something, open a thread in [GitHub Discussions](https://github.com/struklabs/harness-studio/discussions) or an [issue](https://github.com/struklabs/harness-studio/issues). Shipped changes are in the [changelog](../CHANGELOG.md); how Harness compares with similar tools is in [FEATURES.md](FEATURES.md) and [COMPARISON.md](COMPARISON.md).

**Product goal.** A local AI control center that manages text, image, speech, embedding, reranking, and tool workloads in one place. llama.cpp is the default runtime; others are modules you install when you need them.

**Principles.**
- Do what LM Studio and Ollama do, without hiding llama.cpp's advanced controls.
- Be better at observability and at agent use, with low resource use.
- Local first, cloud optional.
- The app, the CLI, the Control API, and MCP all use the same Rust services.

## Now

- **Platform verification.** 0.1.0 ships the Windows installer (NSIS), signed and notarized macOS DMGs, and a Linux `.deb` package as a preview (AppImage next, once it bundles the GStreamer media plugins). Verify Linux on more machines (AMD, NVIDIA, Intel) and distributions, then drop the preview label.
- **First-run polish.** An end-to-end first-run flow (wizard, find a model, download, start, chat) on empty data folders on every platform.
- **Benchmarks on more hardware.** Run the comparison suite (`bench/compare/`) on NVIDIA/CUDA and on Linux, and re-measure the other tools in the same session.
- **Accessibility checks.** Small windows, high interface scale, and Windows forced-colors mode.
- **Linux desktop details.** Native `<select>` styling in WebKitGTK, tray icon click behavior, and GPU memory reporting (NVML, fdinfo) on real hardware.

## Next

- **Server mode.** Run Harness on your own (often headless Linux) server and manage everything from the browser:
  - A web interface served by `harness serve`, with password or token sessions.
  - A Server Management area: domain and HTTPS, access and ports, users and tokens, service status, updates, logs, backup and restore.
  - Automatic SSL from the interface (Let's Encrypt, with DNS and port checks).
  - A Linux server package (tar.gz, .deb) with a one-line installer and a systemd service.
  - Docker images for CUDA, ROCm, Vulkan, and CPU.
- **Harness Link.** Add a remote Harness (address and token) and use its models in your library, Playground, and gateway.
- **Decision models.** `/v1/decisions` and `/v1/systemone` endpoints for yes/no, choice, and scoring questions with constrained output and probabilities.
- **Control API SDKs.** TypeScript and Python clients generated from the OpenAPI schema.
- **MLX engine** for Apple Silicon.
- **Auto-update.** A signed update channel for the desktop app through GitHub Releases.
- **Signed Windows installer.** A code-signed Windows installer (the macOS app is already signed and notarized).

## Later

- **Tool and MCP registry.** A community index (`struklabs/harness-registry`) of tools and MCP servers: manifests with permissions, publisher, source, version, and SHA-256; CI validation; install into the sandbox with hash verification; publish from inside the app; verified and community labels.
- **ComfyUI bridge** to a ComfyUI instance you already run.
- **Tool router** that selects relevant tools per request when many are installed (using the embedding and rerank stack).
- **More Create pages** for image, code, and writing generation, and for audio runtimes.
- **In-app coding workspace** powered by local models.
- **Undecided:** a PyTorch worker for research models (3D, music), OpenVINO, and MLC. Network isolation for Python subprocesses would be designed first.

## Not planned

- Pulling from the Ollama registry (`hf.co/...` references are supported).
- Engines that do not run on Windows or that need Python at runtime for normal use (vLLM, SGLang, text-embeddings-inference, faster-whisper).
