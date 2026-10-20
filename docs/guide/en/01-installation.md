# Installation and First Launch

Türkçe: [../tr/01-kurulum.md](../tr/01-kurulum.md)

**What you do here:** install the app, get past the operating system's first-launch warning, finish the setup window, and choose where your models are stored.

## Get the Installer

Harness Studio 0.1.2 is distributed as platform-specific packages on the project's [GitHub Releases](https://github.com/struklabs/harness-studio/releases) page. To build the app yourself, see the build section of the [main README](../../../README.md); this page covers installing from the ready-made installer.

| System | Package | Status |
| --- | --- | --- |
| Windows 10/11 (x64) | NSIS installer (`.exe`) | Available. The installer is not code-signed yet. |
| Linux (x64) | `.deb` / AppImage | Preview packages; the release workflow builds both formats. |
| macOS 13.3+ (Apple Silicon and Intel) | DMG | Available. Official release DMGs are signed and notarized by Apple. |

### Windows

1. Run the `.exe` installer and follow its steps.
2. If Windows SmartScreen shows a warning, choose **More info**, then **Run anyway**. The warning appears because the installer is not code-signed yet.
3. Open Harness Studio from the Start menu.

**Uninstalling.** When you uninstall from Windows (Settings > Apps, or the Start menu), Harness Studio opens an **Uninstall Harness Studio** window before anything is removed. The app files are always removed; you choose what happens to your data:

- **Remove Everything** deletes the app and all of its data on this computer, including models, conversations, and saved keys.
- **Choose Items** lets you tick items one by one: **Models**, **Conversations & Projects**, **MCP Servers**, **Skills**, **Tools**, **Agents**, **Engines & Runtimes**, **Python Environments**, **Caches & Logs**, and **Settings**. Each item shows its size. By default your own content (models, conversations, skills, tools, MCP servers, settings) is kept, and re-downloadable items (engines, Python environments, caches, logs, agents) are removed.

A summary screen then lists what will be deleted and what will be kept, with the space it frees. Press **Uninstall** to start; it normally finishes in a few seconds, longer if you delete very large models. Folders you added as external model folders are deleted only if you select them one by one. If the window cannot start, the classic uninstaller dialog appears instead; it keeps your data unless you tick **Delete the application data**.

The installer may leave an entry under `HKCU\Software\struklabs\Harness Studio` that only remembers the install location and language; it is harmless and can be deleted (an older `HKCU\Software\harnessstudio` key may come from an earlier version).

**Uninstalling from inside the app.** Open **Settings > Storage** and use **Uninstall Harness Studio** > **Open Uninstall Window**. The app quits and the same window opens. After your data is cleaned up, its last screen finishes the job for your system: on Windows it removes the app files, on macOS it moves the app to the Trash, for the Linux `.deb` it runs `apt remove` after asking for your administrator password (or you can run `sudo apt remove harness-studio` yourself), and for an AppImage it deletes the AppImage file when you press the button. A portable copy can simply be deleted.

On Linux, removing the server `.deb` keeps `/var/lib/harness`, `/etc/harness`, and the `harness` account; purging the package (`apt purge`) deletes them.

### Updating the App

Use **Settings > Updates > Check for Updates**, then **Install Update** to download, install, and restart. You can also run the latest package from GitHub Releases. The llama.cpp engine updates separately under **Settings > Runtime > Installed Runtime**.

## First Launch: The Setup Window

Harness Studio keeps the installer simple (no extra pages) and asks what to set up in its own **Harness Studio Setup** window, which opens the first time you start the app. It has the same look as the uninstall window and four screens:

1. **Welcome:** Shows the hardware Harness Studio found (graphics card, memory, and the acceleration it will use) and lets you pick the interface level, **Simple** (recommended) or **Advanced** (see [Simple and Advanced levels](README.md#simple-and-advanced-interface-levels)). Press **Get Started**, or **Skip for Now** to leave everything for later.
2. **Choose:** Category cards with checkboxes and sizes. The defaults are minimal and safe: only the engine and one small first model are ticked, and only if your hardware and free disk space fit. Nothing else is ticked on its own.
   - **Model Engine:** the llama.cpp build that suits your hardware (CUDA for NVIDIA, ROCm/HIP or Vulkan for suitable AMD cards, SYCL or Vulkan for Intel Arc, Metal for Apple Silicon, CPU otherwise). It shows **Installed** when you already have one.
   - **First Model:** a **Tiny** (about 0.4 GB), **Small** (about 1.1 GB) or **Standard** (about 2.5 GB) model. The one recommended for your computer is marked and preselected; pick another size to change it. Bigger models are in the **Model Hub**.
   - **Agents:** the optional **Hermes Agent** (needs 8 GB of memory; it sets up its own Python environment).
   - **Server and Browser Admin:** adds the `harness` command-line tool to your user path, so you can run `harness serve` for the headless server and its browser administration.
   - **Integrations:** **OpenCode**, **Zed** and **VS Code**. Only tools found on this computer can be ticked. The configuration is written (with a backup) when the local server is running; otherwise connect later in **Connections**.
   - **Storage:** shows the **Models Folder** with its free space; press **Change** to use another drive. The **Data Folder** is shown too, but it can only be set outside the app, with the `HARNESS_DATA_DIR` environment variable.
3. **Install:** Each item shows its own status and progress. **Cancel** stops after the current item, and **Retry** runs failed or cancelled items again. Model downloads also appear in the **Model Hub** queue.
4. **Done:** Lists what was installed and hands off to the **Get Started** checklist on the **Dashboard**, which tracks the next steps (start the model, first chat).

Privacy settings are not asked here; they stay off until you turn them on in **Settings**. Keep the window open while items install; downloads need an internet connection.

To run Setup again later, use **Run Setup Again** in **Settings > Runtime > Installed Runtime**, or in the **Get Started** list.

**Silent installs.** Running the installer with `/S` (a scripted install) skips the Setup window: it leaves a `setup-skipped` file next to the app so the first launch opens straight into the app. Run Setup from Settings whenever you want it.

## Choosing the Runtime

The runtime is the engine that runs a model file. Chat normally uses llama.cpp; optional engines can run other chat formats.

| Option | When to use it |
| --- | --- |
| **Managed (recommended)** | For most people. Harness Studio downloads the official release, verifies it, and keeps it up to date. |
| **Custom Binary** | You have your own `llama-server`. Enter its location in **Llama-server Path** and press **Verify**. A custom binary is never updated or rolled back automatically. |

The recommended acceleration depends on your hardware: CUDA for NVIDIA, ROCm/HIP or Vulkan for suitable AMD cards, SYCL or Vulkan for Intel Arc, Metal for Apple Silicon. Without a GPU, the CPU is used. Detecting a graphics card does not guarantee the chosen build can use it. If the installed build cannot see the card, Setup tries Vulkan (it needs a compatible GPU and driver) and tells you on the **Settings > Runtime > Installed Runtime** page. Existing installs keep their build; change it on that page. On Windows, ROCm/HIP is recommended only when the HIP SDK libraries can be loaded; otherwise Vulkan is recommended.

Other kinds of models use optional engines that you install when you need them, with one click: an **Install** button appears next to the "This model needs the … runtime" note on library cards, in the Model Hub and in Playground errors, and optional engine families are listed under **Other Runtimes** in **Settings > Runtime > Installed Runtime** (at both interface levels). ONNX Runtime, Lemonade, the conversion environment, and extra builds are managed in **Settings > Runtime > Registry** (an Advanced page):

| Capability | Engine |
| --- | --- |
| Speech to text | whisper.cpp; sherpa-onnx (Parakeet, SenseVoice, Omnilingual, Nemotron, Cohere); ONNX CTC; CrispASR; Python (Transformers, faster-whisper); MLX audio |
| Text to speech | sherpa-onnx (Kokoro, Piper); Python; CrispASR; MLX audio; experimental audio.cpp (off by default) |
| Image generation | stable-diffusion.cpp; Python (diffusers) |
| ONNX embeddings, reranking, and chat | ONNX Runtime and ONNX Runtime GenAI (through the bundled `harness-onnx` helper) |
| Converting safetensors models to GGUF | A small conversion environment, built on first use |
| AMD Ryzen AI NPU and extra llama.cpp builds | Lemonade (optional) |
| Safetensors models without conversion | mistral.rs (optional, add your own build); without it, Python (`python-hf`) runs safetensors chat and vision models |
| Hugging Face task models, safetensors chat models | Python (`python-hf`, CPU or CUDA) |
| MLX chat, vision, and audio | Optional MLX engines (Apple Silicon, macOS 14+) |

These are covered in [Model Hub and My Models](02-model-hub-and-library.md), [Playground](04-playground.md), and [Settings](08-settings.md).

## Model Folder and Data Folder

**Settings > Storage > Directories** has two sections:

- **Primary Model Folder**: where new downloads are saved. Change it with **Choose Folder**.
- **Extra Folders**: add existing models on other drives to the library. Type a folder path and press **Add**, or pick one with **Browse**. Each folder shows whether it is reachable, its free space, and how many models it holds.

Application data (settings, the chat database, engines, and logs) is kept in a separate data folder, not in the model folder.

| System | Default data folder |
| --- | --- |
| Windows | `%APPDATA%\com.harnessstudio.app` |
| macOS | `~/Library/Application Support/com.harnessstudio.app` |
| Linux | `$XDG_DATA_HOME/com.harnessstudio.app` (default: `~/.local/share/com.harnessstudio.app`) |

To change the data folder from the command line, see the [CLI reference](../../en/CLI.md). For how credentials are stored, see the [security model](../../en/SECURITY_MODEL.md). To start from scratch, close the app, back up this folder, and delete it; your settings and chats are deleted with it. For full removal on Windows, first run `harness service uninstall` if you installed the CLI login service, clear stored credentials (Hugging Face token, server API keys) in the app or in Windows Credential Manager (entries containing `com.harnessstudio.app`), then uninstall and delete the retained data and model folders you no longer want.

Next: [Model Hub and My Models](02-model-hub-and-library.md).
