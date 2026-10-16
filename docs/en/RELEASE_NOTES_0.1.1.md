# Harness Studio 0.1.1

[Türkçe](../tr/RELEASE_NOTES_0.1.1.md)

This release expands local image and speech support and improves everyday reliability and accessibility.

### Highlights

- **New engines:** transcribe.cpp speech recognition; llama.cpp Qwen3-TTS speech with the model's own voice; mflux FLUX.2 [klein] image generation and editing on Apple Silicon; MLX CSM speech with two voices.
- **Harness Link:** connect another Harness instance and use its models in Playground, My Models, and your local gateway.
- **Audio and model coverage:** Process Audio shows speaker diarization as a timeline and table with JSON export. More ONNX vision and NLP models run locally, alongside broader speech and audio processing support.
- **Desktop improvements:** macOS in-app updates; more accessible keyboard and screen-reader controls; readable layouts and reachable buttons at high interface scales and in small windows.
- **First run and stability:** the welcome tour appears after setup, notifications leave wizard buttons clear, long chats open faster, and model startup, downloads, and memory handling are more reliable.
- **Developer access:** the TypeScript SDK is available on npm as `@struklabs/harness-studio-sdk`; SDK packages keep their own version numbers.

### Platforms and Known Limitations

Windows 10/11 x64, macOS 13.3 or later (Apple Silicon and Intel), and Linux x86_64 (preview). Linux downloads include AppImage and `.deb`; broader Linux verification and updater work remain on the [roadmap](ROADMAP.md). The Windows installer is unsigned. ONNX Runtime models are unavailable on Intel Macs. LFM2.5-Audio output still awaits upstream llama.cpp support. Qwen3-TTS on llama.cpp does not clone voices, and its temporary GPU memory use is not yet included in the gateway's model scheduling budget.

Download from [GitHub Releases](https://github.com/struklabs/harness-studio/releases). See the [installation guide](../guide/en/01-installation.md), [data and privacy information](RELEASE_NOTES_0.1.0.md#data-and-privacy), and [full changelog](../CHANGELOG.md).
