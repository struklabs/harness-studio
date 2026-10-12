# Harness Studio 0.1.0

[Türkçe](../tr/RELEASE_NOTES_0.1.0.md)

Harness Studio is a control center for finding, running, and connecting local AI models to the applications you use.

### Highlights

- Start and stop local model servers; manage multiple servers and saved launch settings.
- Discover Hugging Face models in **Model Hub**, pause and resume downloads; organize models in **My Models** and import from Ollama/LM Studio.
- Chat in **Playground**, compare two models, try embeddings and reranking, and transcribe speech with whisper.cpp.
- Search PDF, DOCX, text, and code documents locally and get answers with citations; summarize long conversations with the option to undo.
- Access multiple models at one address through the optional **gateway**; connect OpenAI-, Anthropic-, and Ollama-compatible clients.
- Use built-in file, search, and other tools with permission checks; connect MCP servers and create your own tools.
- Connect **Claude Code, Codex, OpenCode**, and other clients; preview persistent configuration changes before applying them.
- Inspect speed, token usage, and errors with performance measurements, benchmarks, **analytics, and request logs**.
- Manage models and servers through the **CLI and Control API**; run without the interface and access Harness through MCP.
- See memory estimates and recovery suggestions when memory runs out; the gateway can unload idle models when needed.
- Use the **Turkish/English interface**, light/dark/system themes, high contrast, and interface scaling.

### System requirements

| Platform | Hardware and packages |
| --- | --- |
| Windows 10/11 x64 | Vulkan on AMD GPUs; ROCm/HIP on compatible hardware with the required HIP libraries. CUDA on NVIDIA GPUs. CPU use without a GPU. NSIS installer (GitHub Releases). |
| macOS, Linux | Coming soon. Builds exist but have not yet been verified on real machines, so they are not offered as downloads in this release. You can build from source (see the README). |

GPU acceleration depends on hardware, drivers, and the selected model runner. Choose a model that fits available RAM/GPU memory and allow enough disk space for model files.

### Known limitations

- **The Windows installer is unsigned.** If SmartScreen warns you, select **More info > Run anyway**.
- **Automatic app updates:** the update check reads the signed `latest.json` published with GitHub Releases. There is no update to find in the first release; you can also download new versions from the [Releases](https://github.com/struklabs/harness-studio/releases) page.
- **Windows uninstall:** the remaining entry under `HKCU\Software\harnessstudio` only remembers the installation directory; it is harmless and can be deleted if desired. User data in `%APPDATA%\com.harnessstudio.app` is preserved when uninstalling.
- **Going back to an older version:** close Harness Studio and, before installing the older version, copy `config/settings.json.bak-v2` over `config/settings.json` in the data folder. This discards settings changed since the move to the newer settings format.

### Data and privacy

Settings, conversations, document search records, and logs are stored locally. Default data folders:

| Platform | Location |
| --- | --- |
| Windows | `%APPDATA%\com.harnessstudio.app` |
| macOS | `~/Library/Application Support/com.harnessstudio.app` |
| Linux | `$XDG_DATA_HOME/com.harnessstudio.app`, or `~/.local/share/com.harnessstudio.app` when unset |

- Servers are accessible only through `127.0.0.1` by default. The desktop gateway and Control API start disabled; network access must be enabled separately.
- No automatic telemetry or crash reports are sent. Model discovery/downloads contact Hugging Face; model runner downloads/updates contact GitHub. Web, weather, and remote MCP tools contact their respective services when used. **OpenTelemetry export is optional.**
- Hugging Face tokens, model server API keys, and search provider keys use the operating system keychain. Third-party client settings and backups may contain plaintext keys; review the preview.
- Local databases are not encrypted. Request analytics record usage information rather than content by default; conversations and stored responses contain content locally.
