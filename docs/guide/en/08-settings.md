# Settings

Türkçe: [../tr/08-ayarlar.md](../tr/08-ayarlar.md)

**What you do here:** understand what each settings page is for, choose appearance and startup behavior, and manage what happens when the window is closed to the tray.

## How Settings Work

The **Settings** button in the left menu opens the page. The list on the left is divided into sections and subpages; this guide writes paths as **Settings > Section > Page**. The first page is **Runtime > Installed Runtime**. In the **Simple** interface level only **Installed Runtime**, **Directories**, **Hugging Face Account**, **Appearance & Startup**, **Updates**, and **Privacy** are listed; the rest are Advanced pages (see [Simple and Advanced levels](README.md#simple-and-advanced-interface-levels)).

- On many pages a change is not saved immediately. A bar at the bottom shows "{count} unsaved changes"; save with **Save** or give up with **Discard Changes**.
- Some fields have their own **Save** or **Apply** button (the Hugging Face token and the gateway, for example). Preferences such as language, theme, and interface size apply immediately.
- If you minimize the window to the tray while there are unsaved changes, that input can be lost; see [Leaving the Window Open in the Tray](#leaving-the-window-open-in-the-tray).
- From the command palette (search) you can type a setting's name to go straight to its page.

The floating section menu stays open after selecting a page. Use **Collapse** or Escape to close it. Hidden Advanced pages remain reachable through the command palette and in-app links.

### Settings Map

| Setting | Path |
| --- | --- |
| Engine (llama.cpp) source, version, update, Setup | **Runtime > Installed Runtime** |
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
| Connection to your own n8n | **Automations** |
| Theme, language, interface level, startup, tray | **Appearance & Startup** |
| App version and update check | **Updates** |
| What local diagnostic reports contain | **Privacy** |
| Setup check, reports | **Diagnostics > Health**, **Diagnostics > Reports** |

## Runtime

**Settings > Runtime > Installed Runtime** edits the engine source.

- With **Managed (recommended)** selected you see **Installed Version**, **Check for Updates**, and **Update Automatically**. Automatic updating downloads and verifies a new version in the background and activates it when servers are stopped. You can also download and test a new version by hand, and the old version keeps running until you press **Activate New Version**. If an update causes trouble and a backup exists, you can use **Roll Back to Previous Version**.
- With **Custom Binary** selected, enter the **Llama-server Path** or look for it with **Auto-Detect**, and try it with **Verify**. A custom binary is never updated or rolled back automatically.
- The **Backend** row shows which build (CUDA, Vulkan, ROCm, Metal, CPU) is installed and recommended for your hardware; if the installed build cannot see your graphics card you can install another one.
- **Setup > Run Setup Again** reopens the first-launch Setup window.

**Other Runtimes** on **Settings > Runtime > Installed Runtime** manages whisper.cpp, sherpa-onnx, stable-diffusion.cpp, mistral.rs, Python (`python-hf`, CPU/CUDA), MLX (chat, vision, audio; Apple Silicon, macOS 14+), CrispASR, and experimental audio.cpp (off by default; risk confirmation required). On Windows, Python environment installation checks the projected package paths before downloading. If the data folder would exceed `MAX_PATH` and Windows long paths are disabled or their status is unknown, installation is refused; choose a shorter data folder or enable Windows long paths.

**Registry** (an Advanced page) manages additional llama.cpp builds, ONNX Runtime, Lemonade, and the GGUF conversion environment. Each model definition or server profile can choose its own llama.cpp runtime. The engines are split between these pages:

| Card (page) | What it is for |
| --- | --- |
| Speech Recognition (whisper.cpp) (Installed Runtime > Other Runtimes) | `whisper-server`, which turns audio files into text. |
| Text to Speech (sherpa-onnx) (Installed Runtime > Other Runtimes) | Kokoro and Piper voice models; sherpa-onnx also runs the Parakeet, SenseVoice, and Omnilingual recognition models. |
| Image Generation (stable-diffusion.cpp) (Installed Runtime > Other Runtimes) | `sd-server`, which makes pictures from diffusion models (SD1.x, SDXL, SD3.5, FLUX, Qwen-Image). |
| ONNX Runtime (embeddings, rerank, chat) (Registry) | The libraries used by the `harness-onnx` helper to run Hugging Face ONNX embedding, reranking, and chat models without Python. You can install a **DirectML** (Windows: compatible AMD, Intel and NVIDIA GPUs), **CUDA**, or **CPU** variant; downloads are verified with SHA-256. Before installation, the card lists the download size and expected disk size separately; afterwards it shows the installed disk size. It refreshes after automatic installs. Missing or resized files are reported as a damaged install; use **Install Again** to repair it. ONNX chat models (onnxruntime-genai) work on Windows only for now and have no tool calling. |
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
| **Server Defaults** | **Port**, **GPU layers (-ngl)**, **Context Size (-c)** | Starting values for newly created servers. Port `0` (Automatic) picks a free port from 11440 to 11499. A server that was started before uses its own saved values. |
| **Server Start** | **Load timeout (s)**, **Restart Automatically After a Crash**, **Maximum Attempts** | If a server is not ready within this time, the start ends with an error (increase it for large models). A server that stops unexpectedly while running is restarted with waits of 2, 4, 8... seconds; load errors (memory, a damaged file, a port) are not retried. |
| **Default Bind Address** | **This computer only**, **Local network**, **Custom address** | `127.0.0.1` is open only to this computer (recommended). **Local network** (`0.0.0.0`) lets devices on the same network connect and requires an API key; if you want to open it without a key the app asks for an **Open Anyway** confirmation (these confirmations are listed on **Security > Permissions**). |

**Network > Access** decides which web apps running in a browser can reach the servers (**Allowed Browser Origins (CORS)**). Desktop clients and command-line tools are not affected. Harness Studio does not change your firewall by itself; Windows Firewall may ask for permission on first start, and allowing only "Private networks" is recommended.

## Security

The **API Keys** page shows each server's key status (**Key set** / **No key**) and has **Generate Strong Key**, **Generate New Key**, and **Copy Key** buttons. Keys are kept in the operating system keychain and are not written to files. A new key takes effect when the server restarts, and clients using the old key need updating too. The **Permissions** page lists the **Open Anyway** confirmations you gave for opening servers to the network without a key and lets you take them back. The detailed conditions are in the [security model](../../en/SECURITY_MODEL.md).

## Gateway

**Endpoint** turns the gateway on and edits its address, **Routing** has the model load, unload, and queue rules, and **Responses** has the storage options of the OpenAI Responses API. The gateway is off by default and stops when the app closes. See the [Connections chapter](05-connections-and-gateway.md) for details and for connecting clients.

## Tools

**Built-In Tools** holds the permission policy, trusted tools, the default working folder, the web search provider, and the **Offer Tools to MCP Clients** option. **Installed Tools** manages ready-made or generated packages. Details are in the [Tools chapter](06-tools.md).

## Developer

**Control API** has the API that lets programs manage Harness Studio, and named tokens; you do not need to enable it for everyday chat. The **Use in a Terminal** area on the **CLI** page shows the command-line tool; **Add to PATH** makes it reachable from a terminal. Terminals that are already open do not see the change; open a new one. See [CLI, Control API, and MCP](09-cli-api-mcp.md) for details.

## OpenTelemetry

**OpenTelemetry** is for exporting metrics and traces to a collector you choose. It is meant only for people who run observability infrastructure, and it is off by default. Unless you choose **Turn on Export**, the app does not connect to a collector or send data on its own. The one exception is the **Test Export** button: it sends a real test span to the address in the form even when export is off. When export is on, data goes to the collector address you entered. If the build has no gRPC support, choose the **HTTP/protobuf (port 4318)** protocol.

## Automations

**Automations** connects Harness Studio to an n8n instance you already run. Harness Studio does not install, bundle or start n8n, and nothing is sent until you save an address and an API key.

1. In n8n, create an API key under **Settings > n8n API**.
2. In **Settings > Automations**, enter the n8n URL (for example `https://n8n.example.com` or `http://localhost:5678`) and the key, then choose **Test Connection**. The key is stored in your operating system keychain, not in the settings file.
3. After you save, the page lists your workflows with their active state and triggers, and the latest executions with their status. **Show Error** on a failed execution reads the error message and the failing node from n8n. **Open in n8n Editor** opens the workflow or execution in your browser.
4. **Run** starts a workflow after you confirm. It works through the workflow's own Webhook trigger, so the workflow must be active and have a plain Webhook node. Workflows that only have a manual or schedule trigger are listed but cannot be started from here, because n8n's public API has no run call.

**Harness Models for n8n** shows the gateway's OpenAI-compatible **Base URL**. In n8n, add an OpenAI credential, paste the Base URL, and use a token as the API Key. **Create Token** makes a token named `n8n` with only the `inference` scope; it is shown once, so copy it then. The gateway must be running (see [Connections and the Gateway](05-connections-and-gateway.md)). If n8n runs in Docker, replace `127.0.0.1` in the Base URL with `host.docker.internal`.

## Appearance & Startup

| Setting | What it does |
| --- | --- |
| Theme | **Dark**, **Light**, **System**, **OLED Black**, **Night**, **High Contrast Dark**, **High Contrast Light**. |
| Language | Turkish or English; the change applies immediately. |
| **Interface Level** | **Simple** or **Advanced**; see [Simple and Advanced levels](README.md#simple-and-advanced-interface-levels). It changes only what is visible, never a saved value. |
| **Interface Size** | Scales text, icons, and spacing together. Shortcuts: Ctrl+= to enlarge, Ctrl+- to shrink, Ctrl+0 to reset. |
| **Colour-blind Friendly Colours** | Replaces status and chart colors with colors that do not depend on telling red from green. |
| **Start When You Sign In** | Harness Studio starts when you sign in to your user account. It takes effect immediately. |
| **Start Minimized** | On an automatic start no window opens and the app waits in the tray. Available only while **Start When You Sign In** is on. |
| **Restore Last Session** | On launch, the server you last started starts again. |
| **When Closing the Window** | **Ask**, **Minimize to Tray**, or **Quit**. On macOS the equivalent of tray is **Keep in the menu bar**. |
| **Keep the Window Loaded in the Tray** | Whether the page stays in memory while in the tray; explained below. |

On Windows, the app writes its own per-user `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` entry with a quoted executable path. Status read errors are reported. If you turned the entry off in Task Manager's **Startup apps**, the setting shows it as off. If an earlier version run as administrator left a machine-wide `HKLM` entry, the app says so: open the app once as administrator and switch this setting off and on once to remove it. On Linux, it writes an XDG autostart entry under an absolute `XDG_CONFIG_HOME` (otherwise `~/.config`), pointing to the AppImage itself when used. After moving or renaming an AppImage, launch it once from the new location to refresh the entry.

## Updates

The **Updates** page shows the app version and has an **Application** section with **Check for Updates**. When a newer version is published, you can use **Install Update**; the app downloads and installs it, then restarts, and all running servers are stopped. On Windows, the download and signature verification finish first, then servers are stopped and cleanup runs before the installer starts. If installation fails after that stop, the error says the servers were stopped; restart the ones you need. A download failure leaves them running. If the check reports that no update source is available, download the newest installer from the project's [GitHub Releases](https://github.com/struklabs/harness-studio/releases) page and run it. The llama.cpp engine is updated separately on **Runtime > Installed Runtime**.

## Privacy

**Crash Reports** in **Settings > Privacy** is off by default and enables local reports for Rust panics, unexpected desktop exits, and engine process failures. **Retention period (days)** defaults to 30 days (1-365); expired reports are removed on startup, when reports are opened or created, when settings are saved, and every minute while the app runs.

Reports include the app version, OS, engine, optional GPU/driver summary (**Hardware Info**), compiled stack frames, a normalized panic/error category, and up to 40 recent engine log lines. Free text, chat, model and file contents are omitted; log lines keep only recognized error categories. Home paths are replaced with `~` and known token formats are redacted.

A forced shutdown is detected on the next launch as an unexpected exit; native crashes cannot provide a stack trace. While enabled, a local snapshot of sanitized engine log categories is refreshed every minute for recovery on the next launch. No report is created while collection is disabled.

**Privacy** also offers category preferences: **Crash Reports**, **App errors**, **Hardware Info**, and **Performance Metrics**. Reports are kept only on this device and are never sent anywhere automatically. Prompts, model responses, file contents, and API request bodies do not go into these reports. Chat history and the optional analytics content capture are separate functions; also read the plain-text content capture explanation in the [analytics section](07-performance-and-analytics.md#analytics-and-the-request-log).

## Diagnostics

On **Diagnostics > Health**, **Run Checks** checks your setup, runtime compatibility, GPU libraries, and ports; results are marked **OK**, **Warning**, or **Error**, and **Copy JSON** adds them to a report. **Diagnostics > Reports** is where you review (**Review**), copy, export an individual report with **Export as ZIP**, delete individual reports with **Delete Report**, or delete all reports with **Delete Reports**. ZIP export contains only the selected sanitized JSON report. Sharing is manual: no network upload endpoint is configured or called. Upload is reserved for a future version; enabling local collection does not authorize future uploads. For a problem, first read the explanation of the failed check, then continue with the [troubleshooting chapter](10-troubleshooting.md).

## Leaving the Window Open in the Tray

If **When Closing the Window** is set to **Minimize to Tray**, closing the window does not close the app: it keeps running in the system tray (the menu bar on macOS), and servers and downloads continue. Reopen the window from the tray icon; to quit completely, use **Quit** in the icon's menu. If **Quit** is selected, running servers are stopped. If your desktop has no tray or menu bar icon, closing the window quits the app. On Linux this includes stock GNOME, which shows tray icons only with the AppIndicator extension.

To save memory while in the tray, the window's web page can be closed:

1. In **Settings > Appearance & Startup**, set **When Closing the Window** to **Minimize to Tray**.
2. Close the window. Servers and downloads keep running in the app process.
3. If **Keep the Window Loaded in the Tray** is off (the default), the interface page is closed after about 30 seconds in the tray and only the app process remains; this saves about 60 MiB in the tray. When you open it from the tray icon the page is rebuilt (about half a second) and your open tab comes back.
4. The page is not closed, even in the tray, when: an answer is streaming or a tool call is running in the Playground, a download is running, a tool approval is waiting, there is a message you typed but did not send, there is an unsaved server setting on the Dashboard or an unsaved change in Settings, or the tool creator or an audio recording is running.
5. If you want instant readiness, turn **Keep the Window Loaded in the Tray** on. It uses more memory but opening is immediate.

Input that falls outside these protections (for example, text typed into a search box but not searched) can be lost; save your work before you close the window. On macOS the preference appears as **Keep the Window Loaded in the Menu Bar**.

Next: [CLI, Control API, and MCP](09-cli-api-mcp.md).
