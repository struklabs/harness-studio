# Settings

Türkçe: [../tr/08-ayarlar.md](../tr/08-ayarlar.md)

**What you do here:** understand what each settings page is for, choose appearance and startup behavior, and manage what happens when the window is closed to the tray.

## How Settings Work

The **Settings** button in the left menu opens the page. The list on the left is divided into sections and subpages; this guide writes paths as **Settings > Section > Page**. The first page is **Runtime > Installed Runtime**. In the **Simple** interface level only **Installed Runtime**, **Directories**, **Hugging Face Account**, **Appearance & Startup**, **Updates**, and **Privacy** are listed; the rest are Advanced pages (see [Simple and Advanced levels](README.md#simple-and-advanced-interface-levels)).

- On many pages a change is not saved immediately. A bar at the bottom shows "{count} unsaved changes"; save with **Save** or give up with **Discard Changes**.
- Some fields have their own **Save** or **Apply** button (the Hugging Face token and the gateway, for example). Preferences such as language, theme, and interface size apply immediately.
- If you minimize the window to the tray while there are unsaved changes, that input can be lost; see [Leaving the Window Open in the Tray](#leaving-the-window-open-in-the-tray).
- From the command palette (search) you can type a setting's name to go straight to its page.

### Settings Map

| Setting | Path |
| --- | --- |
| Engine (llama.cpp) source, version, update, setup wizard | **Runtime > Installed Runtime** |
| Optional speech, image, Python, MLX, and mistral.rs engines | **Runtime > Installed Runtime > Other Runtimes** |
| Extra llama.cpp builds, ONNX Runtime, Lemonade, GGUF conversion | **Runtime > Registry** |
| Ollama-compatible API | **Runtime > Ollama** |
| Model folders | **Storage > Directories** |
| Hugging Face account and token | **Storage > Hugging Face Account** |
| Defaults for new servers, load timeout, restart after a crash, listening address | **Network > Listening** |
| Access from browsers (CORS) | **Network > Access** |
| Server API keys | **Security > API Keys** |
| Confirmations for opening to the network without a key | **Security > Permissions** |
| Gateway on or off, port, token | **Gateway > Endpoint** |
| Model load and unload rules | **Gateway > Routing** |
| The Responses API's response storage | **Gateway > Responses** |
| Tool permission policy, working folder, web search | **Tools > Built-In Tools** |
| Installed tool packages | **Tools > Installed Tools** |
| Control API and named tokens | **Developer > Control API** |
| Command-line tool (CLI) | **Developer > CLI** |
| Metric and trace export | **OpenTelemetry** |
| Theme, language, interface level, startup, tray | **Appearance & Startup** |
| App version and update check | **Updates** |
| What local diagnostic reports contain | **Privacy** |
| Setup check, reports | **Diagnostics > Health**, **Diagnostics > Reports** |

## Runtime

**Settings > Runtime > Installed Runtime** edits the engine source.

- With **Managed (recommended)** selected you see **Installed version**, **Check for Updates**, and **Update automatically**. Automatic updating downloads and verifies a new version in the background and activates it when servers are stopped. You can also download and test a new version by hand, and the old version keeps running until you press **Activate New Version**. If an update causes trouble and a backup exists, you can use **Roll back to previous version**.
- With **Custom binary** selected, enter the **llama-server path** or look for it with **Auto-Detect**, and try it with **Verify**. A custom binary is never updated or rolled back automatically.
- The **Backend** row shows which build (CUDA, Vulkan, ROCm, Metal, CPU) is installed and recommended for your hardware; if the installed build cannot see your graphics card you can install another one.
- **Setup wizard > Open Wizard** reruns the first-launch wizard.

**Other Runtimes** on **Settings > Runtime > Installed Runtime** manages whisper.cpp, sherpa-onnx, stable-diffusion.cpp, mistral.rs, Python (`python-hf`, CPU/CUDA), MLX (chat, vision, audio; Apple Silicon, macOS 14+), CrispASR, and experimental audio.cpp (off by default; risk confirmation required).

**Registry** (an Advanced page) manages additional llama.cpp builds, ONNX Runtime, Lemonade, and the GGUF conversion environment. Each model definition or server profile can choose its own llama.cpp runtime. The engines are split between these pages:

| Card (page) | What it is for |
| --- | --- |
| Speech Recognition (whisper.cpp) (Installed Runtime > Other Runtimes) | `whisper-server`, which turns audio files into text. |
| Text to Speech (sherpa-onnx) (Installed Runtime > Other Runtimes) | Kokoro and Piper voice models; sherpa-onnx also runs the Parakeet, SenseVoice, and Omnilingual recognition models. |
| Image Generation (stable-diffusion.cpp) (Installed Runtime > Other Runtimes) | `sd-server`, which makes pictures from diffusion models (SD1.x, SDXL, SD3.5, FLUX, Qwen-Image). |
| ONNX Runtime (embeddings, rerank, chat) (Registry) | The libraries used by the `harness-onnx` helper to run Hugging Face ONNX embedding, reranking, and chat models without Python. You can install a **DirectML** (Windows: AMD and NVIDIA), **CUDA**, or **CPU** variant; downloads are verified with SHA-256. ONNX chat models (onnxruntime-genai) work on Windows only for now and have no tool calling. |
| Lemonade (Ryzen AI NPU) (Registry) | Optional. Runs models on AMD Ryzen AI NPUs (FastFlowLM) and with Lemonade's own ROCm and Vulkan llama.cpp builds; the gateway serves its chat, embedding, rerank, and speech-to-text models. |
| Safetensors Models (mistral.rs) (Installed Runtime > Other Runtimes) | Optional. Runs Hugging Face safetensors chat and vision models without converting them to GGUF. You add your own mistral.rs build. |
| GGUF Conversion Environment (Registry) | The Python environment used when you convert safetensors models to GGUF; install or delete it here. |

The **Ollama** page controls how Ollama clients can connect to the gateway as if it were Ollama ([Connections](05-connections-and-gateway.md)).

## Storage

**Directories** manages the primary model folder (where new downloads go) and extra folders; the details are on the [Installation](01-installation.md#model-folder-and-data-folder) page. **Hugging Face Account** is for an access token or browser sign-in; the steps are on the [Model Hub page](02-model-hub-and-library.md#hugging-face-account-two-ways).

## Network

The **Network > Listening** page has three groups:

| Group | Fields | Meaning |
| --- | --- | --- |
| **Server Defaults** | **Port**, **GPU layers (-ngl)**, **Context size (-c)** | Starting values for newly created servers. Port `0` (Automatic) picks a free port from 11440 to 11499. A server that was started before uses its own saved values. |
| **Server Start** | **Load timeout (s)**, **Restart automatically after a crash**, **Maximum attempts** | If a server is not ready within this time, the start ends with an error (increase it for large models). A server that stops unexpectedly while running is restarted with waits of 2, 4, 8... seconds; load errors (memory, a damaged file, a port) are not retried. |
| **Default Bind Address** | **This computer only**, **Local network**, **Custom address** | `127.0.0.1` is open only to this computer (recommended). **Local network** (`0.0.0.0`) lets devices on the same network connect and requires an API key; if you want to open it without a key the app asks for an **Open Anyway** confirmation (these confirmations are listed on **Security > Permissions**). |

**Network > Access** decides which web apps running in a browser can reach the servers (**Allowed Browser Origins (CORS)**). Desktop clients and command-line tools are not affected. Harness Studio does not change your firewall by itself; Windows Firewall may ask for permission on first start, and allowing only "Private networks" is recommended.

## Security

The **API Keys** page shows each server's key status (**Key set** / **No key**) and has **Generate Strong Key**, **Generate New Key**, and **Copy Key** buttons. Keys are kept in the operating system keychain and are not written to files. A new key takes effect when the server restarts, and clients using the old key need updating too. The **Permissions** page lists the **Open Anyway** confirmations you gave for opening servers to the network without a key and lets you take them back. The detailed conditions are in the [security model](../../en/SECURITY_MODEL.md).

## Gateway

**Endpoint** turns the gateway on and edits its address, **Routing** has the model load, unload, and queue rules, and **Responses** has the storage options of the OpenAI Responses API. The gateway is off by default and stops when the app closes. See the [Connections chapter](05-connections-and-gateway.md) for details and for connecting clients.

## Tools

**Built-In Tools** holds the permission policy, trusted tools, the default working folder, the web search provider, and the **Offer tools to MCP clients** option. **Installed Tools** manages ready-made or generated packages. Details are in the [Tools chapter](06-tools.md).

## Developer

**Control API** has the API that lets programs manage Harness Studio, and named tokens; you do not need to enable it for everyday chat. The **Use in a Terminal** area on the **CLI** page shows the command-line tool; **Add to PATH** makes it reachable from a terminal. Terminals that are already open do not see the change; open a new one. See [CLI, Control API, and MCP](09-cli-api-mcp.md) for details.

## OpenTelemetry

**OpenTelemetry** is for exporting metrics and traces to a collector you choose. It is meant only for people who run observability infrastructure, and it is off by default. Unless you choose **Turn on export**, the app does not connect to a collector or send data on its own. The one exception is the **Test Export** button: it sends a real test span to the address in the form even when export is off. When export is on, data goes to the collector address you entered. If the build has no gRPC support, choose the **HTTP/protobuf (port 4318)** protocol.

## Appearance & Startup

| Setting | What it does |
| --- | --- |
| Theme | **Dark**, **Light**, **System**, **OLED Black**, **Night**, **High Contrast Dark**, **High Contrast Light**. |
| Language | Turkish or English; the change applies immediately. |
| **Interface level** | **Simple** or **Advanced**; see [Simple and Advanced levels](README.md#simple-and-advanced-interface-levels). It changes only what is visible, never a saved value. |
| **Interface size** | Scales text, icons, and spacing together. Shortcuts: Ctrl+= to enlarge, Ctrl+- to shrink, Ctrl+0 to reset. |
| **Colour-blind friendly colours** | Replaces status and chart colors with colors that do not depend on telling red from green. |
| **Start when you sign in** | Harness Studio starts when the computer starts. It takes effect immediately. |
| **Start minimized** | On an automatic start no window opens and the app waits in the tray. Available only while **Start when you sign in** is on. |
| **Restore last session** | On launch, the server you last started starts again. |
| **When closing the window** | **Ask**, **Minimize to Tray**, or **Quit**. On macOS the equivalent of tray is **Keep in the menu bar**. |
| **Keep the window loaded in the tray** | Whether the page stays in memory while in the tray; explained below. |

## Updates

The **Updates** page shows the app version and has an **Application** section with **Check for Updates**. When a newer version is published, you can use **Install Update**; the app downloads and installs it, then restarts, and all running servers are stopped. If the check reports that no update source is available, download the newest installer from the project's [GitHub Releases](https://github.com/struklabs/harness-studio/releases) page and run it. The llama.cpp engine is updated separately on **Runtime > Installed Runtime**.

## Privacy

**Privacy** chooses what local diagnostic reports contain: **Crash reports**, **App errors**, **Server errors (llama.cpp)**, **Hardware info**, and **Performance metrics**. Reports are kept only on this device and are never sent anywhere automatically. Prompts, model responses, file contents, and API request bodies do not go into these reports. Chat history and the optional analytics content capture are separate functions; also read the plain-text content capture explanation in the [analytics section](07-performance-and-analytics.md#analytics-and-the-request-log).

## Diagnostics

On **Diagnostics > Health**, **Run Checks** checks your setup, runtime compatibility, GPU libraries, and ports; results are marked **OK**, **Warning**, or **Error**, and **Copy JSON** adds them to a report. **Diagnostics > Reports** is where you review (**Review**), copy, and delete (**Delete Reports**) local diagnostic records. For a problem, first read the explanation of the failed check, then continue with the [troubleshooting chapter](10-troubleshooting.md).

## Leaving the Window Open in the Tray

If **When closing the window** is set to **Minimize to Tray**, closing the window does not close the app: it keeps running in the system tray (the menu bar on macOS), and servers and downloads continue. Reopen the window from the tray icon; to quit completely, use **Quit** in the icon's menu. If **Quit** is selected, running servers are stopped. If your desktop has no tray or menu bar icon, closing the window quits the app. On Linux this includes stock GNOME, which shows tray icons only with the AppIndicator extension.

To save memory while in the tray, the window's web page can be closed:

1. In **Settings > Appearance & Startup**, set **When closing the window** to **Minimize to Tray**.
2. Close the window. Servers and downloads keep running in the app process.
3. If **Keep the window loaded in the tray** is off (the default), the interface page is closed after about 30 seconds in the tray and only the app process remains; this saves about 60 MiB in the tray. When you open it from the tray icon the page is rebuilt (about half a second) and your open tab comes back.
4. The page is not closed, even in the tray, when: an answer is streaming or a tool call is running in the Playground, a download is running, a tool approval is waiting, there is a message you typed but did not send, there is an unsaved server setting on the Dashboard or an unsaved change in Settings, or the tool creator or an audio recording is running.
5. If you want instant readiness, turn **Keep the window loaded in the tray** on. It uses more memory but opening is immediate.

Input that falls outside these protections (for example, text typed into a search box but not searched) can be lost; save your work before you close the window. On macOS the preference appears as **Keep the window loaded in the menu bar**.

Next: [CLI, Control API, and MCP](09-cli-api-mcp.md).
