# Harness Studio 0.1.2

[Türkçe](../tr/RELEASE_NOTES_0.1.2.md)

This release makes Playground richer, helps you find models that fit your hardware, and simplifies everyday controls.

### Highlights

- **Generative UI in Playground:** tool results, tables, charts, and simple UI blocks appear as rich components.
- **Model discovery:** Best for My Hardware ranks models by how well they fit your machine and shows a fit badge. Model Hub sorting now matches Hugging Face.
- **Simple and Advanced modes:** Simple shows the essentials with plain labels; Advanced keeps every option in clearer groups.
- **Everyday polish:** improved layouts across Dashboard, Servers, Model Hub, My Library, Settings, Connections, Create, Performance, and Analytics. The section menu stays open after a selection until you choose Collapse.
- **Tool controls:** grant Entire Internet access or restrict tools to allowed domains; local and private network addresses stay blocked. Clear Log clears the Automatic Tool Builder activity log.
- **Safer downloads:** archive extraction rejects symbolic links and paths outside the destination folder and removes privileged permission bits.

### Platforms and Known Limitations

Windows 10/11 x64, macOS 13.3 or later (Apple Silicon and Intel), and Linux x86_64 (preview). Linux downloads include AppImage and `.deb`; broader Linux verification and updater work remain on the [roadmap](ROADMAP.md). The Windows installer is unsigned. ONNX Runtime models are unavailable on Intel Macs. LFM2.5-Audio output still awaits upstream llama.cpp support. Qwen3-TTS on llama.cpp does not clone voices, and its temporary GPU memory use is not yet included in the gateway's model scheduling budget.

Download from [GitHub Releases](https://github.com/struklabs/harness-studio/releases). See the [installation guide](../guide/en/01-installation.md), [data and privacy information](RELEASE_NOTES_0.1.0.md#data-and-privacy), and [full changelog](../CHANGELOG.md).
