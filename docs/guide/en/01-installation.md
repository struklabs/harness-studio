# Installation and First Launch

Türkçe: [../tr/01-kurulum.md](../tr/01-kurulum.md)

**What you do here:** install the app, get past the operating system's first-launch warning, finish the setup wizard, and choose where your models are stored.

## Get the Installer

Harness Studio 0.1.0 is distributed as a Windows installer on the project's [GitHub Releases](https://github.com/struklabs/harness-studio/releases) page. To build the app yourself, see the build section of the [main README](../../../README.md); this page covers installing from the ready-made installer.

| System | Package | Status |
| --- | --- | --- |
| Windows 10/11 (x64) | NSIS installer (`.exe`) | Available. The installer is not code-signed yet. |
| Linux | `.deb` / AppImage | Coming soon. Builds exist but have not been verified on real machines yet. |
| macOS | DMG | Coming soon. Builds exist but have not been verified on real machines yet. |

### Windows

1. Run the `.exe` installer and follow its steps.
2. If Windows SmartScreen shows a warning, choose **More info**, then **Run anyway**. The warning appears because the installer is not code-signed yet.
3. Open Harness Studio from the Start menu.

Uninstalling does not delete your user data. It stays in the data folder described [below](#model-folder-and-data-folder). A leftover entry under `HKCU\Software\harnessstudio` only remembers the install location; it is harmless and can be deleted.

### Updating the App

New versions are installed by downloading the latest installer from GitHub Releases and running it. The **Updates** page in Settings shows the app version and can check for a newer one. The llama.cpp engine is updated separately; you manage that in **Settings > Runtime > Installed Runtime**.

## First Launch: Five Steps

The **Setup wizard** opens the first time you start the app. Move forward with **Continue**.

1. **Hardware:** Review your processor, RAM, graphics card, and the recommended acceleration (backend).
2. **Storage:** Check the **Model Folder**. To use another drive, press **Change** and pick a folder. New models are downloaded there. Large models can take tens of gigabytes, so leave enough free space.
3. **Engine:** If a suitable llama.cpp is already on your system you can choose **Use the System Copy**. Otherwise press **Download and Install the Latest**. When the download and verification finish and you see **llama.cpp is ready.**, continue.
4. **Purpose:** Pick **Chat**, **Coding agent**, **Documents / RAG**, or **Speech to text**. The wizard suggests an engine and models that fit your hardware. You can install the suggestions or choose **Skip**. Model downloads run in the background; follow them in the **Model Hub** download queue. This step also asks how you want to use Harness Studio: **Simple** (recommended) or **Advanced** (see [Simple and Advanced levels](README.md#simple-and-advanced-interface-levels)).
5. **Privacy:** Review **Save crash and error reports locally** and **Keep hardware info**, then press **Finish Setup**. Reports stay on this computer and are never sent anywhere automatically.

While the engine install or the **Purpose** install is running you cannot move between wizard steps; wait for it to finish. Model downloads are queued and continue in the background, so you do not have to wait for them.

To run the wizard again later, use the **Open Wizard** button in **Settings > Runtime > Installed Runtime**.

## Choosing the Runtime

The runtime is the engine that runs a model file. Chat uses llama.cpp.

| Option | When to use it |
| --- | --- |
| **Managed (recommended)** | For most people. Harness Studio downloads the official release, verifies it, and keeps it up to date. |
| **Custom Binary** | You have your own `llama-server`. Enter its location in **llama-server path** and press **Verify**. A custom binary is never updated or rolled back automatically. |

The recommended acceleration depends on your hardware: CUDA for NVIDIA, ROCm/HIP or Vulkan for suitable AMD cards, SYCL or Vulkan for Intel Arc, Metal for Apple Silicon. Without a GPU, the CPU is used. Detecting a graphics card does not guarantee the chosen build can use it. If the installed build cannot see the card, Harness Studio switches to a build that works on any card (for example Vulkan) and tells you on the **Settings > Runtime > Installed Runtime** page.

Other kinds of models use optional engines that you install when you need them, from the **Model Hub** (an **Install** button appears above the list or in a model's details) or from **Settings > Runtime > Registry** (an Advanced page):

| Capability | Engine |
| --- | --- |
| Speech to text | whisper.cpp (Whisper models) and sherpa-onnx (Parakeet, SenseVoice, Omnilingual) |
| Text to speech | sherpa-onnx (Kokoro and Piper voices) |
| Image generation | stable-diffusion.cpp |
| ONNX embeddings, reranking, and chat | ONNX Runtime and ONNX Runtime GenAI (through the bundled `harness-onnx` helper) |
| Converting safetensors models to GGUF | A small conversion environment, built on first use |
| AMD Ryzen AI NPU and extra llama.cpp builds | Lemonade (optional) |
| Safetensors models without conversion | mistral.rs (optional, add your own build) |

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
| Linux | `$XDG_DATA_HOME/com.harnessstudio.app`, or `~/.local/share/com.harnessstudio.app` if the variable is not set |

To change the data folder from the command line, see the [CLI reference](../../en/CLI.md). For how credentials are stored, see the [security model](../../en/SECURITY_MODEL.md). To start from scratch, close the app, back up this folder, and delete it; your settings and chats are deleted with it.

Next: [Model Hub and My Models](02-model-hub-and-library.md).
