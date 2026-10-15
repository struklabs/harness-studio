Türkçe: ../tr/FEATURES.md

# Harness Studio: Features and Competitor Comparison

The single source of truth for what Harness Studio **actually has today**, what LM Studio and Ollama have, and how the remaining gaps will be closed. Plans and priorities are in [ROADMAP.md](ROADMAP.md); the target architecture is in [ARCHITECTURE.md](ARCHITECTURE.md).

Last audited: **October 2026** (competitor columns were re-verified against official documentation, and the Harness column was re-audited against `main`; the remaining gaps are on the [ROADMAP](ROADMAP.md)). Versions compared:
- Harness Studio `main` (0.1.0)
- LM Studio **0.4.26** (lmstudio.ai/download, 2026-10-12; 0.4.26 build 4)
- Ollama **0.40.3** (2026-10-11; MLX is the default on Apple Silicon since 0.40.0)
- llama-server **0.5.0-dev build 11236** (the `--help` audit is against this build; the §0 measurements use b11552/b11556)

Sources: a code audit of the repository, lmstudio.ai/docs and /changelog, docs.ollama.com + github.com/ollama/ollama/releases, llama.cpp `tools/server/README.md`, and the local `llama-server --help`.

`?` means a cell that could not be verified (the official documentation neither says it exists nor that it does not); details are in the [Footnotes](#footnotes) at the bottom.

**Status:** ✅ available · 🟡 partial · ❌ missing

**Measured comparison:** this table shows whether a feature exists. Measurements of speed, tool calling, context, concurrency, and memory with the same GGUF files are in [§0](#0-measured-comparison-2026-10-11) below, and in full in [COMPARISON.md](COMPARISON.md).

**Path:**
- **L**: can be done with llama.cpp's capabilities today
- **M**: needs an architectural change (a core crate, the gateway, a persistent store, etc.)
- **X**: specific to a competitor or not suitable for Harness

---

## 0. Measured Comparison (2026-10-11)

Windows 11, RX 7900 GRE 16 GB (Vulkan); Harness 0.1.0 (llama.cpp b11556), LM Studio (engine 2.51.0), Ollama 0.35.1, llama.cpp b11026/b11552. The models are Qwen3-4B-Instruct-2507 and Qwen2.5-Coder-14B-Instruct, Q4_K_M, the same file in every tool. Each tool uses its own defaults ("out of the box"); methodology and raw data are in [COMPARISON.md](COMPARISON.md) and [benchmarks/2026-10-11](../benchmarks/2026-10-11/summary.md).

| Measurement | Harness | LM Studio | Ollama | llama.cpp |
|---|---|---|---|---|
| 4B generation speed (tok/s) | 140.9 | 142.0 | 128.9 | 114.5 |
| 14B generation speed (tok/s) | 48.6 | 42.0 (repeated measurement 50.7) | 49.0 | 40.9 |
| 4B prompt processing, 4k tokens (tok/s) | 2460 | 2308 | **2997** | 1381 |
| 14B prompt processing, 4k tokens (tok/s) | 737 | 442 (repeated measurement 822) | **1109** | 402 |
| 4 concurrent requests, total (tok/s) | **344** | 157 | 126 | 152 |
| 14B tool calling (38 cases) | **36/38** | 5/38 | 4/38 | 5/38 |
| 4B tool calling (38 cases) | 38/38 | 38/38 | 38/38 | 38/38 |
| 20k-token prompt | correct | context error (8k) | context error (4k) | correct |
| Agent test, cached turn 2/3 first token (s) | **0.45 / 0.25** | context error | context error | 0.95 / 0.30 |
| 14B, 65k context: spill into shared memory (MiB) | **98** | 12376 | 10326 | 5208 |
| 14B, 65k context: generation (tok/s, short prompt) | 15.5 | **46.0** | 43.8 | 14.6 |
| Explicit setting that does not fit in memory (14B, 32k f16) | 503 with an explanation | loaded, spilled | loaded, spilled | loaded, spilled |
| Switching 4B → 14B on one endpoint, first token (s) | 6.5 | the wrong model answered | **5.6** | none (restart 22.9) |

Where the competitors lead: prompt processing (Ollama), short-prompt speed at 65k context on a card that is too small (LM Studio/Ollama, by spilling into shared memory), and maturity and ecosystem (the tables below). Limits: a single machine, AMD/Vulkan only; ~20% variation between runs on the 14B.

## 1. Model Discovery and Download

| Feature | Harness | LM Studio | Ollama | Path / note |
|---|---|---|---|---|
| HF search, sorting, task/license/author filters | ✅ | ✅ | ❌ (own catalog) | — |
| Discovery screen (Trending, Newly Added, Most Liked, Most Downloaded) and listing with filters only, no search | ✅ | 🟡 (featured) | ✅ (own catalog) | Cursor pagination, 5-minute session cache, last choices remembered |
| HF tasks (`pipeline_tag`, 35 tasks, with HF groups) and a per-task "runs here" marker | ✅ | ❌ | ❌ | The card for tasks that do not work says "Won’t run here" |
| Quant / size filter, smart search (`Q4_K_M` parsing) | ✅ | 🟡 | ❌ | — |
| Variant table + per-variant hardware fit | ✅ | ✅ | ❌ | — |
| Publisher / model avatar | ✅ | 🟡 | ✅ | **L**: HF `api/organizations/{x}/avatar` / `api/users/{x}/avatar`, letter+color fallback |
| Persistent download queue, pause/resume/prioritize, SHA-256 | ✅ | ✅ | ✅ | One concurrent job in Harness |
| Gated-model check, HF token (keychain) | ✅ | 🟡 | ❌ | License name, "Accept License", "Retry"; keyed `hub.gated_*` errors |
| "Sign in with Hugging Face" (OAuth PKCE, loopback / device code) | ✅ | ✅ | ❌ | Needs the client ID of a registered OAuth application (SECURITY_MODEL.md); `harness hf login` |
| Custom URL download (HTTPS + SSRF protection) | ✅ | ❌ | ❌ | — |
| Update check (LFS oid comparison) | ✅ | ❌ | ✅ (`pull`) | An "update available" badge on the Hub card and in the variant drawer |
| Recommended quant / "fits your hardware" badge | ✅ (on the card and in the drawer) | 🟡 | ❌ | — |
| Load/run when the download finishes, library refreshes automatically | ✅ ("Load and Chat", optional automatic load) | ✅ | ✅ | **L** |
| Model definition file (model.yaml / Modelfile) | ✅ (`*.harness.toml`) | ✅ | ✅ | **M**: the Harness Model Definition (see §4) |

## 2. Local Library (My Models)

| Feature | Harness | LM Studio | Ollama | Path / note |
|---|---|---|---|---|
| A separate library screen | ✅ (My Models) | ✅ | CLI `ls` | **L** |
| Separation by type: LLM / Embedding / VLM / Draft / LoRA / mmproj / Reranker | ✅ (tabs; the type from GGUF, VLM from the projector match, Draft from tokenizer compatibility) | 🟡 (LLM/Embedding) | ❌ | **L**: GGUF `general.type`, `pooling_type`, `adapter.type`, `clip.*` keys; the reranker also from the `cls.output.weight` tensor (bge-reranker-v2-m3 carries no `pooling_type`) |
| Info: quant, architecture, parameters, context, size, path | ✅ | ✅ | ✅ (`show`) | Parameter count from the tensor table (≈ if estimated); capabilities from the template and projector |
| Loaded/not loaded state, last use, preset used | ✅ | ✅ | ✅ (`ps`) | **L**: `state.db` `model_state` |
| Load / unload / reload | ✅ (My Models, the Playground selector) | ✅ | ✅ | **L** |
| Show in folder, move, delete | ✅ (a preview on delete; blocked while in use) | ✅ | 🟡 (`rm`) | **L** |
| Alias (same file, different settings) | ✅ (Model Definition) | 🟡 (`--identifier`) | ✅ (`cp`/`create`) | **M**: Model Definition |
| Per-model default load settings | ✅ (per definition, "Save as Default") | ✅ | ✅ (Modelfile) | **L** |
| Import from Ollama / LM Studio (link/copy) | ✅ | ✅ (`lms import`) | ❌ | — |
| Multiple model folders, safe move | ✅ | 🟡 | 🟡 (`OLLAMA_MODELS`) | — |
| Missing shard detection and repair | ✅ | ? | ❌ | The HF folder, the recorded `resolve` address, or GGUF source metadata; a shard address if unknown |
| Duplicate model detection (hash) | ✅ (quick digest + full SHA-256) | ❌ | ✅ (digest) | **L** |
| Favorites, recently used, recently downloaded | ✅ | 🟡 | ❌ | **L** |

## 3. Load Configuration

According to the `--help` output of llama-server build 11236:

| Setting | Harness | LM Studio | Ollama | Path / note |
|---|---|---|---|---|
| Context, GPU layers, main GPU, tensor split, split mode | ✅ | ✅ | 🟡 (env/options) | `-ngl auto/all` is not supported |
| Flash Attention, K/V quant | ✅ (f16/q8_0/q4_0) | ✅ | 🟡 (global env) | The runtime supports 9 types (bf16, iq4_nl, q5_x…) |
| KV offload (`-kvo`), unified KV (`-kvu`), `--swa-full` | ✅ | ✅ | ❌ | **L** |
| Batch / ubatch / threads / parallel slots | ✅ | ✅ | 🟡 | — |
| Continuous batching, cache-reuse, `--cache-ram` | ✅ (`--cache-ram` is a separate field in the Memory section, default 1024; with many slots in a unified KV pool, `--cache-ram 0` automatically, against the b11552 stall) | 🟡 (continuous batching since 0.4.0) | ❌ | **L** |
| RoPE / YaRN | ✅ (a YaRN hint when the training context is exceeded) | ✅ | ❌ | **L** |
| Context shift | ✅ | 🟡 (overflow policy) | ✅ | — |
| Jinja / chat template / template kwargs | ✅ (template editor, `--chat-template-file`, viewing the GGUF template) | ✅ | ✅ (TEMPLATE) | **L** |
| Reasoning format / budget / effort | ✅ (catalog) | ✅ | ✅ (`think`) | **L** |
| Draft model + ngl | ✅ | ✅ | 🟡 | When a draft model is selected, `--spec-type draft-simple` is added automatically in newer versions (b11236 produces no draft without it); `--spec-type`, n-max/min are in the catalog |
| Built-in MTP / DFlash / DSpark speculative decoding | 🟡 (`--spec-type` values are read from `--help` and selectable if the runtime supports them; no dedicated UI or measurement) | ✅ (MTP 0.4.14, DFlash/DSpark drafters 0.4.22) | ? | Source: lmstudio.ai/changelog |
| Compatible draft model suggestion | ✅ ("Recommended" on the Dashboard and in the definition editor) | 🟡 | ❌ | Same tokenizer (`tokenizer.ggml.model`, pre, BOS/EOS, vocab ±128, token text digest) and ≤40% size |
| LoRA | ✅ (multiple adapters, live scale on a running server) | ✅ | 🟡 (ADAPTER) | `harness lora list|set` |
| mmproj (vision/audio/video) | ✅ (a path field + automatic suggestion on the Dashboard/definition) | ✅ | ✅ | — |
| MoE: `--n-cpu-moe`, `--cpu-moe`, `-ot` | ✅ | ✅ | ❌ | **L** |
| mmap / mlock / direct-IO (`--load-mode`) | ✅ (`--load-mode`; `--no-mmap/--mlock` on older runtimes) | ✅ | 🟡 | **L**: the old `--no-mmap/--mlock` was removed |
| Embedding / rerank mode, pooling | ✅ (`--embeddings` / `--rerank` automatically from the model type; can be changed with "Server mode"; pooling from GGUF) | ✅ | ✅ | In an embedding/rerank run, `-b`/`-ub` rises to the training context, at least 2048 (if the user did not give a ubatch) |
| Automatic ctx/ngl/KV by memory | ✅ (its own fitting ladder: f16 → q8_0 KV → full offload down to 8k → hybrid fallback at 16k with fewer layers; prevents spilling into shared memory, AMD/Windows driver reserve ≥2200 MiB; an explicit user setting that does not fit gives a 503 with an explanation) | ✅ (autoFit) | ✅ | Out-of-the-box measurement §0 |
| Default context | ✅ (automatic: the GGUF training context, at most 32k) | 🟡 (8k) | 🟡 (4k) | 20k prompt/agent test §0 |
| Estimated memory requirement | ✅ (Rust, SWA/mmproj/KV type; corrected with measurement after loading) | ✅ (estimate-only) | ✅ (measurement + fit) | — |
| UI by runtime capability | ✅ (a capability record: flags/values from the `--help` output; per-parameter `feature`, unsupported fields badged/hidden) | ✅ | — | ARCHITECTURE §13.2 |
| Free-form extra parameters, protected-flag list | ✅ | ✅ | ❌ | — |

## 4. Preset, Model Definition, Alias

| Feature | Harness | LM Studio | Ollama | Path / note |
|---|---|---|---|---|
| Per-model load preset | ✅ (a default definition per model, "Save as Default"; plus named definitions) | ✅ | ✅ | — |
| Named server profile (model + all settings) | ✅ | ❌ | ❌ | The core of the Model Definition |
| Inference preset (system prompt + sampling) | ✅ | ✅ | ✅ (PARAMETER/SYSTEM) | **L** |
| Ready-made presets (Coding, Reasoning, Creative, Deterministic) | ✅ | 🟡 | ❌ | **L** |
| Preset import/export | ✅ (JSON) | ✅ | ✅ (Modelfile) | **L** |
| A single reusable definition (base + template + defaults + adapter + alias) | ✅ (`*.harness.toml`) | ✅ (model.yaml) | ✅ (Modelfile) | **M**: the "Harness Model Definition" (TOML) |
| Ollama Modelfile import | ✅ (FROM GGUF/Ollama model name, PARAMETER, SYSTEM, ADAPTER; TEMPLATE and MESSAGE → warning; preview, `harness import modelfile`) | ❌ | — | The GGUF's Jinja template is used instead of the Go TEMPLATE |

## 5. Inference / Sampling (Playground and API)

| Parameter | Harness Playground | llama-server | LM Studio | Ollama |
|---|---|---|---|---|
| temperature, top_p, top_k, min_p, repeat_penalty, seed, max_tokens | ✅ | ✅ | ✅ | ✅ |
| typical_p, presence/frequency penalty | ✅ | ✅ | ✅ | ✅ |
| stop sequences | ✅ | ✅ | ✅ | ✅ |
| DRY, XTC, top-n-sigma, mirostat, dynatemp, adaptive-p | ✅ | ✅ | 🟡 (XTC) | ❌ |
| sampler chain order (`samplers`) | ✅ | ✅ | ❌ | ❌ |
| GBNF grammar, JSON Schema / `response_format` | ✅ | ✅ | ✅ | ✅ (`format`) |
| logit_bias, logprobs | ✅ (a token-probability view in the Playground) | ✅ | ✅ | 🟡 |
| reasoning on/off/effort/budget | ✅ | ✅ | ✅ | ✅ |
| tool_choice | ✅ | ✅ | ✅ | ❌ |

## 6. Playground

| Feature | Harness | LM Studio | Ollama app | Path / note |
|---|---|---|---|---|
| Streaming chat, thinking block, stats (t/s, TTFT, tokens, cache) | ✅ | ✅ | 🟡 | — |
| Composer token count and conversation usage | ✅ (system, tools, history, draft; exact with the local chat template + tokenize, ≈ in others; a context warning with the reply reserve; message/conversation input-output totals, TR/EN) | — | — | — |
| Tool-calling loop + approval, MCP tools | ✅ | ✅ | ✅ | — |
| MCP client: stdio, Streamable HTTP, legacy HTTP+SSE, OAuth (RFC 9728/8414/7591, PKCE), resources (list/read/attach to chat) | ✅ (no authorization servers without dynamic client registration) | 🟡 | 🟡 | — |
| Multiple chats: list, search, rename, delete, copy, pin, folders | ✅ (`state.db`; drag a chat into a folder or "Move to Folder" from the menu) | ✅ | 🟡 | Dragging uses pointer events; it does not conflict with Tauri's file-drop handler |
| Export/import, automatic title, open the last chat | ✅ | ✅ | 🟡 | **L** |
| Edit any message, regenerate, fork from here, retry | ✅ | ✅ | 🟡 | **L** |
| Full markdown (lists, tables, headings, mermaid) | ✅ (```mermaid blocks are diagrams: Code/Diagram, copy the source, save SVG; follows the theme) | ✅ | ✅ | Mermaid loads (a dynamic import) only for the first diagram |
| Context fill indicator (18,420 / 32,768) | ✅ | 🟡 | ❌ | **L**: `timings` + `/props n_ctx` |
| Load/start/stop a model from the Playground, pick load settings | ✅ ("Load Settings" in the selector, one-off or save to the definition) | ✅ | ✅ | **L** |
| Side-by-side comparison (split view) | ✅ (two models or two presets, parallel streaming, TTFT/t/s/tokens, similarity and word diff, automatically sequential if they do not fit, "Take This Answer into the Chat") | ✅ | ❌ | The split turn is a variant in `state.db`; both sides in JSON/Markdown export |
| Image attachment (vision) | ✅ (button, paste, drag-and-drop; only on a vision model) | ✅ | ✅ | **L**: `image_url` / base64 |
| Text/code/PDF attachment, RAG on long documents | ✅ (PDF page by page, DOCX, Markdown, text, code; drag-and-drop; a document that fits goes in full, otherwise hybrid search: embedding + BM25, optional reranker; numbered citations and the page under the answer) | ✅ | 🟡 | Fully local; embedding/rerank through the gateway by definition alias (JIT). Keyword search if there is no embedding model. Not used in split view |
| Preset selection | ✅ | ✅ | ❌ | **L** |
| Built-in web search and weather | ✅ (`web_search`, `weather`, `geocode`) | ? | 🟡 (ollama.com web search/fetch API, cloud, free account + API key) | DuckDuckGo HTML is the keyless default (new/missing settings); SearXNG, Brave, Tavily are selectable. Location: Open-Meteo + Nominatim if there is no result; forecast: Open-Meteo. A saved `none` is kept; the tool permission policy applies |
| Context compaction (summarize old turns, recent turns verbatim) | ✅ (automatic: a percentage of the context or a fixed token threshold, default 80%; manual "Compact Now" and `/compact [instruction]`; the summary in a divider, undo / reapply; global or per-chat setting, summary model selection, tool-result shortening) | 🟡 (truncation on overflow: cut the middle / rolling window) | ❌ | Original messages are never deleted (a marker node); token counting via llama-server `/tokenize`; see [COMPACTION.md](COMPACTION.md) |
| Embedding test (multiple texts, dimension, time, cosine similarity matrix) and Rerank test (query + documents, scored ranking) | ✅ | ❌ | ❌ | A real `/v1/embeddings` and `/v1/rerank` request to the selected server; the chat tab warns on an embedding/rerank server |
| Speculative acceptance rate per response | ✅ (`timings.draft_n_accepted / draft_n`) | 🟡 | ❌ | — |

## 7. Server Orchestration

| Feature | Harness | LM Studio | Ollama | Path / note |
|---|---|---|---|---|
| Multiple concurrent servers, port safety (automatic ports 11440–11499, with the name of the program holding the port, a gateway/API spare port), profiles, autostart | ✅ | 🟡 | 🟡 | A strength of Harness |
| Router mode (`--models-dir`) | ✅ (model state, load/unload, per-model preset, gateway routing; `harness router`) | — | — | **L** |
| JIT loading (load when a request arrives) | ✅ (gateway; if a running server serves the definition, it routes to it) | ✅ | ✅ | Only for requests arriving through the gateway |
| Unload on idle TTL | ✅ (per definition, a global default; off by default) | ✅ (60 min) | ✅ (5 min) | Only for servers loaded by the gateway |
| Memory-aware eviction, maximum loaded models | ✅ (free VRAM + measured process VRAM/estimate, LRU, pinned models) | ✅ | ✅ | — |
| Automatic parallel slots on gateway loads | ✅ (by context/8k request size; 4 slots for the 4B at 32k) | ✅ (4) | 🟡 (1, via env) | 4-stream measurement §0 |
| Request queue, backpressure (503), cancellation, timeout | ✅ (concurrency equal to the slots, a bounded queue, 503 + `Retry-After`, upstream cancellation on disconnect) | 🟡 | ✅ | On direct ports, llama-server's own slots |
| Automatic restart on crash / backoff | ✅ (optional, at most 3 attempts, 2→60 s exponential wait, a countdown in the UI) | ? | ✅ | Configuration errors are not restarted |
| OOM recovery (lower ngl/ctx/ubatch, unload another) | ✅ (ngl/ctx/ubatch, KV quant, `--fit`, MoE→CPU, unload; a real restart in one click) | 🟡 (guardrail) | ✅ (eviction + 1 retry) | Automatic eviction + 1 retry on a gateway JIT load |
| Remote model access across devices (LM Link: end-to-end encrypted, Tailscale partnership; `lms link`) | ❌ (a local solution with LAN opening + a token, §11) | ✅ | ❌ | Planned: Harness Link (a remote Harness source, HTTPS + token, no account and no VPN); see the ROADMAP |
| Load timeout setting | ✅ (a setting + per model/profile) | ? | ✅ | — |
| Error classification (OOM, missing DLL, corrupt GGUF, architecture, port) and `harness doctor` | ✅ (stable codes, TR/EN messages, Settings > Diagnostics) | 🟡 | 🟡 | — |

## 8. API Compatibility and the Harness API

| Feature | Harness | LM Studio | Ollama | Path / note |
|---|---|---|---|---|
| OpenAI `/v1/models`, chat, completions, embeddings | ✅ (llama-server directly) | ✅ | ✅ | — |
| `/v1/responses` | ✅ (llama-server, by translating to chat; the gateway flattens `namespace` tools; stateful in the gateway: `store`, `previous_response_id`) | ✅ | 🟡 (stateless) | Unsupported tool types give an explicit 400 in the gateway (see §10) |
| Anthropic `/v1/messages` + `count_tokens` | ✅ (llama-server) | ✅ (`/v1/messages`; `count_tokens` ?) | 🟡 (`count_tokens`, `tool_choice`, no prompt caching) | — |
| Speech to text `/v1/audio/transcriptions` and `/translations` (Whisper, sherpa-onnx, ONNX CTC, Python, CrispASR, MLX Whisper/Parakeet; translations: Whisper and supported CrispASR architectures) | ✅ (ONNX CTC: wav2vec2, HuBERT, WavLM via ONNX Runtime; sherpa-onnx ONNX ASR too: Parakeet TDT v3, SenseVoice, Omnilingual (Turkish included), Nemotron 3.5 streaming (Turkish included), Cohere Transcribe, timestamped segments with Silero VAD, who spoke each segment (speaker diarization, `diarized_json`); `harness transcribe`, `harness asr`; whisper-server is a second runtime; JIT + TTL, counted against the VRAM budget; WAV/MP3/FLAC/Ogg directly, M4A/WebM/Opus via ffmpeg; json/text/srt/vtt/verbose_json; the Playground "Speech" tab) | ❌¹ | ❌¹ | The official whisper.cpp releases have no Vulkan/ROCm: a CPU build on AMD; the CUDA build is not verified on NVIDIA |
| Text to speech `/v1/audio/speech` | ✅ (Python, experimental CrispASR, experimental audio.cpp (off by default), MLX audio; managed sherpa-onnx, CPU; Kokoro multilingual, the Turkish Piper voices fettah/fahrettin, Meta MMS folders you unpack (non-commercial license); wav/pcm, mp3/opus/aac/flac via ffmpeg; the Playground "Read Aloud" and Speech tab; `harness speak`) | ❌¹ | ❌ | sherpa-onnx: a process per request; managed speech workers unload when idle |
| Hugging Face ONNX encoders (embedding, rerank; no Python) | ✅ (the `harness-onnx` side process: ONNX Runtime + tokenizers, DirectML/CUDA/CPU; pooling and normalization from sentence-transformers files; ONNX Runtime is downloaded with SHA-256 verification on first use; all-MiniLM, bge, ms-marco cross-encoder) | 🟡 | ❌ | LLM repositories that contain only safetensors work with the conversion below |
| ONNX chat models (onnxruntime-genai) | ✅ (`/v1/chat/completions` inside `harness-onnx`, streaming; DirectML/CPU; DirectML/CPU/CUDA variants in the Hub; Phi-3-mini int4 DirectML ~100–130 tok/s) | ❌ | ❌ | No tool calling; no GenAI library on Linux/macOS; generic exports can produce corrupt output on DirectML |
| Safetensors → GGUF conversion | ✅ ("Convert to GGUF and Download" in the Hub, `harness convert`; managed uv + Python only during conversion, llama.cpp's official converter + `llama-quantize`; Q4_K_M/Q5_K_M/Q6_K/Q8_0/F16/BF16) | ❌ | 🟡 (`ollama create` imports safetensors) | Tried on Windows only; the environment is ~700 MB |
| Lemonade engine (Ryzen AI NPU/FastFlowLM, ROCm/Vulkan llama.cpp) | ✅ (optional `lemond`, an installation with SHA-256; chat/embedding/rerank/transcription through the gateway; a Settings card, `harness lemonade`) | ❌ | ❌ | The NPU path was not tried without the hardware; the license of the FastFlowLM NPU binaries is contradictory, so they are not bundled |
| mistral.rs engine (safetensors LLM/VLM, no conversion) | ✅ (optional; a local Windows CUDA build; chat, streaming, `/v1/responses`, Ollama `/api/chat` through the gateway) | ❌ | 🟡 (Ollama imports safetensors) | Tried on NVIDIA only; no AMD Windows path; the GUI card is not verified in an installed build |
| MLX audio engine (mlx-audio; Apple Silicon; Qwen3-TTS speech synthesis; Whisper and Parakeet speech recognition) | ✅ (optional, macOS 14+; shares the mlx-vlm environment; /v1/audio/speech through the gateway with voice and speed; `/v1/audio/transcriptions` for Whisper and Parakeet; the Hub marks speech recognition repos as running with mlx-audio and Qwen3-TTS repos "Speaks with MLX"; no voice cloning, no file paths; 127.0.0.1 with a per-run key) | ✅ (macOS) | ❌ | Kokoro on MLX is not offered (sherpa-onnx runs it) |
| MLX vision engine (mlx-vlm; Apple Silicon; chat with images on MLX-format vision-language models) | ✅ (optional, macOS 14+; its own managed environment, about 760 MB; the Hub marks mlx-community image-text-to-text repos "Runs with MLX (vision)"; images only inline as data: URLs, the worker opens no links or file paths; 127.0.0.1 with a per-run key) | ✅ (macOS) | ✅ (macOS) | |
| MLX engine (Apple Silicon; MLX-format chat models, including ones with no GGUF) | ✅ (optional, macOS 14+; managed Python environment from a hashed lock; chat, streaming, tool calls through the gateway; the Hub marks mlx-community chat repos "Runs with MLX"; no network isolation on macOS, the worker listens on 127.0.0.1 with a per-run key) | ✅ (macOS) | ✅ (macOS) |  |
| Task pipelines `/v1/hf/{model}` (Hugging Face Inference API format) | ✅ (ONNX: text classification, zero-shot classification, named entities, fill-mask, question answering, image classification, object detection, semantic segmentation, depth estimation, zero-shot image classification with CLIP / SigLIP, audio classification, image feature extraction, image captioning, zero-shot object detection with OWL-ViT / OWLv2, summarization, translation and text2text generation with T5 / BART / Marian; the Playground "Tasks" tab) | ❌ | ❌ | No Python; see ARCHITECTURE §14.4 |
| Rerank `/v1/rerank` | ✅ (a reranker model starts with `--rerank`; the Playground "Rerank" tab; a real `/rerank` check after launch) | ❌ | ❌ | — |
| Many models on one endpoint (routing by model) | ✅ (gateway, `127.0.0.1:11435`; OpenAI, Responses, Anthropic, embeddings, rerank) | ✅ | ✅ | Direct server ports work too |
| Converting tool calls written as text (`<tools>`, `<tool_call>`, plain JSON) into real `tool_calls` | ✅ (gateway, on by default, can be turned off per model) | ❌ | ❌ | 14B tool test 36/38 vs 4–5/38, §0 |
| Ollama native `/api/*` compatibility | ✅ (gateway: `/api/chat`, `/api/generate`, `/api/embed`, `/api/embeddings`, `/api/tags`, `/api/ps`, `/api/show`, `/api/version`; NDJSON streaming, ns durations, tool calling, images, `format`, `think`, `keep_alive` → TTL; verified with the real `ollama` CLI) | ❌ | — | `/api/create`, `/api/copy`, `/api/delete`, `/api/blobs` exist; `suffix`/`context` and `/api/push` give an explicit error |
| Listening on the Ollama port (11434) | ✅ (optional, off by default; the port is not taken if Ollama is running or installed, with a warning) | ❌ | ✅ | — |
| Ollama `/api/pull` | 🟡 (only `hf.co/<owner>/<repo>:<quant>`, enabled by a setting; registry names give an explicit error) | ❌ | ✅ | The Ollama registry is not used |
| Management API (list, download, load, unload, server, metrics, log, benchmark) | ✅ (Control API, SSE events) | ✅ (REST v1) | ✅ | — |
| OpenAPI schema | ✅ (`openapi.json`, utoipa) | ❌ | ✅ | — |
| Decision models `/v1/systemone` (Ollama) / `/v1/decisions` (LM Studio, Jev-compatible) | ❌ | ✅ (0.4.26) | ✅ (0.35.0) | Yes/no probability, choice, score questions; planned (GBNF + logprobs); see the ROADMAP |
| Video generation (`/v1/videos`, jobs) | ✅ (stable-diffusion.cpp `sd-server`; Wan2.1 T2V 1.3B pack; webm / animated webp / avi) | ? | ❌ | — |
| Image generation (`/v1/images/generations`, image editing `/v1/images/edits`) | ✅ (stable-diffusion.cpp `sd-server`; SD1.5/SDXL/SD-Turbo/SD3.5/FLUX/FLUX.2 klein packs; the Playground Image tab, `harness image`; optionally the Python engine's diffusers folders: text-to-image, image-to-image and inpainting with `strength`, sampler choice (`euler`, `euler_a`, `dpm++2m`... and `karras`), the "Largest image" (megapixels per request) and "Low memory" (sliced attention and VAE) settings) | ? | ❌ (removed) | — |
| Stateful chat (`previous_response_id`) | ✅ (gateway: `store` on by default, `GET`/`DELETE /v1/responses/{id}`, `input_items`, `background: true` + `POST .../cancel`, chain compaction with `context_management`; token-owned in `state.db`, with retention time/count/size limits; can be turned off in Settings > Gateway) | ✅ | ❌ | Not on a direct llama-server port (llama.cpp is stateless) |

## 9. CLI, Headless, Agent Control Plane

| Feature | Harness | LM Studio | Ollama | Path / note |
|---|---|---|---|---|
| CLI (`ls/ps/load/unload/pull/run/chat/serve/logs`) | ✅ (`harness`; [CLI.md](CLI.md)) | ✅ (`lms`) | ✅ | The running instance's Control API, otherwise a temporary instance for the duration of the command |
| `--json` output, consistent exit codes | ✅ (`--json` on every command, stable codes 0-6, OOM = 5) | 🟡 | ❌ (API only) | — |
| Headless / service mode | ✅ (`harness serve`; systemd `--user`, launchd, Task Scheduler) | ✅ (llmster) | ✅ (`serve`) | The real installation of systemd/launchd is covered by content tests only |
| Remote instance (`--host`) | ✅ (`HARNESS_HOST` + `HARNESS_TOKEN`, named-token scopes) | ✅ | ✅ (`OLLAMA_HOST`) | — |
| Its own MCP server (agents use Harness as a tool) | ✅ (`harness mcp` stdio + Control API `/mcp`, 20 tools (16 in the default scope; 4 conversation tools need the `conversations` scope), token scopes; [MCP.md](MCP.md)) | ❌ | ❌ | A differentiator |
| `launch <tool>` (start a tool with the right environment) | ✅ (`harness launch`) | ❌ | ✅ | **L** |
| Official SDKs (JS / Python) and plugin development (`lms dev`, `lms push`/`clone` to the Hub) | ❌ (OpenAPI schema + MCP; no SDK) | ✅ | ✅ (Python / JS client libraries) | Control API SDKs are planned (see the ROADMAP); a plugin runtime and Hub publishing are deliberately out of scope (§14) |
| Shell completion | ✅ (`harness completion <shell>`) | ? | ❌ | — |

## 10. Integrations

| Tool | Harness today | Note |
|---|---|---|
| Claude Code | ✅ `harness launch claude` / "Launch" (env + a temporary `--settings`); persistent: `~/.claude/settings.json` `env`; add/remove Harness MCP | The Anthropic base address without `/v1` |
| Codex CLI / IDE | ✅ `launch` (`-c` provider overrides); persistent: `[model_providers.harness]` + the `harness.config.toml` profile (`codex --profile harness`); Harness MCP | Responses API; `web_search` off (the gateway does not drop it silently, it gives 400) |
| OpenCode | ✅ `launch` (`OPENCODE_CONFIG_CONTENT`; the desktop version's `opencode-cli` if it is not on PATH); the desktop application is detected, "Configure and Open in Desktop"; persistent apply/undo (all gateway chat models are listed, the global `model` only on request); per-model tool-calling status (a real trial / gateway record / template) and a first-response estimate; Harness MCP | The desktop paths on macOS/Linux are assumptions |
| Aider | ✅ `launch` (parameter + env + temporary model metadata); a command snippet | — |
| Goose | ✅ `launch` (env); persistent: `custom_providers/harness.json` | — |
| VS Code Copilot BYOK | ✅ `chatLanguageModels.json` (`customendpoint`) | The path is not verified |
| Zed | ✅ `language_models.openai_compatible` | — |
| Kilo Code | ✅ `kilo.json` `provider.harness` | The Windows path is not verified |
| LibreChat | ✅ `librechat.yaml` `endpoints.custom` (the path comes from the user) | YAML comments cannot be preserved |
| Open Design, Continue | ✅ "Prepare Server": the largest context the model supports and that fits the GPU (Open Design ≥128k), KV `q8_0`→`q4_0`, the reason + alternatives shown beforehand | The memory estimate is capped at 90% of the budget |
| Claude Desktop (3P gateway), Cline, Roo Code, JetBrains AI, Jan, Open WebUI, Obsidian Copilot | ✅ step-by-step instructions + copyable fields | Roo Code shut down (2026-05) |
| Cursor | ✅ instructions (with a warning that it cannot reach localhost); add/remove Harness MCP | — |
| OpenAI / Anthropic SDK, LangChain, LlamaIndex, Vercel AI SDK, cURL | ✅ snippet (the real endpoint + alias) | — |

For every persistent integration: a preview (a colored diff), "Test Connection" (a real request with a tool on the tool's API), per-key undo (only keys Harness wrote and that are unchanged), timestamped 5-generation backups, and a hash warning if the file was edited by hand. Reference: [INTEGRATIONS.md](INTEGRATIONS.md).

## 11. Security

| Feature | Harness | LM Studio | Ollama | Path / note |
|---|---|---|---|---|
| API key in the keychain, not visible on the command line | ✅ | 🟡 | ❌ | — |
| Protected flags (`--host/--port/--api-key/--tools…`) | ✅ | ❌ | — | — |
| Localhost only by default | ✅ | ✅ | ✅ | — |
| LAN-opening UI | ✅ (Settings > Security) | ✅ | 🟡 (env) | **L** |
| Keyless-LAN warning/block | ✅ (a keyless network bind is always rejected while tools/MCP are on; otherwise rejected unless the user confirms "open anyway") | 🟡 | ❌ (no built-in authentication in the documentation; a reverse proxy is recommended) | `network.rs` |
| Named tokens, authority scopes, create/edit/delete | ✅ (Control API; the `inference` scope gives access to the gateway; narrow `servers:control` / `downloads:write`; allowed model patterns and endpoint groups per token) | ✅ | ❌ | — |
| CORS / allowed origins | ✅ | ✅ | ✅ | **L** (`--cors-origins`) |
| Rate limit, request timeout | ✅ (per token: requests / tokens per minute and concurrency; 429 + `Retry-After`, in OpenAI/Anthropic/Ollama format; request timeout and queue limit; counters in `analytics.db`, preserved across restarts and shared among instances that share the same data folder) | ❌ | 🟡 | The concurrency limit is in-process |

## 12. Observability

| Feature | Harness | LM Studio | Ollama | Path / note |
|---|---|---|---|---|
| Live t/s, prompt t/s, slots, KV, queue, speculative acceptance rate, process CPU/RSS, GPU load/temperature | ✅ | 🟡 | ❌ | Harness's strongest side; GPU load/temperature on NVIDIA only |
| Log search (regex), run history, a cleaned export | ✅ | 🟡 | 🟡 | — |
| llama-bench sweep, history, comparison | ✅ | ❌ | ❌ | — |
| Per-request metadata (model, client, endpoint, tokens, TTFT, duration, queue, finish, cache) | ✅ (gateway traffic, `analytics.db`; also context fill, speculative acceptance, VRAM/RAM peak, backend, error/OOM; traffic on direct ports cannot be seen) | ❌ | ❌ | Content is not recorded by default; it is opened per definition, with a time limit |
| Historical analysis: P50/P95 by model/client, error and OOM rate | ✅ (raw rows for 30 days, daily totals persist; the Analytics screen, `harness analytics`, `/analytics/summary`) | ❌ | ❌ | — |
| Per-client input/output/total tokens and share | ✅ (Analytics + Performance > Clients; gateway and labeled direct Playground/generator usage, double-count protection, a total KPI, TR/EN names) | — | — | General direct traffic is only the metrics total |
| Request inspector (trace) | ✅ (a filtered list, a queue → load → prompt → generation timeline, error detail, tool-call diagnosis: culprit server/template, context, model, or client; `harness requests`) | 🟡 (`log stream`) | 🟡 (debug) | See [TOOL-DIAGNOSIS.md](TOOL-DIAGNOSIS.md) |
| Prometheus export | ✅ (a combined `/metrics`: gateway counters and per-model histograms, scheduler, system memory, each llama-server's `llamacpp:*` values; on the gateway and the Control API) | ❌ | ❌ | — |
| OpenTelemetry | ✅ (OTLP HTTP/protobuf or gRPC; a span per request and queue/load/generation child spans, optional correlated logs, gen_ai metrics, W3C traceparent/tracestate; content only when explicitly enabled; "Send Test") | ❌ | ❌ | All off by default; the log signal is a separate opt-in |
| Benchmark ↔ real usage comparison | ✅ (the best pp/tg of the last llama-bench run next to the gateway average) | ❌ | ❌ | — |

## 13. Hardware, Backends, and Engines

- **audio.cpp speech and processing:** managed GitHub-release builds for Windows, Linux, and macOS; Vulkan for AMD/Intel, CUDA with companion runtime libraries for Windows NVIDIA, Metal on macOS, and CPU fallback. Auto uses the CPU when there is no discrete GPU, since long Vulkan jobs on an integrated GPU can hit the driver timeout. Vulkan can still be chosen per model, and a GPU lost mid-job is reported as such. Processing requests get a longer timeout for long audio (120 s + 90 s per second of audio, at most an hour, never below the gateway timeout). TTS families speak through `/v1/audio/speech`; seven audited separation/enhancement GGUF families use `/v1/audio/process`. Playground supports recording, playback, and WAV export. The voice picker reads `embeddings/*.safetensors`; backend, device, threads, and idle unload are catalog settings. Hub package variants preserve the selected model folder and its companions. Other audio.cpp tasks remain unsupported.

| Feature | Harness | LM Studio | Ollama | Path / note |
|---|---|---|---|---|
| Windows GPU: DXGI VRAM + PDH live usage, multi-GPU ordering | ✅ | ✅ | ✅ | — |
| Linux: NVIDIA (nvidia-smi), AMD/Intel (sysfs/hwmon), CUDA `ldconfig`/WSL | ✅ (not yet verified on real Linux machines) | ✅ | ✅ | — |
| macOS: unified memory, Intel Mac distinction | ✅ (the `hw.memsize`/`iogpu.wired_limit_mb` rule; not yet verified on macOS) | ✅ | ✅ | — |
| Automatic backend: CUDA / ROCm-HIP / SYCL / Vulkan / CPU / Metal | ✅ (at first install; only a recommendation afterwards) | ✅ | ✅ | — |
| Device list by runtime (`--list-devices`, free VRAM) | ✅ | ✅ | ✅ | — |
| Tensor split / main GPU suggestion | ✅ (by free-VRAM ratio) | ✅ | ✅ | — |
| Runtimes other than official llama.cpp (the PrismML fork etc.), per-server runtime | ✅ (a runtime registry: GitHub/local, SHA-256; selection per definition/profile; a suggestion from the GGUF quant) | ✅ (runtime selection) | ❌ | **M**: engine abstraction |
| MLX engine (Apple Silicon) | ✅ (mlx-lm, mlx-vlm and mlx-audio, ARCHITECTURE §14.9–14.11) | ✅ (MLX; the Mac-specific Splash engine 0.4.25) | ✅ (default on supported architectures since 0.40.0) | **M**, outside llama.cpp: a separate evaluation (ARCHITECTURE §13.8) |
| Orphan-process protection | ✅ Windows, ✅ Linux (PDEATHSIG), ✅ macOS (a kqueue watchdog, not yet verified on macOS) | ✅ | ✅ | — |

## 13a. Application Window and Layout

| Feature | Harness | LM Studio | Ollama app | Path / note |
|---|---|---|---|---|
| Its own window frame: search, status, and window buttons in the title bar | ✅ (Windows/Linux; native traffic lights on macOS) | ✅ | ? | The Snap Layouts menu is not on the custom maximize button (a Tauri limit) |
| Side panels that expand and collapse by dragging and are remembered | ✅ (navigation, the Playground chat list and settings, Dashboard settings, Settings sections; keyboard and Ctrl+B) | 🟡 | ❌ | A single `ResizablePane` component |
| Small-window and high-scale support | 🟡 (down to 900×600; in narrow windows panels collapse to a strip or open on top; not yet verified on screen) | ✅ | ✅ | See the ROADMAP |

---

## 13b. Interface and Accessibility

| Feature | Harness | LM Studio | Ollama | Path / note |
|---|---|---|---|---|
| Light / dark / system theme | ✅ | ✅ | 🟡 (application) | — |
| High-contrast themes (AAA text, ≥3:1 borders, thick focus) | ✅ (dark + light) | ❌ | ❌ | Contrast measurement in ARCHITECTURE §11.1 |
| OLED pure black, Night (low blue) theme | ✅ | 🟡 (a few color themes) | ❌ | — |
| Color-blind-friendly status and chart colors | ✅ (a switch on top of every theme) | ❌ | ❌ | Okabe–Ito based; measured with deuteranopia/protanopia simulation |
| Non-color cues in states (icon, hatching, dashed line) | ✅ | 🟡 | — | Server badge, log, analytics, benchmark, memory gauge, live chart |
| OS "increase contrast" (`prefers-contrast: more`) | ✅ | ❌ | — | The system theme switches to high contrast; in other themes secondary text/borders darken |
| Windows high contrast (`forced-colors`) | 🟡 | ? | — | System colors + visible focus; charts are kept. Not tried by hand |
| Interface scale (90–150%) | ✅ | ✅ | 🟡 | — |
| Interface languages | 🟡 (Turkish / English only) | ✅ (30+ languages²) | ? | Adding a language needs a locale file |

---

## 14. Deliberately Out of Scope

| Feature | Whose | Why |
|---|---|---|
| Web search / web fetch (ollama.com API) | Ollama | A cloud service. If wanted, it is added as an MCP server and does not enter the core |
| Publishing presets/plugins to the Hub, an account system, `lms login` | LM Studio | Harness has no account or server (cross-device access is through the account-free Harness Link; see the ROADMAP) |
| `push` to a model registry | Ollama | We have no registry of our own; HF is already the source |
| Bionic (an agent application) | LM Studio | Outside the product scope |
| A JS/Python plugin runtime | LM Studio | MCP meets the same need through a standard path |
| A live draggable widget grid, a cloud ROI counter | — | Eliminated earlier; the reasoning has not changed |

**A changed decision:** There used to be a "no proxy/gateway, no daemon" principle. A gateway is required for JIT loading, many models on a single endpoint, Ollama compatibility, and per-request observation.

The new decision:
- The gateway is **optional** and runs **inside the application process** (no separate daemon).
- The servers' direct ports stay as they are.
- Request content is not recorded by default; only metadata is kept.

Details: ARCHITECTURE §13.

## 15. Create: Tool Generation

A **Create** group in the main menu; in the shared section side panel: **Tool Creator**, **Automatic Tool Generator**, **My Tools**. The manual page shares the Playground generator. The automatic page runs the inventory → idea → queue → generation → test → install flow with the selected model; pending cards are prioritized by dragging/keyboard. There are pause/resume/stop, retry after an error, a live log, and counters. TR/EN, light/dark themes, and reduced motion are supported.

The persistent worker in the Rust core is shared by the UI, the CLI, and the Control API. Suggestions are validated with strict JSON; name and description token similarity eliminates duplicates. The generator makes two correction attempts. Packages that ask for no external access and pass their tests are installed permanently and automatically; those that ask for network/file access wait for approval that shows the exact permissions. Tests run without external access. The queue, drafts, and log are stored on disk; when the application reopens, a paused run can be resumed.

My Tools lists built-in, installed/generated, and per-server MCP tools in functional categories. The manifest `category` field is validated against six fixed values; older packages become `other`. Installed-tool management shares the existing Settings component; there is package code/manifest/test review, enabling, testing, and removal. [Usage and package guide](TOOLS.md).

---

## Footnotes

1. **Audio endpoints (LM Studio, Ollama):** LM Studio's OpenAI compatibility page (lmstudio.ai/docs/developer/openai-compat) lists only the endpoints `/v1/models`, `/responses`, `/decisions`, `/chat/completions`, `/embeddings`, `/completions`; Ollama's OpenAI compatibility page (docs.ollama.com/api/openai-compatibility) mentions no audio endpoint. The earlier ✅ values were therefore changed to ❌; an undocumented endpoint may exist, and it was not tried by hand. The speech-to-text feature in LM Studio's Bionic application (Bionic 1.1.3) is not an LM Studio server.
2. **LM Studio interface languages:** one version of the documentation counts "26+ more languages", another about 14 languages (lmstudio.ai/docs/app/user-interface/languages); "30+" is approximate, and the exact number could not be verified.
3. Cells marked `?` (for example Ollama interface languages, LM Studio web search/cloud, MTP, image generation, `count_tokens`) were not guessed because they could not be verified in the official documentation.


### Experimental audio.cpp Speech and Processing

audio.cpp is **experimental and off by default**. Enable it in Settings > Runtime > Installed Runtime > Other Runtimes after confirming the risk notice. Its local server has no authentication or Host/Origin checks and accepts server-side file paths (`voice_ref`, JSON `audio`, and `/v1/tasks/*`). While running, a web page on this computer can send requests and pass file paths directly to that server. Loopback binding does not remove this risk.

Disabled engines refuse gateway, Load, and CLI launches. Models remain visible in the Hub and library with an experimental engine requirement. Workers start on demand, bind only to `127.0.0.1` on a random free port (excluding 11435, 11440, and 11500), and use `--no-ui` without `--ui-management`. Harness stops the process after five minutes without requests; `--idle-unload-ms 300000` also releases model memory. Pinned models do not bypass this timeout or start at gateway startup.

The gateway supports `POST /v1/audio/speech`, rebuilding model, input, discovered voice, speed, response format, and optional inline `voice_ref` (`type: base64`, decoded size at most 5 MiB). String/path references and undiscovered voices return 400. It also supports the processing route below. Client access to transcriptions, alignments, tasks, model management, and all other audio.cpp worker routes is blocked. These gateway restrictions do not secure direct access to the worker port.

`POST /v1/audio/process` accepts multipart `model` + `file` or JSON `model` + `audio_base64` (including WAV data URIs). Options are flat scalar fields. Input must be complete PCM/float WAV, mono/stereo, at most 32 MiB and 600 seconds; SAM Audio is limited to 400 seconds. The existing converter targets 16 kHz mono for ASR, so processing rejects other containers instead of silently reducing music quality. Unknown options are rejected; `audio`, `voice_ref`, `output*`, path/directory fields and artifacts are dropped. The gateway rebuilds `{model: resolvedAlias, request: {audio_base64, options}}` for the private `/v1/tasks/run`; SAM's bounded `prompt` becomes `request.text`.

| Family | Safe package (Hugging Face) | Task | Input / output | Request options |
|---|---|---|---|---|
| `bs_roformer` | `audio-cpp/audio.cpp-gguf`: `BS-RoFormer-ep368-GGUF/bs-roformer-ep368-q8_0.gguf` | `sep` | 44.1 kHz, mono/stereo → `vocals`, `instrumental`, same rate/channels | None (overlap is a session setting) |
| `mel_band_roformer` | Same repo: `Mel-Band-RoFormer-GGUF/mel-band-roformer-{q8_0,f16}.gguf` | `sep` | 44.1 kHz, mono/stereo → `vocals`, `instrumental` | None |
| `sam_audio` | `audio-cpp/SAM-Audio-GGUF`: `sam-audio-{small,base,large}-{f32,bf16,q8_0}.gguf` (embedded tokenizer/T5/config) | `s2s` | WAV resampled internally → `target`, `residual`, 48 kHz mono | Required `prompt` (1–1024 characters); `seed`, `num_inference_steps`. No image/video paths or anchors |
| `sidon` | `audio-cpp/audio.cpp-gguf`: `Sidon-GGUF/sidon-v0.1-f32.gguf` | `s2s` | WAV internally downmixed/resampled to 16 kHz → `enhanced`, 48 kHz mono | None |
| `apollo` | Same repo: `Apollo-GGUF/apollo-orig.gguf` | `s2s` | 44.1 kHz, mono/stereo → `enhanced`, same channels/rate | `audio_chunk_duration_sec`, `audio_chunk_overlap_sec`, `edge_pad_duration_sec` |
| `audiosr` | Same repo: `AudioSR-GGUF/audiosr-basic-f32.gguf` | `s2s` | WAV internally prepared → `enhanced`, 48 kHz | `num_inference_steps`, `guidance_scale`, `ddim_eta`, chunk duration/overlap, `seed`; GPU backend only: audio.cpp v0.9.1 aborts on the CPU backend, so Harness refuses to start it there. |
| `universr` | Same repo: `UniverSR-GGUF/universr-{audio,speech}-orig.gguf` | `s2s` | WAV → `enhanced`, 48 kHz mono | `input_sample_rate` (8000/12000/16000/24000 effective source bandwidth), `sampler_mode` (euler/midpoint/rk4), steps, guidance, seed, chunk duration |

The audited v0.9.1 server returns a base64 WAV in `audio`, or `{id,audio,...}` entries in `named_audio_outputs`; artifacts use `{id,kind,payload,meta}`. Harness returns only `{model,outputs:[{name,audio_base64,mime:"audio/wav"}]}` with bounded outputs. File artifacts are read only inside the worker's owned output directory after canonicalization and symlink/traversal checks, then removed. All other metadata and paths stay private; unsafe or malformed results return 502.

Playground > Speech includes **Process Audio**: choose a model, upload WAV or record, adjust family options, run, play each output, and save through the native WAV dialog. Recordings use 44.1 kHz for RoFormer/Apollo. The disabled panel links to the existing engine setting. Hub shows the experimental requirement without claiming other engines' audio repos.

Not included: `mossformer2` and `tf_gridnet` have no model specs/implementation in the supplied v0.9.1 tree. `builtin_audio_utils` RNNoise (`rnnoise10Gb_15.safetensors`) and DeepFilterNet2 (`deepfilternet2.safetensors`, 48 kHz input/output) have repository asset weights but no single HF package in their model spec; a safe Hub install/identity path is not implemented. SpeechBrain pickle `.ckpt`, custom-code repositories, standalone vocoders and other built-in utilities remain excluded.

## Experimental CrispASR Speech Recognition

CrispASR is **experimental**: it is on by default once installed, but the engine is young and its model support is limited to CrispASR-format GGUFs.

[CrispASR](https://github.com/CrispStrobe/CrispASR/tree/v0.8.43) (MIT) is a managed native ServerEngine pinned to v0.8.43. Settings > Runtime > Installed Runtime > Other Runtimes installs it; the runtime registry updates/removes it. Windows AMD uses Vulkan, Linux AMD prefers HIP then Vulkan, NVIDIA uses CUDA, Apple Silicon uses Metal, and other platforms use CPU. The runtime record stores the chosen backend.

Only CrispASR-format ASR GGUFs are supported: Parakeet, Canary, Cohere, Qwen3-ASR, Voxtral, Nemotron, GigaAM, Granite, VibeVoice, Moonshine, SenseVoice and cstr wav2vec2/HuBERT/Data2Vec. Hub ASR `cstr/*-GGUF` repositories offer GGUF variants with fitting Q5/Q4 recommendations. Architecture/provenance are checked; Cohere additionally requires CrispASR tensor names. handy-computer exports are unsupported except `handy-computer/canary-180m-flash-gguf`. Other TTS backends, standalone codecs and chat are not supported yet. Source-only repositories retain their status. Whisper ggml .bin, sherpa-onnx catalog models and ONNX CTC retain their engines.

Playground > Speech > Transcribe, the gateway audio API and `harness transcribe audio.wav --model <alias>` use this engine (CLI also accepts supported local GGUF paths). It starts on demand and managed workers stop after the gateway idle TTL. A worker exit reports an upstream error; the next request can restart it. Bind is 127.0.0.1 only, on a random free port outside the automatic server range. Each run gets a fresh random key through CRISPASR_API_KEYS, never argv; Harness sends Bearer authentication. CORS and automatic model downloading are disabled.

Only POST /v1/audio/transcriptions and POST /v1/audio/translations reach the worker. Multipart is rebuilt with file bytes, the definition alias as model, language, prompt, response_format, temperature and timestamp_granularities (including the [] spelling). Unknown fields or model/path fields such as diarize_embedder, lid_model, punc_model, truecase_model and voice return 400. Worker /load, /inference, /backends, chat and speech synthesis are never forwarded. Ollama /api/chat, /api/generate and /api/embed return 400 for these models before starting a worker.

The CrispASR worker never downloads anything from the network. Its helper models (language detection `ggml-tiny.bin` and punctuation restoration `fireredpunc-q4_k.gguf`) are pinned to a repository revision with a size and SHA-256 and come through Harness's own verified downloader: they show in the download queue and in Settings > Runtime > Installed Runtime > Other Runtimes > CrispASR Helper Models (Download / Remove), and reach the worker only as local paths (`--lid-model`, `--punc-model`). A missing language model starts the worker with `--lid-backend off` and the gateway answers a request without a language with a 400; a missing punctuation model means `--no-punctuation`. Every launch passes `--cache-dir <data>/cache/crispasr` and sets `CRISPASR_CACHE_DIR` to it, and the worker's proxy variables point at a closed local port. On Linux the worker is not put in a network namespace: CrispASR can only listen on a TCP port, which a namespace without network would hide from the gateway (python workers listen on a Unix socket for that reason), so the closed proxy, the empty cache folder and the absent download flags are the guard there. A model downloaded or removed while a worker runs takes effect when that worker restarts.

### Per-Chat Thinking

The chat composer offers Thinking for reasoning models. Model Default follows the preset, then the model default; the choice and optional token budget are saved per conversation without modifying its preset. Advanced offers Off, Light, Normal and Deep when supported (1024, 4096 and -1 tokens on budget-capable toggle models). Effort models use low/medium/high and omit Off; Simple switches between Light and Normal for these models because thinking cannot be fully disabled. Other models use a Think toggle (Off/Normal). Unknown or older runtimes with only a toggle offer Off/On; Light/Normal/Deep requests all mean On.

Budget support is enabled for toggle templates served by numbered llama.cpp builds b11321 or later, or an explicit `supports_reasoning_budget` capability. The checked b11321 source accepts `thinking_budget_tokens` and `reasoning_budget_tokens`; -1 follows the server launch budget, normally unlimited. A template alone never enables the budget input. The gateway applies the same level table to OpenAI Chat/Responses effort and Ollama `think`, preserving explicit client kwargs and budgets. Split view continues to use each side's preset. Title generation, compaction and tool generation keep their existing thinking-off requests.

Reasoning streams in a collapsible panel, closes when the answer starts, and can be copied. Leading inline `<think>` blocks use the same panel. The duration from the first reasoning token to the first answer token is saved with the message; old or unfinished replies show no duration.
## Experimental CrispASR Text to Speech

Kokoro and Pocket TTS GGUFs from `cstr/kokoro-82m-GGUF` and `cstr/pocket-tts-GGUF` (English builds only: the bundled alba voice is English) run through `POST /v1/audio/speech` and appear in Playground > Speech > Text to Speech. Install the experimental v0.8.43 runtime and its companions in Settings > Runtime > Installed Runtime > Other Runtimes > CrispASR Helper Models first. The initial English presets are Kokoro `af_heart` and Pocket TTS `alba`. Other backends are not supported yet. Pocket TTS has its Mimi decoder and tokenizer inside the model; Kokoro has no separate codec.

Only verified presets enter a Harness-owned per-model voice folder. The allowlist is built from SHA-256-checked files, never arbitrary model siblings. Kokoro resolves its single preset at launch (`--voice`); its adapter does not resolve bare names through `--voice-dir`, so the gateway validates `af_heart` and omits the worker request's voice field. Pocket TTS resolves `alba` against the owned folder. Paths, inline audio, voice uploads, cloning and `consent_attestation` are rejected. Unknown JSON fields and every other model-targeted route return 400 before acquiring a worker. The gateway rewrites the model alias and forwards only known speech fields. Text is limited to 4000 characters; generation has a 120-second deadline, output is limited to 120 seconds and 24 MiB. Speech workers stop after 300 idle seconds, including pinned definitions.

Companions use the pinned, size- and SHA-256-checked download queue. Pocket's upstream repository may require access acceptance and a configured token; Harness does not accept terms automatically. The worker receives local paths and uses the same owned cache and closed proxy settings as ASR; missing required TTS companions prevent launch. Punctuation and language detection remain optional ASR helpers and are disabled when absent.

| Companion | Repository and revision | File | Bytes | SHA-256 |
|---|---|---|---:|---|
| Kokoro preset | `cstr/kokoro-voices-GGUF@1c98db113c69e3feef9644cf0aaec5c0bea02a8e` | `kokoro-voice-af_heart.gguf` | 522784 | `14463a5a5204a16a382b2da87b4b0206fcda225b44b7c4627999eb527f49c018` |
| Pocket preset | `audio-cpp/audio.cpp-gguf@f861648c0c1f4da9f18e9a0ce5e217b0679bb2f5` | `PocketTTS-GGUF/english/embeddings/alba.safetensors` | 6194424 | `69c32db63ca56843d994f81f343f62e0bf2d73f7e4c9bc73e44bb1110b1d8845` |
| English pronunciation | GitHub `hexgrad/misaki@fba1236595f2d2bf21d414ba6e57d25256afada3` | `misaki/data/us_gold.json` | 3000469 | `dc414872a49a28ae6c141463d502fd945f3b2fde040484fdc47d00cc4612686f` |
| English fallback | GitHub `cmusphinx/cmudict@74790861f652b15e4ac49015a90074ad62a27690` | `cmudict.dict` | 3618488 | `81917843c7f44ce2b094ac63873c2c7a4cf802040792c455ba3ca406891c3d22` |

The local gold lexicon prevents Misaki's automatic download; a local CMU dictionary also covers its fallback path and `builtin-only` avoids external phonemizer selection. The optional neural G2P path is fixed inside the owned cache; its built-in inflection and fallback rules handle unlisted words. Only English presets are offered initially. Companion licenses: Apache-2.0 (Kokoro and Misaki), CC-BY-4.0 (Pocket), BSD-2-Clause (CMUdict).
