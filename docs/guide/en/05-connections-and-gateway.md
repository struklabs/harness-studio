# Connections and the Gateway

Türkçe: [../tr/05-baglantilar-ve-ag-gecidi.md](../tr/05-baglantilar-ve-ag-gecidi.md)

**What you do here:** serve your local models from one address and connect another application or a coding agent to it.

## Direct Server or Gateway?

Another program (Claude Code, Codex, OpenCode, an editor extension, Open WebUI, and so on) can reach your local model in two ways:

| Route | How it works | When to use it |
| --- | --- | --- |
| Direct server | The own address of a single llama-server that you started from the Dashboard (`http://127.0.0.1:<port>`). With an Automatic port it can change on every start. | One model, a quick try. |
| Gateway | A single endpoint that runs inside Harness Studio. The `model` field of a request is routed to the right server by **Alias**; if the model is not loaded it loads on the first request, and it can be unloaded when idle. | Several models, a fixed address, agents. |

The big advantage of the gateway is one fixed address: even if you change the model or the port, the address in your client stays the same. The gateway can also serve models that run on the optional engines (speech, image, ONNX, Lemonade, mistral.rs) next to llama.cpp models, through the same address.

For a model loaded with an audio projector, `/v1/chat/completions` accepts OpenAI `input_audio` message parts (`{"type": "input_audio", "input_audio": {"data": "<base64>", "format": "wav"}}`, WAV or MP3), streamed or not. To make the answer speech, send its text to `/v1/audio/speech` with an installed text-to-speech model.

### Turn the Gateway on

1. Check that a [model definition exists in My Models](02-model-hub-and-library.md#my-models-and-model-definitions); the name your client uses is the definition's alias.
2. Open **Settings > Gateway > Endpoint**.
3. Enable **Turn on the Gateway** and press **Apply**. The status should say **Running**.
4. Enter the address from **Base URL** (it has a **Copy** button) in your client. Use the definition's alias as the model name.

The gateway is off by default and stops when Harness Studio closes. The Playground's Speech and Image tabs and the integrations use the gateway too.

The default port is `11435` and the address is `127.0.0.1`, meaning only your own computer. If another program is using the port, the gateway moves to a free port; the **Connections** page (**Endpoints > Port Notes**) shows the current address and which clients need to be moved. Always go by the address shown on screen.

| Client type | Default address format |
| --- | --- |
| OpenAI-compatible (most tools, editor extensions, SDKs) | `http://127.0.0.1:11435/v1` |
| Anthropic-compatible (Claude Code, the Anthropic SDK) | `http://127.0.0.1:11435`, without `/v1` at the end |
| Ollama-compatible | The gateway's `/api/*` paths; the setting is on **Settings > Runtime > Ollama**, which also has an option to listen on Ollama's own port as well |

The local gateway does not ask for a key by default. For tools that insist on one, any value will do (Harness Studio uses the placeholder `harness-local`).

### Gateway Settings

| Page | What it does |
| --- | --- |
| **Settings > Gateway > Endpoint** | Turn it on or off, **Port**, **Bind Address**, **Allow Network Access**, **Require a Token on This Computer Too**, **Allowed Browser Origins** (for web apps). |
| **Settings > Gateway > Routing** | **Max loaded models**, **Queue per Model**, **Request Timeout (s)**, **Default Idle Time (s)**, **Memory-aware Eviction**, **VRAM Headroom (MiB, 0 = Automatic)**, **Chat Model Emulation for Decision Endpoints**; below them, each model's state (**Ready**, **Loading**, **Not loaded**) and an **Unload** button. |
| **Settings > Gateway > Responses** | Whether the OpenAI Responses API keeps responses locally (**Store Responses**) and the retention limits. |

**Default Idle Time (s)** applies to models without their own idle time; empty means never unload for idleness. With a compatible stable-diffusion.cpp build and automatic fitting enabled, the gateway can admit an image model with part of its weights in RAM: the GPU must hold the compute buffers plus headroom, and the remaining weights must fit within 90% of free RAM. An older build, disabled auto-fit, or explicit weight placement requires the full GPU estimate to fit.

The gateway unloads only the models it loaded itself (**Loaded by gateway**) according to idle time and memory rules. It routes to servers you started by hand (**Your server**) but never shuts them down. If you mark a model definition **Pinned (always loaded)**, the gateway loads it when it starts and never unloads it.

If a request is rejected because a model is loading or a queue is full, the Playground shows a message like "{model} cannot be loaded right now: {busy} is serving requests and cannot be unloaded. {wait} try again."; once the requests finish, resend with **Retry** (see [Troubleshooting](10-troubleshooting.md#model-busy-503)).

For detailed routing rules, per-client settings, and limits, read the [integrations reference](../../en/INTEGRATIONS.md).

### Decision Models

A decision model reads a text (the *state*) and answers typed questions with probabilities instead of a written reply: yes or no, one option out of a list, or a level on a scale. The gateway serves two request formats for this, with the same token, browser origin, and automatic loading rules as the other routes:

| Path | Format | Question types | Errors |
| --- | --- | --- | --- |
| `/v1/systemone` | Ollama, LM Studio, llama.cpp: `state`, `images`, `questions` as an object keyed by name, `keep_alive` | `noul`, `choice`, `score` | `{"error": "..."}`, like Ollama |
| `/v1/decisions` | OpenAI, LM Studio: `input`, `questions` as an array; answers in question order | `predicate`, `choice`, `score` | OpenAI's error object |

```bash
curl http://127.0.0.1:11435/v1/systemone -H "Content-Type: application/json" -d '{
  "model": "my-model",
  "state": "Customer message: I was charged twice and nobody has replied.",
  "questions": {
    "route": {"type": "choice", "instructions": "Which team should handle this?",
      "criteria": {"billing": null, "shipping": null, "technical": null}},
    "angry": {"type": "noul", "instructions": "Is the customer angry?"},
    "urgency": {"type": "score", "instructions": "How urgent is this?",
      "criteria": ["can wait", "this week", "today", "right now"]}
  }
}'
```

- A model with decision metadata in its file (labelled **Decision model** in My Models) is answered by llama.cpp itself.
- Any other chat model answers through emulation: the gateway asks one question at a time, lets the model write only one of the option codes, and reads the probabilities from the model's token probabilities. **These probabilities are not calibrated**: use them to compare the options, not as a measured accuracy. Turn this off with **Settings > Gateway > Routing > Chat model emulation for decision endpoints**; then only decision models answer, as in Ollama and LM Studio, and a chat model gets a 400 error.
- The `X-Harness-Decision-Mode` response header says which one answered: `native` or `emulated`.
- Limits: up to 64 questions per request, 2–255 options per choice question (at most 52 with emulation), 2–10 score levels, no streaming. Images are base64 (a data URL, or bare base64 on `/v1/systemone`) and need a model that reads images.

### Task Endpoints

Classifiers, entity taggers, zero-shot models and object detectors (Hugging Face task models served by the ONNX or Python engine) have typed routes next to the generic `POST /v1/hf/{model}`. All of them take a JSON body with the `model` alias, answer the same way whichever engine runs the model, and use the **Task pipelines** endpoint group of a token.

| Path | Model task | Request | Answer |
| --- | --- | --- | --- |
| `/v1/classifications` | Text classification | `input` (a text or an array of up to 256 texts), optional `top_k` | `data[i].labels`: `label` and `score`, best first |
| `/v1/entities` | Token classification (NER) | `input`, optional `aggregation_strategy` (`simple` by default) | `data[i].entities`: `label`, `text`, `score`, `start`, `end` |
| `/v1/zero-shot` | Zero-shot classification | `input` (one text), `candidate_labels` (array or comma-separated), optional `multi_label`, `hypothesis_template` | `data[0].labels`, best first |
| `/v1/detections` | Object detection | `image` (a data URL or base64; URLs are not fetched), optional `threshold` | `data`: `label`, `score`, `box` (`xmin`, `ymin`, `xmax`, `ymax`) |
| `/v1/moderations` | Text classification by a moderation model | `input` (a text or an array), optional `threshold` (0.5 by default) | OpenAI's shape: `results[i].flagged`, `categories`, `category_scores` |

`/v1/moderations` needs a toxicity or moderation classifier in your library; name it in `model` (there is no default, so OpenAI's own model names are refused as unknown). A label that means "nothing to flag" (`non-toxic`, `neutral`, `OK`) is never a category. Labels of the common models are mapped to OpenAI's category names (`insult` to `harassment`, `identity_hate` to `hate`, `obscene` to `sexual`, `threat` to `harassment/threatening`); any other label keeps its own name, so `categories` lists what the model can tell, not OpenAI's whole set. A category is flagged when its score reaches the threshold.

```bash
curl http://127.0.0.1:11435/v1/entities -H "Content-Type: application/json" -d '{
  "model": "my-ner", "input": "Ada Lovelace worked with Charles Babbage in London."
}'
```

## The Connections Page

The sections of **Connections** in the left menu:

| Group | Pages | What they hold |
| --- | --- | --- |
| **Tools and Clients** | **Automatic Setup**, **Step-by-Step Setup**, **SDKs and Code Samples**, **Add Harness MCP** | Connecting coding tools and code snippets. |
| **Harness Link** | **Harness Link** | Remote Harness models and connection status. |
| **Cloud Providers** | **API Keys** | Cloud providers you use with your own API key, their models, and connection tests. |
| **Endpoints** | **Endpoint List**, **Ollama Endpoint**, **Port Notes** | Addresses of the servers and the gateway; copyable. |
| **MCP Servers** | **Server List**, import, **Status and Live Tools** | Servers that Harness Studio connects to with its own MCP client. |
| **Test** | **Tool-Calling Workbench**, **Connection Tests** | Connection and tool-calling trials. |

In **Simple**, open **Connect an App**: **Automatic Setup**, **Step-by-Step Setup**, **Harness Link**, **API Keys**, and **Endpoint List** are visible. A fresh visit starts on Automatic Setup. In **Advanced**, the screen is called **Connections** and all groups are available; gateway settings are Advanced pages that links can also open from Simple. The section groups follow this order: Tools and Clients, Harness Link, Cloud Providers, Endpoints, MCP Servers, Test.

## Use a Cloud Provider With Your Own Key

On **Connections > Cloud Providers > API Keys**, press **Add Provider** and pick OpenAI, Gemini, Anthropic, OpenRouter, or **Custom (OpenAI-Compatible)**. Enter your API key; leave **Base URL** blank to use the provider's default address. A custom endpoint needs its base URL (for example `http://localhost:8000/v1`) and, if the server asks for none, no key.

- The key is stored only in this device's system keychain (macOS Keychain, Windows Credential Manager, or Secret Service on Linux). It is never written to settings files or logs. **Remove Saved Key** in **Edit Provider** deletes it; **Remove** deletes the provider and its key.
- **Test Connection** reads the provider's model list with the key and shows the state, the response time, and, for OpenRouter, the credits used. Under **Models**, each model shows what the provider's API reports: context length, price per million tokens, tool calling, and image input. Most providers report only some of these.
- **Test Model** sends one small chat request with a tool to the selected model. The provider may charge for it.
- HTTPS is required, except for a server on this computer or the local network.

With the gateway on, the provider's chat models appear in the Playground model picker under **Cloud: OpenAI** (or the provider you added), with IDs such as `cloud:openai/gpt-4o`. Requests go through the gateway to the provider, with streaming and tool calling where the provider supports them. Any client connected to the gateway can use these IDs too. Prompts sent to a cloud model leave your computer.

Which endpoints a cloud model accepts depends on the provider:

| Endpoint | OpenAI | Anthropic | Gemini | OpenRouter and Custom |
| --- | --- | --- | --- | --- |
| Chat completions (`/v1/chat/completions`) | Yes | Yes (translated to Anthropic Messages) | Yes (its OpenAI-compatible endpoint) | Yes |
| Responses (`/v1/responses`) | Yes | No | No | Yes |
| Messages (`/v1/messages`, `/v1/messages/count_tokens`) | No | Yes (sent unchanged) | No | No |
| Embeddings (`/v1/embeddings`) | Yes | No | Yes | Yes |

An endpoint the provider does not serve is refused with a clear error. Streaming and tool calls work on every endpoint a provider serves, and the provider's own errors (such as 429 with its `Retry-After`) reach your client. A rejected key is reported as a gateway error that never repeats the key.

Embedding models (such as `cloud:openai/text-embedding-3-small`) are listed apart from the chat models. In a knowledge base you can pick one as the embedding model instead of a local model; the index is marked with that model ID, and changing the model re-embeds the documents. Document text is sent to the provider while it is indexed.

The Control API manages providers too (admin scope): `GET/POST /cloud/providers`, `PUT/DELETE /cloud/providers/{id}`, `POST /cloud/providers/{id}/test` and `GET /cloud/models`. The key is accepted when you add or update a provider and is never returned.

## Connect a Coding Agent

An in-app launch flow exists for Claude Code, Codex, OpenCode, Aider, and Goose. The client itself must be installed on your computer; installed tools are marked **Detected**, and **Rescan** refreshes the list.

1. On **Connections > Tools and Clients > Automatic Setup**, pick the client in **Tool Integrations**.
2. Check the model and the connection address. If the gateway is on, the address is the gateway; otherwise it is the selected server's direct address. Use a model that can call tools, and read the **Context too small** warning if it appears. Coding agents send large prompts: 64k of context is recommended for Claude Code, Codex, OpenCode, and Goose, and 32k for Aider.
3. Press **Launch**. In the window that opens, choose the **Project folder** and the **Model**.
4. If the context is too small, look at the **Prepare Server** options. **Prepare and Load** writes the context, Flash Attention, KV cache, and concurrent-request values into the model definition and stops running instances; read the explanation on screen. If memory is not enough, the earlier settings are restored.
5. Press **Launch** to open the agent. This route gives the settings only to that session's environment variables and a temporary file, and opens a new terminal window; it does not change the tool's global configuration. If the platform cannot open a terminal, run the **Command-line equivalent** yourself.

For a lasting setup, use the **Configure** button on supported clients. Harness Studio changes only its own keys in the tool's settings file and first shows a colored diff preview; read the preview and then apply it. **Revert** undoes only keys Harness Studio wrote and that you have not changed since. For clients set up by hand (the **Step-by-Step Setup** page), use the instructions and copyable fields. If you rename a model, the card shows "The tool uses an old model name" with a **Fix** button.

A summary for three common clients (details and full environment variables are in the [integrations reference](../../en/INTEGRATIONS.md)):

| Client | What it needs |
| --- | --- |
| Claude Code | The Anthropic-compatible address (`http://127.0.0.1:11435`, no `/v1`) and a model name. **Launch** or **Configure** writes this to `ANTHROPIC_BASE_URL` and the related model variables. |
| Codex | The gateway address through the OpenAI Responses API. Codex speaks only the Responses API; the settings are added as a `harness` provider. |
| OpenCode | The OpenAI-compatible address (`http://127.0.0.1:11435/v1`). **Configure** writes the `harness` provider and the gateway's chat models into `opencode.json`. |

Use **Test Connection** to try the client's own API format. A passing test does not mean your real agent will call the right tool on every task. Small local models can make tool-calling mistakes; the **Tool-Calling Workbench** lets you try this beforehand. After providing a tool result, use **Generate Answer** to request the final response. Client-specific limitations and test records are in the [integrations reference](../../en/INTEGRATIONS.md).

## Keep API Keys Apart

| Credential | What it is for |
| --- | --- |
| Hugging Face access token | Access to the model repository and downloads. It is not the chat server's key. |
| Server API key | Protects the direct llama-server connection. Managed on the Dashboard's **Network** page in the **API key** field or in **Settings > Security > API Keys**. |
| Named token | Scoped access given to the Control API or the gateway. Created in **Settings > Developer > Control API**. A model request needs the `inference` scope. |
| Cloud provider API key | Your own key for OpenAI, Gemini, Anthropic, OpenRouter, or a custom endpoint. Saved in **Connections > Cloud Providers**; the gateway sends it only to that provider. |

If **Require a Token on This Computer Too** is off on the gateway, local clients connect without a key. If it is on, use a named token with the `inference` scope. A named token's value is shown only when it is created, so copy it then.

For other devices to reach the gateway you need **Allow Network Access** and a token with the `inference` scope; there is no keyless network access. For this use, follow the conditions in the [security model](../../en/SECURITY_MODEL.md) and set your firewall yourself; Harness Studio does not change it. Do not use the gateway's, the Control API's, and the direct server's keys in place of one another.

Next: [Tools](06-tools.md).

### Document OCR and Questions

`POST /v1/ocr` and `POST /v1/document/qa` use the gateway's **pipelines** endpoint
policy and the **inference** token scope. Both accept JSON with a local `model` alias
and either `document` (a base64 data URL) or `pages` (1?16 image data URLs in reading
order). Remote URLs are not fetched. Supported images are PNG, JPEG, WebP and GIF;
`document` also accepts `data:application/pdf;base64,...`. The decoded total is limited
to 20 MiB, within the gateway's 64 MiB request body limit.

```json
{"model":"my-vision-model","document":"data:image/png;base64,..."}
```

OCR returns `{ "model": "...", "markdown": "...", "pages": [...] }`. Each page has a
one-based `page`, `markdown`, `lines`, `blocks` (paragraph strings in reading order)
and `source` (`vlm`, `pipeline` or `pdf_text`). These are simple text layouts, without
bounding boxes. VLMs use a fixed transcription prompt; Python `image-to-text` models
use the existing Hugging Face pipeline. Captioning models may describe rather than
transcribe: choose an OCR-trained model for reliable extraction.

For QA, add a non-empty `question` (at most 8192 UTF-8 bytes):

```json
{"model":"my-vision-model","document":"data:image/png;base64,...","question":"What is the total?"}
```

QA returns `{ "model": "...", "answer": "..." }` and includes `score` only when the
Python `document-question-answering` pipeline supplies one. VLMs use chat and support
both routes with a configured projector. Python DQA accepts one image per question.
A Python OCR model and a DQA model can be selected separately for the two calls.

PDF pages with a text layer are extracted locally, page by page, without inference.
Pages without one (scans) are rendered to PNG at 150 dpi by the Python engine's
rasterizer (the `python-hf` environment must be installed; otherwise the call returns
HTTP 503 with `rasterizer_unavailable` and an install hint) and read by the chosen model;
their `source` is `vlm` or `pipeline`, the others stay `pdf_text`. Pages keep their PDF
page numbers. For QA with a VLM, text-layer pages are sent as text and scanned pages as
images. At most 16 PDF pages are accepted per request (`HARNESS_OCR_MAX_PAGES`, up to
1000, and `HARNESS_OCR_DPI`, 72 to 300, change the limits); a rendered page may be
12 MiB and all rendered pages together 20 MiB. A Python DQA pipeline still takes one
page image per question, not a PDF.

Invalid inputs or task mismatches return 400, decoded uploads over 20 MiB return 413,
token policy denials return 403, and inference limits return 429. OpenAPI definitions
for these gateway-listener routes are included in `sdk/openapi.json`.
