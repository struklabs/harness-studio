# Model Hub and My Models

Türkçe: [../tr/02-model-hub-ve-kutuphane.md](../tr/02-model-hub-ve-kutuphane.md)

**What you do here:** find a suitable model file, download it, set up Hugging Face access, convert or import models, and manage your local model definitions.

## Find a Model

1. Open **Find a Model** in Simple, or **Model Hub** in Advanced, from the left menu.
2. Type a model name into **Search models** and press **Search**. You can also browse without typing anything: with an empty search, the list follows the filters and the **Sort** order (**Trending**, **Most Downloaded**, **Most Liked**, **Recently Updated**, **Newest**).
3. In Advanced, use the detailed **Filters** panel for **License** and **Author**. Choose **Task** from the task picker; **Sort** is on the toolbar. Choose **Text Generation (LLM)** for chat, or **Speech Recognition (ASR)** for speech to text. The task list follows Hugging Face's own categories, and each task is marked with whether it can run in Harness Studio. **Decision model** lists the repositories Hugging Face tags as decision models (models that answer yes/no, choice, and score questions with probabilities); their cards carry a **Decision model** label.
4. In Advanced, narrow the results with **Tag**, **Quant**, and **Size**. If nothing matches, press **Reset Filters** and try again.

By default the list shows only GGUF repositories, the file format llama.cpp uses. For speech models, search for `whisper` (this finds Whisper ggml `.bin` files) or pick the **Speech Recognition (ASR)** task.

**Also show non-GGUF models** widens that default. Being listed does not mean a model can run. Downloads of unsupported formats are disabled, so pay attention to the **Won't run here** and **Partial support** notes on cards. The card and drawer explain the specific reason a repository cannot run, such as an unsupported architecture, a CTranslate2 export, an Apple-only format, or a component that needs the full model. For recognized `Systran/faster-whisper-*` exports, **Open whisper.cpp Build** opens the matching ggml file in `ggerganov/whisper.cpp`; the CTranslate2 export itself cannot run here. Some formats are supported through optional engines; the card tells you which one a model needs:

| Format | What it needs |
| --- | --- |
| Whisper ggml `.bin` | Speech module (whisper.cpp) |
| ONNX speech and embedding models, ONNX chat models | sherpa-onnx or ONNX Runtime (see [Settings](08-settings.md#runtime)). Speech repos in sherpa-onnx's layouts (Piper, Kokoro, Matcha with its vocoder, KittenTTS voices; Zipformer and other transducers, Paraformer, SenseVoice, Whisper, Moonshine v1 recognisers) show as runnable and download as one model per precision; Optimum or transformers.js exports do not. |
| Diffusion image models (SD1.5, SDXL, SD-Turbo, SD3.5, FLUX packages) | Image module (stable-diffusion.cpp). The extra files a model needs (VAE, text encoders) are downloaded along with it. |
| Safetensors LLMs | Convert to GGUF (see [below](#convert-safetensors-models-to-gguf)), the optional mistral.rs runtime, or Python (`python-hf`, Transformers) when there is no GGUF and no mistral.rs build |
| CrispASR / audio.cpp GGUF speech models | CrispASR; experimental audio.cpp (off by default) |
| MLX-format chat, vision, and audio | MLX engines (Apple Silicon, macOS 14+) |
| Hugging Face task folders / diffusers pipelines | Python (`python-hf`, CPU/CUDA); diffusers opens in Image Generation |
| Safetensors sentence-transformers embedders and cross-encoder rerankers without an `onnx/` folder | Python (`python-hf`). They work as embedding and rerank models in knowledge bases and on `/v1/embeddings` and `/v1/rerank`; a repo that ships an ONNX export runs on ONNX Runtime instead, which starts faster and uses less memory. The decoder-based rerankers (Qwen3-Reranker) are not cross-encoders and are not offered. |

**Safetensors chat and vision models on Python.** When a Hub repository has no GGUF file and you have not added a mistral.rs build, Harness Studio runs the safetensors checkpoint itself with Transformers on the Python engine. Its card says **Supported (Python)**, and the details panel shows the size of the weights and the memory to expect. Memory use is the weights plus about 0.7 GB; on a processor without native bfloat16 support, 16-bit weights are loaded as 32-bit, so plan for about twice the download size. When that is more than your free memory, the panel warns you. These models are slower than a GGUF and do not take quantized checkpoints (GPTQ, AWQ, bitsandbytes), and models that need their own code are refused. Harness checks engines in this order: GGUF (llama.cpp), then mistral.rs, then Python. **Convert to GGUF** stays available for a faster result.

If a card says **Required: Speech module (whisper.cpp)** or **Required: Image module (stable-diffusion.cpp)**, you can still download the model, but you need to press **Install** (above the list, or in the model's details) before you can run it.

**Why is a model missing from the list?** Many official model repositories contain no GGUF file at all; `meta-llama/Llama-3.2-1B-Instruct` is an example. The default Model Hub list does not show such repositories, only GGUF copies published by third parties. Those copies usually do not require license approval, but the original publisher did not make them: check the publisher and the license on the **Model Card** before downloading.

In **Simple**, start with **Chat**, **Transcribe Audio**, **Create Speech**, or **Generate Images**; **All Tasks (N)** opens the full task list. Use **Advanced Settings** for the detailed filter panel and **Custom URL**. The **Sort** menu remains available in both levels and follows Hugging Face ordering and pagination with the same filters. It shows the full label when space permits; hover for the full label when constrained.

**Best for My Hardware** is a separate button next to the task filters, available at both levels. It ranks loaded results by estimated full GPU fit, then partial offload/CPU using RAM, then unknown; models that cannot run come last. Download popularity orders each tier. Cards show a fit badge and suggested quantization. Metadata is fetched lazily for visible results without downloading model weights; rankings can change as more results load. This is an estimate, not a guarantee.

In Simple, the file drawer emphasizes the recommended download and hides technical details. Use **Show All (N)** to compare alternatives locally, or switch to Advanced for the full quantization details.

## Variants, Quants, and Memory

1. Open the **Files and quants** section of a repository.
2. Read the task, license, and description on the **Model Card**.
3. In the **Files** table, compare the **Quant**, **Size**, and **Hardware fit** columns.
4. Look at the recommended variant and press **Download**. On buttons that show a size the text is **Download ({size})**, with the file size in place of `{size}`.

A quant stores model weights in a more compact form. The filter offers **Q4_K_M (recommended)**, **Q5_K_M (high quality)**, **Q8_0 (maximum)**, and **Q4_0 (light)**. As a rule, a smaller number means a smaller file and slightly lower quality. If you are unsure, start with Q4_K_M. These labels do not guarantee that a file fits your computer.

File size and working memory are not the same thing. The chat history needs a KV cache, and the computation needs scratch space. **Hardware fit** is an estimate; it can change when the context grows or when other models are loaded. If a model barely fits, try a shorter context first (see the [Dashboard chapter](03-dashboard-and-servers.md#what-the-settings-mean)).

A split variant needs all its parts. The queue downloads them together; do not pick a single part and try to run it. Vision models can also need a projector (mmproj) file; check the model card and the file match.

## Convert Safetensors Models to GGUF

Some repositories contain only safetensors weights. When Harness Studio can convert one, its card shows **Convertible to GGUF** and offers **Convert to GGUF**. The panel shows the architecture, parameter count, download size, size after conversion, and the disk space needed and available.

1. Press **Convert to GGUF** on the card or in the repository details.
2. Choose a **Quantization** (Q4_K_M, Q5_K_M, Q6_K, Q8_0, F16, or BF16) and decide whether to keep the downloaded safetensors files.
3. Start the conversion and follow it in the **Conversions** list: preparing the environment, downloading, converting, quantizing, and adding to the library.

The first conversion builds a conversion environment (about 700 MB, once) with a managed Python; Python runs only during conversions. You can install or delete this environment in **Settings > Runtime > Registry** (**GGUF Conversion Environment**, an Advanced page). Conversion needs the official managed llama.cpp runtime. Repositories that contain only a LoRA adapter, or models whose architecture the installed converter does not know, cannot be converted. For vision models the vision encoder (mmproj) is converted too.

## Download Queue

The **Download Queue** shows downloading, verifying, and failed items. Downloads continue while the app is open, including when it is minimized to the tray.

| Action | What it does |
| --- | --- |
| **Pause** | Pauses the download and keeps the part already downloaded. |
| **Resume** | Continues a paused download. |
| **Move to front** | Gives a waiting download priority. |
| **Remove** | Removes the item from the list; **Also delete partial data** is a separate choice. |
| **Clear Finished** | Clears finished items from the queue view. |

Do not treat a file as complete until **Verifying (SHA-256)** has finished. Verification checks that the file arrived intact. If Hugging Face provides no checksum for a file, only its size and format are checked, and no SHA-256 verification happens. After **Completed**, the file is in your primary model folder. If a download fails, see [Troubleshooting](10-troubleshooting.md).

You can also download from a direct link with **Custom URL** (HTTPS only; Harness Studio blocks addresses that point to internal networks). The file must be a GGUF or a Whisper ggml model.

Repositories you have already downloaded get an **Update Available** badge when a newer version exists on Hugging Face. In the variant drawer, a downloaded variant offers **Update** when an update exists; **Download Again** is available in **Advanced** mode.

## Hugging Face Account: Two Ways

Account settings are in **Settings > Storage > Hugging Face Account**. You need an account to download gated (license-approval) or private models; signing in also raises the request limit Hugging Face applies to anonymous users. Openly licensed models do not need an account.

There are two ways because they trade simplicity for convenience. An access token is the simplest: you paste one value. Signing in through your browser (OAuth) saves you from creating and copying a token and refreshes the session on its own, but you must first create an OAuth app on Hugging Face and enter its client ID. For most people a token is enough.

### The Simple Way: An Access Token

1. Sign in on the [Hugging Face tokens page](https://huggingface.co/settings/tokens) (the **Open the Tokens Page** button opens the same address).
2. Create a token with **Read** permission.
3. In Harness Studio, open **Settings > Storage > Hugging Face Account**.
4. Paste the value into the **Access Token** field and press **Save** next to it.
5. Check that the **Account** row says **Connected with an access token**. The token is stored in your operating system keychain and replaces any existing OAuth sign-in. Use **Remove** to delete it.

### Signing in with Your Browser: OAuth

1. On the same page, open **Advanced: OAuth App**. This route needs the client ID of an OAuth app.
2. Press **Open the New App Page** and create a public OAuth app (one without a secret) on Hugging Face. The app asks for these values:
   - Redirect URI: `http://127.0.0.1/callback`
   - Scopes: `openid profile read-repos gated-repos`

   The same section of the app shows these values; use the ones shown there.
3. Enter the client ID in **Client ID** and press **Save**.
4. Press **Sign In with Hugging Face** and approve in the browser. While waiting you can press **Cancel**; if you do not finish within 5 minutes, the sign-in times out.
5. Back in the app, check that the **Account** row says **Signed in with Hugging Face**.

**Sign Out** only deletes the token from this computer. To remove the app's access to your account completely, revoke it on huggingface.co's connected applications page (**Open Connected Applications** opens it).

If you have no client ID, use the access token. See the [security model](../../en/SECURITY_MODEL.md) for how these credentials are handled.

## Download a License-Gated Model

An example of a gated model is `google/gemma-3-1b-it-qat-q4_0-gguf`. This is Google's official GGUF repository and requires manual license approval. It contains one file, `gemma-3-1b-it-q4_0.gguf` (Q4_0, about 1 GB).

1. Connect your account first, using either method above.
2. Search for the repository name in **Model Hub** and open **Files and quants**.
3. The card shows **License acceptance required**. Press **Accept the License on huggingface.co** to open the model page.
4. On the model page, sign in with the account you connected to the app and accept the license terms. For manually approved models you may have to wait for the model owner.
5. Return to Harness Studio and press **Retry**. When you see **License accepted, you have access**, download the file with **Download**.

Without access, **Add Anyway** queues the download but does not grant permission, so it will most likely fail. Adding a token does not replace accepting the license.

For models whose license was only just approved, Hugging Face sometimes returns a masked checksum. The app treats a masked checksum as invalid and fetches a fresh one with your current token when it verifies the file. If a download still ends with a SHA-256 error, the checksum really does not match: the damaged file is deleted, so start the download again.

## My Models and Model Definitions

1. Open **My Models** from the left menu. The list on the left has the **Models** group (**All**, **Chat**, **Vision**, **Embedding**, **Reranker**, **Speech**, **Image Generation**, **Text to Speech**), the **Supporting Files** group (**Draft**, **Adapter**, **Projector**), and the **Management** group (**Import**, **Model Definitions**). The **Embedding** and **Reranker** categories, supporting-file pages, duplicate finder, and **Model Definitions** are Advanced only; **Import** remains available in Simple.
2. If a file does not appear, press **Rescan Library**. You can add models in other folders under **Settings > Storage > Directories**.
3. On a model card, look at the size, quant, context, hardware estimate, and any missing-part warning. Organize the list with favorites and recent use. A chat model card offers **Load and Chat**. An image, speech, voice, embedding, or reranker card offers **Open in Playground**, which opens the matching tab with that file selected. This also works for files that were already in your folders and not downloaded through Model Hub; you do not need to create a definition first.
4. Check the file's **Definitions (Aliases)** section. A definition is made for you when you press **Load and Chat** or **Open in Playground**, and the Playground tabs also list scanned image, speech, embedding, and reranker files by themselves. Choose **New Definition** only when you want an extra one with your own settings.
5. Set an **Alias** and, if needed, a **Default preset**.

A model definition ties a model file to the load settings used with it (context, GPU layers, KV cache, chat template), to supporting files (a LoRA adapter, a draft model, a vision projector), and to a default preset, under one name. You can create different definitions of the same file for different purposes, for example one with a long context and one that is fast. The **Alias** is the model name other programs use. It must start with a letter or digit and contain only letters, digits, and `. _ - : /`.

Deleting a definition does not delete the model file. If you change an alias, reapply the settings of clients that used the old name from **Connections**. Definitions can be exported and imported (`.harness.toml`) and can be created from an Ollama Modelfile.

The **Import** page lets you link or copy models from Ollama and LM Studio into the library. Linked models cannot be moved or deleted; only the link is removed. Read the delete preview, and stop the server before you delete a model that is in use. A model with missing parts cannot be loaded; use **Repair ({repo})** when it is offered.

See also the [Features reference](../../en/FEATURES.md) and the [security model](../../en/SECURITY_MODEL.md).

Next: [Dashboard and Servers](03-dashboard-and-servers.md).
