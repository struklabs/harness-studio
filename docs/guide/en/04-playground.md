# Playground

Türkçe: [../tr/04-playground.md](../tr/04-playground.md)

**What you do here:** chat with a local model, organize chats, use documents and tools, compare two answers, work with embeddings and reranking, turn speech into text and text into speech, generate images, and group work into projects.

## How the Screen Is Laid Out

At the top of the Playground are the mode tabs (**Playground mode**): **Chat**, **Embedding**, **Rerank**, **Speech**, **Image Generation**, and **Tasks**. In the **Simple** interface level you see **Chat**, **Speech**, and **Image Generation**; **Embedding**, **Rerank**, and **Tasks** appear in **Advanced**. In Chat mode, the chat list is on the left (**Show chats**), the conversation and the **Your message** box are in the middle, and the **Session Settings** panel (**Show settings**) is on the right. Session Settings is split into these pages:

| Group | Page | Purpose |
| --- | --- | --- |
| Session | **Model and Preset** | Choose the model and a preset (a saved set of settings). |
| Session | **Assistant Instructions** (Simple) / **System Prompt** (Advanced) | The general instruction given to the model. |
| Generation (Advanced) | **Sampling**, **Output Limits** | Creativity and answer length. |
| Context (Advanced) | **History and Compaction**, **RAG** | Summarizing long chats and searching documents. |
| Tools | **Selection and Permissions** | Which tools the model may use, and the working folder. |

Open this screen as **Chat and Media** in Simple or **Playground** in Advanced. **Advanced Settings** reveals Sampling, Output Limits, History and Compaction, and RAG controls. Comparison and audio processing are Advanced; optional speech and image controls can be expanded locally under **Advanced Settings**. An already active Advanced tab stays visible when switching to Simple, and active comparison can still be turned off.

## Your First Chat

1. [Start a chat server from the Dashboard](03-dashboard-and-servers.md), or turn on the gateway and pick a model definition.
2. Open **Playground > Chat**. Check the model picker. If you use the gateway, the model may load on the first request, which takes anywhere from a few seconds to a few minutes.
3. Type your request in **Your message** and press **Send**.
4. Use **Stop** to cut generation short.
5. Under the answer, check the token count, generation speed, and time to first token. A token is a word or a piece of a word.

If you see **Server is not running**, use **Go to Dashboard** to check the server. A server started in embedding or rerank mode does not answer chat; use the **Open the Embedding Tab** / **Open the Rerank Tab** links in the banner.

The message box also has an **Attach Image** button (PNG, JPEG, WebP, and GIF; you can drag or paste images too) for vision models, which need their projector (mmproj) file loaded, a **Dictation** button for speaking instead of typing, and a **Web and Tools** switch that offers web search and your selected tools to the model.

## Chat List and Folders

Sending your first message creates the chat record, which is stored in a local database. Open the list with **Show chats**; find an old conversation with **Search chats**.

1. **New Chat** starts a separate conversation.
2. From a chat's action menu, choose **Rename**, **Pin**, **Add to favorites**, or **Duplicate**.
3. **New Folder** creates a folder for organizing chats; place a chat with **Move to Folder** (or drag it onto a folder).
4. **Export** saves JSON or Markdown. **Import Chat (JSON)** adds a saved conversation.

A chat folder is not a working folder for file tools (see [Use Tools and Review Approvals](#use-tools-and-review-approvals)). Deleting a chat folder does not delete the chats inside; deleting a chat with **Delete** removes it permanently along with its branches and answer versions.

To get a different answer, choose **Regenerate Response**; this works for an answer anywhere in the history. The new answer becomes a new version of that message, later messages stay with the old version, and the **< 1/3 >** arrows show each version's own continuation. To change an earlier message of yours, use the **Edit and Resend** icon: the text is edited in place, and when you save, the answer is regenerated from there, with the old continuation kept as a side branch. **Fork From Here** copies the history up to that message into a new chat.

Answers render full Markdown (lists, tables, headings), and `mermaid` code blocks appear as diagrams you can switch to source, copy, or save as SVG. **Read Aloud** on an answer reads it out (see [Text to Speech](#text-to-speech)).

## Tables, Charts, and UI Blocks

Structured JSON tool results appear as key-value cards or sortable tables; **Raw JSON** shows the underlying result. Markdown tables scroll horizontally and offer **Copy as CSV**. A complete `chart` block can show a bar, line, or pie chart. A complete `ui` block can show a card, list, steps, callout, progress bar, or key-value view. These use a limited declarative JSON schema: model HTML and JavaScript are never executed. Invalid or incomplete blocks remain readable code. See the [supported schemas](../../en/FEATURES.md).

## System Prompt, Presets, and Sampling

1. Open the **Session Settings** panel with **Show settings**.
2. On **Assistant Instructions** (Simple) or **System Prompt** (Advanced), write the general instruction you want the model to follow. Example: "Answer in short sentences."
3. On the **Sampling** page you can edit **Temperature**, **Top P**, **Top K**, **Min P**, **Repeat penalty**, and **Seed**. Use the defaults if you do not need to change them; empty fields are not sent with the request.
4. On the **Output Limits** page, set the upper limit of the answer with **Max tokens**. Watch the **Context usage** indicator so there is room left for the output.

What they do: as **Temperature** rises, answers become more varied and unpredictable; as it falls they become consistent and repetitive. **Top P**, **Top K**, and **Min P** narrow which word candidates the model chooses from. **Repeat penalty** reduces repeated phrases. With a fixed **Seed**, the same prompt gives a similar answer.

A **Preset** collects these request parameters (sampling, penalties, reasoning, output limit) and an optional system prompt under one name. Pick it from the **Preset** selector on the **Model and Preset** page. Built-in presets cannot be changed; use **Copy** to duplicate and edit one. **Manage Presets** creates new presets and imports or exports them. If you assign a **Default preset** to a model definition, new chats opened with that model start with it selected.

To see how confident the model was in each word, turn on **Token probabilities** in a preset's sampling settings (you can also set how many **Alternatives per token** to return). Answers then offer **Show token probabilities**: each token is tinted (faint means confident, strong means uncertain) and hovering shows the chosen token and its alternatives. **Back to normal view** returns to plain text. Probabilities are kept for the current session only.

When the context fills, the model may forget old messages or the request may be rejected. Start a new chat or use context compression on the **History and Compaction** page: when a chat gets close to filling the context, older messages are replaced by a summary the model writes, recent turns stay as they are, the original messages are not deleted, and compaction can be undone with **Undo**. You can also compact by hand by typing `/compact` in the message box; see the [Features](../../en/FEATURES.md). For a bigger context, change the server's load settings, which needs more memory.

## Compare Two Answers

Comparison sends the same message to two models, or to two presets of the same model, and shows the answers side by side.

1. Open **Compare** (shortcut: Ctrl+Shift+M).
2. Choose the models or presets for A and B. Gateway models (including ones that load on first request) and running servers appear in separate groups.
3. Send the same message. You see the answers, times, word counts, similarity ratio, and speed difference; **Show Differences** marks word-level differences.
4. If two models do not fit in memory together, or the gateway keeps only one model at a time, the app switches to running them in turn by itself and tells you why. To force it, turn on **Run sequentially**.
5. Press **Continue with this answer ({side})** to carry on with the side you picked; `{side}` shows A or B. The comparison closes and the other side stays as an answer version.

Document search is not used during a comparison. If tools are on, each side runs its own tool loop; review the permissions for each call separately.

## Use Tools and Review Approvals

A tool is something the model does besides generating text, such as reading a file, running a command, or searching the web. Tools ask for approval by default.

1. On **Session Settings > Selection and Permissions**, turn on **Enable tool use**.
2. Choose which tools to offer the model (**All tools in the group** toggles a whole group at once). The model's chat template must support tool calling; a model that does not support it will not call tools.
3. For file or command tools, set a folder under **Working folder** with **Choose Folder** (the folder button in the input does the same). This choice belongs to that chat. You can also use the **Default folder** in **Settings > Tools > Built-In Tools**. File tools work only inside this folder; they cannot reach a parent folder, an absolute path, or a symbolic link. The command tool (`exec_shell_command`) starts its command in this folder but is not limited to it: a command can do anything on your computer that your user account can (it is in the dangerous class). Read the command carefully before you approve it. Without a folder, file and command tools return an error.
4. When the model calls a tool, read the **Parameters** on the tool card. Press **Approve** to accept the action or **Deny** to refuse it.

Approval lets the tool run on your computer. For file writes and commands, check the target and the content. Set the policy and trusted tools on the [Tools chapter](06-tools.md). Network tools send their queries or requests to outside services.

## Add Documents

1. Pick a PDF, DOCX, Markdown, text, or code file with **Add document**, or drag a file into the chat.
2. Wait for reading and indexing to finish (**reading**, with a chunk counter).
3. Send a concrete question about the document.
4. Check the quotes and page information under **Sources of the answer**; the quote the answer cites is marked.

The **Use** choice next to a document decides how it is used. With **Auto**, a document that fits comfortably in the context is added as full text, and for large documents the relevant parts are searched for on every question. You can change this with **Full Text** and **Search**. If no **Embedding model** is selected on **Session Settings > RAG**, only keyword search is used; for semantic search, download a small embedding model (for example nomic-embed-text) from Model Hub. Optional **Reranking** orders results more accurately with a reranker model at a small cost in speed. **Chunks per question** is the number of quotes added to the answer in search mode. Document processing and indexes run locally. Document search is not used in the comparison view.

## Knowledge Bases

A knowledge base is a named set of documents you can reuse in any chat.

1. Open **Knowledge Bases** above the message box and choose **New from Folder** (supported files in the folder and its subfolders; hidden, build, and dependency folders are skipped) or **New from Files**.
2. If no embedding model is installed, **Install Embedding Model** downloads a small multilingual one (about 126 MB). It runs on the CPU, so indexing does not keep your GPU busy. Make sure the gateway is on.
3. Watch the progress under the base; **Stop** cancels it and **Resume** continues later. When indexing ends, the base shows how long it took.
4. Choose **Use in This Chat**. On every question, only the best matching passages are added (a quarter of the context at most, 2,000 tokens at the very most), and the answer shows them as source chips.

A base remembers which embedding model built its index. If you switch to another embedding model, the base is marked **Stale**; **Rebuild** re-reads and re-indexes it. Until then, searches fall back to keywords and a notice says why. Deleting a base removes its index only, never your files.

## Embedding and Rerank

**Embedding** turns text into a numeric vector, used for search and similarity. Start a server with a suitable embedding model (a server whose model type is embedding starts in this mode on its own), fill in the **Texts** boxes on the **Embedding** tab, and press **Get Embeddings**. A similarity matrix needs at least two non-empty texts.

**Rerank** orders documents by how well they match a question. Start a suitable reranker model, fill in **Query** and **Documents**, and press **Rank**. Compare scores only among the documents of the same query.

## Speech: Speech to Text and Text to Speech

The **Speech** tab covers both directions. Audio goes through the gateway; if it is off, use **Turn the Gateway On**. Models load on the first request.

### Speech to Text

1. In **Model Hub**, search for `whisper` and download a Whisper ggml model. A `ggml-*.bin` file that is already in one of your folders is listed in the **Model** list on its own. For Turkish or other languages, do not pick a model that only knows English (a `.en` model).
2. Install the whisper.cpp engine: press the **Install whisper.cpp** button shown on the library card, in Model Hub, or in this tab's error message. You can also install it from the **Other Runtimes** list in **Settings > Runtime > Installed Runtime**. On macOS there is no ready-made whisper server package; you can add the folder of a `whisper-server` you built yourself.
3. Open the **Playground > Speech** tab.
4. Choose a **Model**. Add audio with **Choose File**, or start a microphone recording with **Record** and end it with **Stop**. The operating system may ask for microphone permission.
5. You can type a language code such as `tr` into **Language** or leave it automatic. For **Task**, choose **Transcribe** or **Translate to English**, then press **Transcribe**.
6. Look at the result. **Add to Chat** puts the text in the chat draft; you still have to press **Send** to send it.

Besides Whisper, the **Recommended sherpa-onnx Recognition Models** section lets you download NVIDIA Parakeet (25 European languages), SenseVoice (Chinese, English, Japanese, Korean, Cantonese), and Omnilingual (1600 languages, Turkish included). Press **Install Engine** first to install the sherpa-onnx engine. Long recordings are cut at pauses and read as timed segments, and the chat **Dictation** button uses the same model choice. These models only transcribe (**Translate to English** is a Whisper task), and Omnilingual output has no punctuation or capital letters. For these models only 16-bit mono WAV is read directly; other formats, MP3 included, need ffmpeg.

WAV, MP3, FLAC, and Ogg are read directly. Formats such as M4A, WebM, and Opus need ffmpeg. Microphone recording produces 16 kHz WAV. If microphone permission is denied, allow Harness Studio in your system privacy settings.

ONNX CTC models (wav2vec2, HuBERT, WavLM) from Model Hub or library folders also appear in the model list. Use **Open in Playground** on the library card to select one. They use the managed ONNX Runtime engine and only transcribe: language and prompt fields are hidden, and translation is unavailable. Their output has no timed segments.

### Text to Speech

The **Text to Speech** section of the **Speech** tab turns text into audio. Press **Install Engine** to install the sherpa-onnx engine (about 20 MB, verified), then download a voice under **Recommended Voice Models**: Kokoro (multilingual) and Piper voices, which are recommended for Turkish. Choose a **Model**, a **Voice**, and a **Speed**, type the **Text to read**, and press **Speak**. The audio plays in the app and can be downloaded. Once a voice model is installed, the **Read Aloud** button on a chat answer reads it with that voice; without a voice model the app points you to this section.

### Process Audio (Experimental)

The **Process Audio** section separates the sounds in a recording or improves its quality. It runs on the experimental audio.cpp engine, which is off by default.

1. Press **Enable Engine**. In **Settings > Runtime > Installed Runtime**, install the engine with **Install audio.cpp**, turn on **Experimental audio.cpp Engine**, and confirm the warning.
2. In **Model Hub**, open the `audio-cpp/audio.cpp-gguf` repository and download a model marked **Experimental**. For example, choose BS-RoFormer to separate vocals or Apollo to improve a recording.
3. Choose a **Model**. Add a WAV file with **Choose File**, or record from the microphone with **Record**.
4. Fill in any extra settings the model asks for. SAM Audio, for example, asks you to describe the sound to isolate in a sentence. Then press **Process Audio**.
5. Listen to each output channel in the app (for example **Vocals** and **Instrumental**) and save it to your computer with **Save**.

Only WAV is accepted: up to 32 MiB, 10 minutes, and two channels. Some models need 44.1 kHz; the panel says so and records the microphone at that rate. AudioSR runs only on a GPU backend.

## Generate Images

Safetensors chat and vision models that run on the Python engine appear in the model picker like any other chat model, with **Python (Transformers)** shown as their engine. Chats stream, tool calls work when the model's chat template supports tools, and vision models accept pictures. The first message loads the model, which takes a while on a CPU.

The **Image Generation** tab generates pictures from diffusion models through stable-diffusion.cpp or the Python engine (diffusers). Download an image model from Model Hub (the extra files it needs, such as the VAE and text encoders, come with it), or use one that is already in your folders: it appears in the **Model** list on its own, and **Open in Playground** on its library card opens this tab with it selected. If the list stays empty although the file is there, some of its extra files are missing; the tab says so and install the **Image module (stable-diffusion.cpp)** when asked. Turn the gateway on, pick the **Model**, and write a **Prompt** (optionally a **Negative Prompt**). You can set **Size**, **Steps**, **Seed**, **Count**, and **CFG Scale**; an empty **Seed** chooses a random seed on each request, while the other empty fields use the model's own values. Press **Generate**: the model loads on the first request. Pictures appear under **Generated** and are kept only for this session; use **Save** to keep one.

For image-to-image, add an optional **Start Image** and set **Strength** (how much it changes). Models that only edit images require a start image. Python diffusers models also open in this tab.

### Generate Images with ComfyUI

If you already run [ComfyUI](https://github.com/comfyanonymous/ComfyUI), the **Image Generation** tab can use it instead of the built-in engine. Harness does not install, ship, or start ComfyUI; it only talks to the ComfyUI you run, through its HTTP API.

1. Start ComfyUI as usual (by default it listens on `http://127.0.0.1:8188`).
2. In **Image Generation**, switch from **Built-In** to **Image (ComfyUI)**. Enter the **ComfyUI Address** and press **Detect**. The address is saved when ComfyUI answers, and the version and device (for example `mps` on a Mac) are shown.
3. Pick a **Workflow**:
   - **Text to Image (Built-In)** is a simple checkpoint, KSampler, and Save Image graph. Choose its **Checkpoint**, or leave **First Checkpoint**.
   - **Saved Workflows** are the ones you saved in ComfyUI.
   - **Templates** are ComfyUI's own image templates and those of your custom nodes. Partner-API (paid) templates are not listed.
4. Write a **Prompt** and press **Generate in ComfyUI**. Optionally set a **Negative Prompt**, **Size**, **Steps**, **Seed**, or **CFG Scale** (under **Advanced Settings** in Simple mode); empty fields keep the workflow's own values, and an empty **Seed** is random. While it runs, the queue position and the sampler steps are shown, and **Cancel** removes the run from ComfyUI's queue or stops it.

While a project with a folder is active (see [Projects and Outputs](#projects-and-outputs)), the pictures are saved to its outputs; otherwise they go to `outputs/comfyui` in the Harness data folder. They are listed under **ComfyUI Outputs** with **Open** and **Show in Folder**. **Remove from List (Keep File)** leaves the file on disk.

Workflows saved in ComfyUI's editor format are converted with your ComfyUI's node definitions. A workflow that uses subgraphs or group nodes cannot be converted; in ComfyUI, export it in API format, save it, and pick that file. A missing custom node is named in the error message.
## Projects and Outputs

A project groups chats, the files they produce, and agent tasks under a name and a folder. Open **Projects** in the sidebar and press **New Project**.

1. Give it a **Name**, an optional description, and a **Project Folder** (an existing folder). Outputs are written to the `outputs` folder inside it.
2. **Extra Allowed Folders** (one per line) lets tools and agents use other folders too. Tools in the project's chats and its agent tasks work only inside the project folder and these folders; a file tool asked to work elsewhere is refused.
3. **Attached Tools**, **Attached Skills**, and **Attached MCP Servers** narrow what the project's chats offer. With nothing picked, the global settings apply.

Press **Use in Playground**, or pick the project in the list above the chats (**All Chats** shows every chat). While a project is active, new chats are filed into it, the chat list shows only its chats, generated images and speech are saved to its `outputs` folder, and files written by `write_file` or `edit_file` are recorded as outputs.

On the project page, **Outputs** lists each file with its source (chat, tool, image generation, speech, or agent), model, and time. **Open** opens it with the default app, **Show in Folder** reveals it, and **Remove from List (Keep File)** forgets it without touching the disk. **Agent Tasks** runs a Hermes Agent task that can read only the project's folders; **Save as Output** keeps its answer as a Markdown file.

**Archive** hides a project from the list and the Playground picker (**Show Archived** brings it back; **Unarchive** restores it). **Export as Zip** saves the project details, its chats as JSON, and its output files. **Delete Project** keeps the chats (they no longer belong to a project); tick the checkbox to also delete the output files in the project's folders from disk. The folders themselves always stay.

## Tasks (Advanced)

The **Tasks** tab runs ONNX and Python task models: choose a model, provide its text, image, or audio input, and run it. Results can include labels, captions, boxes, masks, or a depth map.

In a chat, **Thinking** controls the selected model’s thinking behavior for that conversation, with **Model Default**, **Off**, or the levels that model supports. Models with an audio-capable projector can accept an attached audio file or a microphone recording as chat input.

See also the [Tools reference](../../en/TOOLS.md) and the [Features reference](../../en/FEATURES.md).

Next: [Connections and the Gateway](05-connections-and-gateway.md).
