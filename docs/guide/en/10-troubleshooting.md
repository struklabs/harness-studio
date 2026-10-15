# Troubleshooting and FAQ

Türkçe: [../tr/10-sorun-giderme.md](../tr/10-sorun-giderme.md)

**What you do here:** recognize the message on your screen, understand its cause, apply the right fix, and find answers to common questions.

## Three Checks First

1. On the **Servers** page, check the status (**Running**, **Starting**, **Stopped**, **Error**) and address of the right server. (If you do not see the **Servers** page, switch the interface to **Advanced**; the Dashboard's **Overview** page shows the status of the selected server too.) In the switcher at the top (**Controlled server**), make sure the server you want to work with is selected.
2. In **Server Logs** on the Dashboard, read the first error line. Do not mistake loading for a connection error; a large model can take minutes to load.
3. Use **Run Checks** on **Settings > Diagnostics > Health**. If the runtime, GPU library, or port check shows **Error** or **Warning**, fix that first.

The messages below are quoted from the app's English interface. Fields such as `{url}`, `{path}`, and `{port}` are filled with real values on screen. An operating system error detail can be in a different language.

## Error Messages and Fixes

### Cannot Connect to the Server

| Actual message | Cause and what to do |
| --- | --- |
| `Could not connect to the server ({url}). Is it running? Check it on the Servers page.` | The local address (127.0.0.1) could not be reached: the server may be stopped, crashed, or on another port. Check the status on the **Servers** page and use **Start** if it is stopped. If it is running, compare the address in your client with the one shown in the app; with an Automatic port the number can change on every start, so use the gateway for a fixed address. |
| `Could not connect to the server ({url}). Check your network connection, proxy settings and that the address is correct.` | A non-local (remote) address could not be reached. Check the address, the network connection, and the proxy settings. If it happens while downloading a model, check access to Hugging Face too. |
| `The request timed out ({url}). The local server may be busy or not responding. Check its status and log on the Servers page.` | The local server did not answer. Wait for running requests to finish; if the log shows an error or a shutdown, restart the server. |
| `The request timed out ({url}). Check your network connection, proxy settings and that the remote server is responding, then try again.` | The remote server did not answer. Check the network and the remote server. |
| `Could not resolve the server name ({url}). Check the address and network/DNS settings.` | The domain name cannot be resolved: a typo in the address or a DNS problem. |
| `Could not establish a secure connection ({url}). Check the server TLS certificate and your computer clock.` | The certificate is invalid or the computer's clock is wrong. |

### The Model Does Not Fit in Memory, or the Server Stops While Loading

| Actual message | Cause and what to do |
| --- | --- |
| `The GPU ran out of memory; the server was stopped. [{code}]` and the **The model did not fit in memory** dialog | The model, the context, or the compute space did not fit in graphics memory. Follow the [OOM recovery steps](03-dashboard-and-servers.md#when-memory-runs-out-oom): **Retry with This** reduces the context, compresses the KV cache, or lowers GPU layers. For a lasting fix, choose a smaller model or quant. |
| `Not enough memory: llama-server could not allocate memory for the model (VRAM or RAM).` | The classified explanation of the same situation. Check RAM and VRAM needs together; if another server is holding the graphics card, stop it. |
| `The model does not fit in GPU memory: ~{need_mib} MiB needed, the GPU share of unified memory is {budget_mib} MiB. Lower the context or the GPU layers.` | The pre-start memory check. Lower the context or the GPU layers by as much as suggested. |
| `The GPU stopped the work (driver reset or GPU error). Start the server again; if it repeats, try fewer GPU layers or another GPU driver version.` | The GPU driver interrupted the process. Update the driver or reduce GPU layers. |
| `The model file is damaged or incomplete (interrupted download?). Download it again.` | The file did not arrive in full or is damaged. Download the model again; for a split model, check that every part is present (**My Models > Repair**). |
| `This llama.cpp runtime does not support the model architecture "{arch}". Update the runtime.` | The model is a new architecture and the engine is old. Check for an update on **Settings > Runtime > Installed Runtime**. |
| `The server was not ready within {seconds} s. Raise the load timeout for large models.` | The model did not load in time. Raise **Load timeout (s)** on the Dashboard's **GPU** page (for that model) or on **Settings > Network > Listening** (globally). If the log shows another error, fix that first. |
| `llama-server exited while loading ({status}). Check the logs. [{code}]` | The process quit during loading. The last lines of **Server Logs** say why (memory, a damaged file, an unsupported architecture, a missing library). |

### Port in Use

| Actual message | Cause and what to do |
| --- | --- |
| `Port {port} is used by "{program}" (PID {pid}). Pick another port or set the port to Automatic.` | Another program holds the port you chose. Enter a different port on **Dashboard > Network** or choose **Automatic**. Change the port instead of closing a program you do not recognize. |
| `Port {port} is used by the "{server}" server. Pick another port or set the port to Automatic.` | Two Harness Studio servers were given the same port. Give them different ports or use automatic selection. |
| `The gateway could not start on this address (the port may be in use)` | The gateway port is taken. Change the port on **Settings > Gateway > Endpoint**; if the default port is taken, Harness Studio moves to a free port itself and shows the new address on the **Connections** page. |

### The Runtime (Engine) Is Missing or Will Not Start

| Actual message | Cause and what to do |
| --- | --- |
| `The llama.cpp runtime is not installed yet. Download it with one click under Settings > Runtime, or pick the llama-server on your system.` | The engine is not installed. Download the managed engine on **Settings > Runtime > Installed Runtime**. |
| `llama-server not found: {path}` | The chosen file does not exist. Install the managed engine, or fix the custom binary's path and choose **Verify**. |
| `No valid llama-server binary was found on this system.` | None was found when looking for an engine installed on the system. Install the managed engine. |
| `The custom llama-server path is not set or not valid.` | **Custom Binary** is selected but the path is empty or wrong. Enter the path or go back to **Managed (recommended)**. |
| `A library the runtime needs could not be loaded (GPU driver or runtime files missing or incompatible).` | The driver is missing or old, or the build does not fit the hardware. Run the **Health** check; use the recommended build and a current driver (for example a recent enough NVIDIA driver for CUDA, or the Vulkan build if not). |
| `Stage 1 check failed: the binary could not run ({error}). A DLL may be missing or the architecture may not match.` | The downloaded engine could not run. Try installing another build (Vulkan or CPU). |
| `Stop all running servers before changing the runtime.` | Stop the servers first to change the engine. |
| `llama-bench not found: {path}. Use the official llama.cpp package.` | A benchmark needs `llama-bench`. The managed official package includes it; if you use a custom package, check that the file is there. |

If a card in Model Hub or My Models says an engine is required (the Speech module, the Image module, sherpa-onnx), install it from **Install** in Model Hub or from **Settings > Runtime > Installed Runtime > Other Runtimes**. ONNX Runtime is on **Settings > Runtime > Registry**. If a GGUF conversion fails, the message names the stage and the end of the converter's output; it needs the official managed llama.cpp runtime, enough free disk space, and a repository whose architecture the installed converter supports.

### Model Download

| Actual message | Cause and what to do |
| --- | --- |
| `SHA-256 check failed ({file}). The corrupt file was deleted; please download it again.` | The downloaded file did not match the expected checksum (a connection interruption or corruption); the file was deleted automatically. Start the download again. For models whose license was only just approved, a masked checksum is treated as invalid and a fresh one is fetched at verification, so this warning now points to a real mismatch. |
| `Incomplete download ({file}): {downloaded} / {total} bytes. Retrying resumes where it stopped.` | The connection dropped. Press **Resume** in the download queue; check the connection and the disk space. |
| `Not enough disk space. The target drive needs at least {gb} GB free.` | Free up space, or choose a primary model folder with enough free space on **Settings > Storage > Directories**. |
| `Download failed: HTTP {status}. This model may need access approval (gated) or a valid Hugging Face token.` | The model may be license-gated; see the section below. |
| `Hugging Face rate limit reached. Try again in {retry} seconds; signing in raises the limit.` | Hugging Face's anonymous request limit. Wait, or connect your account. |
| `The downloaded file is not a valid GGUF (magic bytes do not match).` | What arrived is not a model file (for example an error page). Check the address and the repository. |
| `Model not found: {path}` | The file was moved or deleted. Press **Rescan Library** in **My Models** and fix the file selection in the model definition. |

### License-Gated Model and Hugging Face Account

| Actual message | Cause and what to do |
| --- | --- |
| `{repo} requires accepting its license. Sign in with Hugging Face (Settings > Storage > Hugging Face account), accept the license on the model page and retry the download.` | No account is connected. Complete the [account connection and license flow](02-model-hub-and-library.md#download-a-license-gated-model). |
| `{repo} requires accepting its license and your account has no access yet. Accept the license on the model page (some owners approve requests manually), then retry the download.` | The account is connected but the license is not accepted or is awaiting approval. Accept the license on the model page with the same account, then press **Retry**. With manual approval you may have to wait ("Still no access. If approval is manual, try again a little later."). |
| `The Hugging Face token is invalid or expired.` | Create a new **Read** token and save it in the **Access token** field on **Settings > Storage > Hugging Face Account**. |
| `Signing in with Hugging Face needs the client ID of an OAuth app. Enter it in Settings > Storage > Hugging Face account, or use an access token.` | There is no client ID for browser sign-in. The simplest route is to use an access token. |
| `Hugging Face sign-in timed out (5 minutes). Try again.` | The browser approval was not finished in time. Try again with **Sign In with Hugging Face**. |

### Model Busy (503)

When the gateway needs to load a model, it may find no model it can unload to free memory, because the other models are serving requests. In that case the request is rejected with 503.

| Actual message | Cause and what to do |
| --- | --- |
| `{model} cannot be loaded right now: {busy} is serving requests and cannot be unloaded. {wait} try again.` (Playground) | There is no memory to load the new model, and the loaded model(s) are answering at that moment. When the answers finish, press **Retry** in the Playground. |
| `{models} are busy; retry {model} after their requests finish.` and `Retry after the busy model finishes its request.` | The same situation, as the gateway reports it. Your HTTP client receives an English message (for example `Cannot load "{model}": model(s) … are busy; retry after their requests finish.`), the `model_busy` code, the 503 status, and a `Retry-After` header; it should wait and retry. If this happens often, review **Max loaded models**, **Queue per model**, and **VRAM headroom (MiB, 0 = automatic)** on **Settings > Gateway > Routing**, or mark a model **Pinned (always loaded)**. |

## What a Start Failure Explanation Looks Like

Server start failures can also show a classified explanation and **Recovery Options**. The **Open logs** button opens the relevant lines (**Log Lines**). The memory, library, damaged model, architecture, and timeout explanations in the tables above come from this classification.

## Frequently Asked Questions

**Do I need to learn the terminal?** No. In the ready-made app package, installing the engine, downloading models, starting servers, and chatting are all done in the interface. Flows that have no ready-made package, such as the whisper server on macOS, need extra preparation; they are noted in [Installation](01-installation.md) and [Speech](04-playground.md#speech-speech-to-text-and-text-to-speech).

**Which model should I download?** In **Model Hub**, first choose a file whose **Hardware fit** column says **Fits in VRAM**. If you are unsure, start with the Q4_K_M quant and a small model (1B to 8B parameters), then go bigger.

**I downloaded a model. Why can't I chat?** A downloaded file has to be loaded by a server. Start it from the Dashboard, or pick the model definition on the open gateway. If the model is an embedding, reranker, speech recognition, voice, or image model, use the matching Playground tab, or press **Open in Playground** on its library card. Files that were already in your folders are listed there without any extra step.

**I cannot find a page this guide mentions.** You may be in the **Simple** interface level. Switch to **Advanced** with the switch under the main navigation (see [Simple and Advanced levels](README.md#simple-and-advanced-interface-levels)); switching never changes a saved value.

**A model does not show up in search. Do I need an account?** First reset the filters. The default Hub list is limited to GGUF repositories, so official repositories without a GGUF file (for example `meta-llama/Llama-3.2-1B-Instruct`) do not appear; only third-party GGUF copies are listed. For speech, pick the **Speech recognition (ASR)** task to find Whisper and ONNX CTC models. For private or license-gated repositories, account access is needed as well.

**I added a token. Why won't the gated model download?** The token's account must have accepted that model's license. Some model owners approve access by hand. An OAuth client ID is not a substitute for model access permission.

**Why does the model forget earlier information as the chat gets longer?** There is a context limit. Context shift can drop old content. Start a new chat, compress with **History and Compaction**, or, if your memory allows, start the server with a bigger context.

**Why is generation slow?** If not all GPU layers fit on the graphics card, the rest runs on the CPU and speed drops noticeably. Check **GPU layers** and the hardware-fit badge on the Dashboard, and generation speed (t/s) on the **Performance** page; a smaller quant or a smaller context speeds things up. If the installed engine uses only the CPU, read the warning on **Settings > Runtime > Installed Runtime** and install a GPU build.

**Why does a file tool ask for a folder?** The chat's working folder must be selected, or the default folder in **Settings > Tools > Built-In Tools** must be used. The folders in the chat list are separate from this choice.

**The microphone does not work. What should I do?** In your system privacy settings, give Harness Studio microphone permission and check the active microphone. You can also use **Choose File** with an audio file instead of recording. Supported formats are listed in the [Speech section](04-playground.md#speech-speech-to-text-and-text-to-speech).

**Does closing the window stop the model?** **Minimize to Tray** leaves the servers running. **Quit** stops running servers. If the interface is not kept in memory in the tray, unsaved input can be lost; read the [Settings](08-settings.md#leaving-the-window-open-in-the-tray) section.

**Are conversations or requests saved to disk?** Chat history is stored locally; the database is not encrypted. Analytics keeps only metadata by default (time, model, tokens, duration). If content capture is turned on in a model definition, prompts and responses are stored as plain text; see the [Analytics](07-performance-and-analytics.md#analytics-and-the-request-log) section. The app sends no automatic telemetry or crash reports.

**How do I check for a new version?** Look at **Settings > Updates**, or download the newest installer from [GitHub Releases](https://github.com/struklabs/harness-studio/releases). Engine updates work separately on **Settings > Runtime > Installed Runtime**.

**How do I go back to an older version?** Close Harness Studio and, before installing the older version, copy `config/settings.json.bak-v2` in the data folder over `config/settings.json`. Settings you changed since moving to the newer settings format are lost. The details are in the [release notes](../../en/RELEASE_NOTES_0.1.0.md).

If the problem continues, gather the error text, the app and runtime versions, the **Health** check result, and the exported logs. Review them before you share them. The [security model](../../en/SECURITY_MODEL.md), [integrations](../../en/INTEGRATIONS.md), [CLI](../../en/CLI.md), [MCP](../../en/MCP.md), and [tools](../../en/TOOLS.md) references have more detail.
