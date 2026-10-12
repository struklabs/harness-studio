# Dashboard and Servers

Türkçe: [../tr/03-dashboard-ve-sunucular.md](../tr/03-dashboard-ve-sunucular.md)

**What you do here:** pick a model, adjust how it runs, start a server, save settings as profiles, and recover from out-of-memory errors.

## How the Screen Is Laid Out

The **Dashboard** has two parts. The settings are on the left. In the **Simple** interface level you see **Overview**, **Model and Runtime**, and **Context and Memory**. In **Advanced** the list is:

| Group | Page | What it holds |
| --- | --- | --- |
| Server | **Overview** | The selected server's name, purpose, and status. |
| Server | **Model and Runtime** | Model, **Model definition**, and runtime choice; the **Router Mode** switch. |
| Server | **Network** (Advanced) | Port, listening address, and the server API key. |
| Inference | **Context and Memory** | Context size, concurrent requests, conversation memory precision, context shift. |
| Inference | **GPU** (Advanced) | GPU layers, target graphics card, CPU threads, flash attention, load timeout. |
| Inference | **Sampling** (Advanced) | Points to where sampling is edited (the Playground and presets). |
| Advanced | **Template and Reasoning**, **Tools**, **Other Parameters** | Chat template, draft model (speculative decoding), LoRA adapters, raw command-line parameters. |

On the right is live telemetry (generation speed, VRAM and RAM use) with **Server Logs** below it. At the bottom, a bar that saves your settings sits above the **Start Server** / **Stop Server** button.

The server switcher at the top (**Controlled server**) decides which server the Dashboard, Playground, Connections, and Performance pages work with. If you have more than one server, check this first.

## Start Your First Server

1. On **Dashboard > Model and Runtime**, pick a local model and, if you have one, a **Model definition**. If the list is empty, use **Go to Model Hub and download a model**.
2. If there is a runtime warning, [install or verify the runtime](01-installation.md#choosing-the-runtime) first.
3. Read the suggestions on **Context and Memory** and **GPU**. For everyday chat, 4096 or 8192 tokens of context is the app's starting suggestion; check the model's limit and your free memory. The hardware-fit badge next to the model (**Fits in VRAM**, **VRAM is tight**, **Partial GPU offload**, **Mostly CPU**, **May run out of memory**) shows the estimate.
4. On the **Network** page, check the port. With **Automatic**, Harness Studio picks a free port from 11440 to 11499 on every start. If you type a port, that port is saved.
5. Press **Start Server**. While it shows **Starting...** the model is being loaded into memory (large models can take minutes; **Cancel** stops it). Wait for **Server active**.
6. Go to the **Playground** and chat. When you are done, **Stop Server** frees the memory the model uses.

When **Context size** and **GPU layers** are left on **Auto**, Harness Studio picks them for you. On a small graphics card it prefers keeping every layer on the GPU and shrinking the context (it tries 32k, 24k, 16k, 12k, then 8k, each with automatic slots and then one slot) before it falls back to a 16k hybrid with part of the model on the CPU. The Dashboard form shows the context, layers, and slots it is about to use. Values you set yourself (KV cache, slots, context, fit) are left as you set them. The same rule applies when you start a model from the CLI, the Control API, **Load and Chat** in My Models, or **Load** in the Playground.

The **Router Mode** switch serves every ready model in your model folder from a single server instead of one model; clients choose a model by name, and a model loads on its first request or from the **Router Models** list. For everyday use, a single model or the [gateway](05-connections-and-gateway.md) is enough.

The **Server mode** setting normally follows the model type: an embedding model starts as an embedding server and a reranker as a rerank server. It can be changed if you need to.

## What the Settings Mean

The fields below use their names in the app. If you are unsure, leave the recommended values and the **Auto** choices.

| Field (page) | What it means for you |
| --- | --- |
| **Context size** (Context and Memory) | The total input and output the model can handle in one request, in tokens. Chat history, the system prompt, documents, and tool information share this space; when it fills up the model forgets the oldest part. A larger value needs more memory. Coding agents need a much larger context than everyday chat (at least 48k for OpenCode). |
| **Concurrent requests** (Context and Memory) | How many requests the model answers at the same time (the server's "slots"). More wait in line. Slots share the context, so the requests running at once must fit inside it. With **Auto**, Harness Studio suggests a value based on memory; usually 1 for a single user and 2 for agents. |
| **Conversation memory precision** (Context and Memory) | How many bits are used to store the chat's working memory (the KV cache). **Auto (recommended)** keeps full precision (**Full (16-bit)**) while memory allows and steps down to 8 bits when it does not. **Balanced (8-bit)** uses half the memory with a very small quality difference. The saver options use even less; accuracy can drop on very long contexts and some mixed key/value pairs run slowly (the app warns you). |
| **K cache quant** and **V cache quant** (Context and Memory) | The technical form of the setting above. They appear when you choose **Custom (keys and values separately)**. Lowering the keys (K) to 4 bits noticeably reduces quality. |
| **Context shift management** (Context and Memory) | Whether old content is shifted out automatically when the context fills up. Shifting keeps generation going, but the shifted content leaves the model's memory. |
| **Offload KV cache to GPU** | When off, the cache stays in system RAM: you save graphics memory but speed may drop. |
| **GPU layers** (GPU) | How many of the model's layers are loaded onto the graphics card. Speed is highest when all of them are on the card. If VRAM is not enough, reduce it; the remaining layers run on the CPU, which is slower and needs more RAM. |
| **Target graphics card (GPU)** (GPU) | Which card is used when you have more than one. **Auto (recommended)** picks the dedicated card with the most memory; an integrated GPU cannot be selected while a discrete card is present. |
| **CPU threads** (GPU) | The number of cores used for CPU computation. Left empty (**Auto (recommended)**), the physical core count is used. |
| **Flash Attention** (GPU) | Speeds up attention and reduces the memory cost of long contexts. Support depends on the runtime. Harness Studio turns it on itself when the value cache (V) is quantized. |
| **Logical batch** and **Micro batch** (GPU) | How many tokens are processed at once. Lowering the micro batch can help with a compute-memory error. |
| **Load timeout (s)** (GPU) | How long to wait for the server to become ready. Increase it when loading a large model from a slow disk. If empty, the global value in **Settings > Network > Listening** is used. |
| **Draft model** (Other Parameters) | Speculative decoding: a small model from the same family speeds up generation. The gain depends on the model and the hardware. Harness Studio can suggest a compatible draft model. |
| **LoRA adapters** (Other Parameters) | Adapter files applied on top of the model, each with a scale. On a running server, **Live LoRA Scales** changes a scale without restarting; 0 turns an adapter off. |
| **Extra raw CLI parameters** (Other Parameters) | llama-server flags that have no field in the interface. Do not force flags the runtime does not support. |

**Sampling** (temperature, Top P, and so on) is set per request; edit it in the Playground's session settings or the preset manager. To prepare for coding agents, see [the flow in Connections](05-connections-and-gateway.md#connect-a-coding-agent).

### Model Definition and the Save Bar

The selected **Model definition** (see [My Models](02-model-hub-and-library.md#my-models-and-model-definitions)) carries the starting values for these settings. When you change the form, the bar at the bottom says there are unsaved changes. Starting a server does not save them. The bar offers:

| Button | Effect |
| --- | --- |
| **Save as Default** | Writes the changes into the selected definition. Profiles that use the definition are affected too. |
| **Save as New Definition** | Creates a separate definition (a new alias) with your changes. |
| **Undo** | Resets the form to the definition's saved values. |

## When Memory Runs Out (OOM)

If a model does not fit in memory, the server stops and the **The model did not fit in memory** dialog opens (error text: "The GPU ran out of memory; the server was stopped.").

1. In the dialog, read the **Free VRAM** and estimated need line and what did not fit: the KV cache, the model weights, or the compute buffers.
2. Look at the option marked **Recommended**. Possible suggestions are: let llama-server choose the GPU layers, lower the context, quantize the KV cache, use fewer GPU layers, keep MoE expert weights on the CPU, use a smaller physical batch, or stop another server that is using the GPU.
3. Press **Retry with This** next to your choice.
4. If the retry works, check with a new chat. To keep the values, tick **Save these values to the model definition**; when it is off, the values apply to this start only.
5. If there is no option ("No automatic option for this failure. Check the logs."), use **Open logs** to inspect the error and pick a smaller model or a smaller quant (for example Q4_K_M).

Reducing GPU layers increases RAM needs. If the app warns that system memory may not be enough, take it seriously. Recovery options are for memory errors during loading. For servers that stop unexpectedly while running, **Settings > Network > Listening** has **Restart automatically after a crash** (load failures are not retried).

## Servers and Profiles

The **Servers** page (an Advanced page) shows each server's status (**Running**, **Starting**, **Stopping**, **Stopped**, **Error**), its model, address, port, and whether it has an API key. Each row offers **Start**, **Stop**, **Restart**, **Open in Dashboard**, and **Open in Playground**; a stopped instance can be taken off the list with **Remove**.

A profile is a named record of a model and a set of run settings (port included). A profile starts on its own server instance with one click, so different models, for example one for chat and one for code, can run at the same time.

1. Prepare the model and settings you want in the Dashboard.
2. Press **Save as Server**, enter a **Profile name**, and save.
3. Next time, start the profile from the **Servers** page.
4. When you change a profile's settings, use **Save Changes** on the Dashboard.
5. If you want, turn on **Start automatically when the app launches**. It takes effect the next time Harness Studio starts.

**New Server** creates a new server draft with the same settings on the next free port; edit the model and settings before you start it. Manage profiles with **Rename**, **Duplicate**, and **Delete**; you cannot delete a running profile, so stop it first. The memory needs of servers running at the same time add up, and they may not all fit on the graphics card. If you give two servers the same port you get a warning: "This port is currently used by the "{label}" server. Pick another port before starting."

## Using the Logs

**Server Logs** at the lower right of the Dashboard shows loading steps and error lines live. Use search and filters to find a problem, and turn off **Auto-scroll** to read lines at your own pace. **Export** opens the **Export log** window; use **Save...** to save the output for support. Review the content before you share it. To see which port a running server actually uses, read it from the address shown in the app.

For advanced settings and limits, see the [Features reference](../../en/FEATURES.md) and the [security model](../../en/SECURITY_MODEL.md).

Next: [Playground](04-playground.md).
