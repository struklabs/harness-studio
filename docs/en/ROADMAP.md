# Roadmap

[Türkçe](../tr/ROADMAP.md)

Harness Studio manages local text, image, speech, and tool workloads in one place. Local first, with optional engines and cloud connections. This roadmap shows direction; priorities may change with feedback. Share suggestions in [Discussions](https://github.com/struklabs/harness-studio/discussions) or [issues](https://github.com/struklabs/harness-studio/issues). See the [changelog](../CHANGELOG.md) for details.

## Shipped in 0.1.2

- **Playground:** generative UI for tool results, tables, charts, and simple UI blocks.
- **Model discovery:** Best for My Hardware ranking and fit badges; sorting that matches Hugging Face.
- **Everyday controls:** Simple and Advanced modes, polished page layouts, a section menu that stays open, Entire Internet tool permissions, and Clear Log in Automatic Tool Builder.
- **Security:** archive extraction rejects symbolic links and paths outside the destination and removes privileged permission bits.

## Shipped in 0.1.1

- **Engines:** transcribe.cpp, llama.cpp Qwen3-TTS, mflux images on Apple Silicon, and broader MLX speech support including CSM.
- **Model coverage:** more ONNX vision, NLP, and audio tasks, image editing, video generation, and audio processing with speaker diarization.
- **Connections:** Harness Link brings remote Harness models to Playground and the local gateway.
- **Desktop experience:** accessibility and high interface scale fixes, first-run improvements, and more responsive long chats and model libraries.
- **Platforms and stability:** macOS updates, Linux AppImage support, and fixes for memory pressure, downloads, and model startup.
- **SDKs:** TypeScript SDK published on npm; TypeScript and Python clients checked against the Control API in CI.

## Next

- Verify Linux on more AMD and NVIDIA machines and distributions, then drop the preview label.
- Complete Linux first-run and accessibility checks.
- Run the benchmark comparison on Linux.
- Complete Linux updater packages and verification.
- Add CUDA vLLM after Linux NVIDIA verification.
- Publish the Python SDK on PyPI through Trusted Publishing.
- Sign the Windows installer after the website is ready.
- Enable LFM2.5-Audio audio output when upstream llama.cpp supports it.

## Planned for 0.2.0

- **Server mode:** browser management through `harness serve`, sessions, HTTPS, users and tokens, updates, logs, backup and restore; Linux service packages and Docker images.
- **Tool, MCP, and Skills Registry:** a community catalog with permission manifests, publisher and version information, hash verification, CI validation, and in-app publishing.

## Later

- A ComfyUI bridge and a tool router that selects relevant tools per request.
- More Create pages and an in-app coding workspace powered by local models.
- Under consideration: faster-whisper after an NVIDIA benchmark; VieNeu when its SEA-G2P dependency is practical.
- Undecided: OpenVINO and MLC.

## Not Planned

- Pulling from the Ollama registry (`hf.co/...` references are supported).
- Requiring Python for the core app or duplicating native model coverage with Python engines; optional engines remain add-ons.
