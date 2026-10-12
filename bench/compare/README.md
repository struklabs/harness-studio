# Comparison benchmark: Harness Studio vs LM Studio vs Ollama vs llama.cpp

A reproducible suite that measures the same GGUF files through four products' OpenAI-compatible APIs on one
Windows machine. Results of the 2026-10-11 run (Harness rows re-measured on the release build; rows of the earlier build are kept): `docs/benchmarks/2026-10-11/summary.md` (tables) and
`docs/benchmarks/2026-10-11/raw/` (one JSON per backend, mode, model and metric, including the effective settings
each tool ended up with).

## What is compared

| Key | Tool | How it is reached |
|---|---|---|
| `harness` | Harness Studio gateway (`http://127.0.0.1:11435/v1`, on-demand loading) | model *definitions* created through the Control API |
| `lmstudio` | LM Studio `lms server` (`:1234`) | `lms load` with explicit flags |
| `ollama` | A **second** `ollama serve` on `:11437` started by the suite with a clean environment | models created from the same GGUF with a `Modelfile` |
| `llamacpp` | Plain `llama-server` from winget (b11026) | process started by the suite |
| `llamacpp_b11552` | Harness's own managed `llama-server` (b11552) started directly | engine-parity control: the difference to `harness` is product-level only |

Models (same file for every tool, no re-quantisation): `Qwen3-4B-Instruct-2507-Q4_K_M.gguf` ("4b") and
`qwen2.5-coder-14b-instruct-q4_k_m.gguf` ("14b"). Edit `GGUF` in `backends.py` for other paths.

## Modes

* `out_of_box` - what a new user gets. Harness: a definition with no load settings; LM Studio: `lms load <model>`;
  Ollama: a `FROM <gguf>` Modelfile and a server without any `OLLAMA_*` tuning variables; llama.cpp: `-m` only.
* `equal` - context 32768, full GPU offload, flash attention on, f16 KV cache, 1 slot, for every tool where the setting is
  reachable (LM Studio's CLI has no flash-attention or KV-type switch, its value is recorded instead).
* `equal4` - like `equal` with 4 slots and the same total context (32768), used for the concurrency test only.
* `ctx65536` - 14B with a 65536-token context and every other setting left at the tool default (memory-safety test).

## Metrics (`--metrics`)

`speed` (TTFT + decode tok/s for a ~500-token prompt / 256 generated tokens, prompt-processing tok/s for ~4k tokens),
`needle` (~20k-token prompt, fact at the start, asked at the end: correct / error / silent truncation),
`tools` (38 fixed OpenAI-format tool cases in `tools_suite.py`, EN + TR, up to 38 tool definitions per request),
`agent` (3-turn conversation, ~8k-token system prompt + 38 tools, TTFT of turns 2 and 3 = prompt-cache reuse),
`concurrency` (4 simultaneous streams x 256 tokens), `memory` (14B @ 65536), `switch` (4B loaded, ask for the 14B).
Temperature 0, seed 42, one warm-up run discarded, medians over >= 3 runs. Every request starts with a unique nonce so
prompt caches cannot hide the cost of the first request, except where the cache is exactly what is measured.

## Reproduce

Prerequisites: Windows with a Vulkan GPU, the four tools installed, Python 3.10+.

```powershell
cd bench\compare
python -m venv .venv ; .venv\Scripts\python.exe -m pip install -r requirements.txt

# Harness: run the app, or headless:  harness serve --gateway
#   create the six definitions (alias -> load settings) through POST /api/harness/v1/definitions, see setup_definitions.py
.venv\Scripts\python.exe setup_definitions.py

# LM Studio: import the two files without copying (hard link) so the same bytes are used
lms import -L --user-repo bench/qwen3-4b-instruct-2507 -y <path-to-4b.gguf>
lms import -L --user-repo bench/qwen2.5-coder-14b-instruct -y <path-to-14b.gguf>

# Ollama: the suite starts its own server on :11437; create the models once against it
$env:OLLAMA_HOST="127.0.0.1:11437"; ollama serve   # separate window, then:
ollama create bench-4b -f ollama\Modelfile.4b ; ollama create bench-14b -f ollama\Modelfile.14b
# plus the num_ctx variants bench-4b-eq / bench-14b-eq (num_ctx 32768) and bench-14b-ctx65536 (see ollama\)

.venv\Scripts\python.exe run.py --backends harness,lmstudio,ollama,llamacpp,llamacpp_b11552
.venv\Scripts\python.exe summarize.py        # writes summary.json / summary.md next to raw/
```

GPU discipline: the runner stops every other backend before each load and waits until dedicated VRAM is back to the
baseline it measured at start (Windows performance counters, `\GPU Adapter Memory`). The desktop itself holds several GB
of VRAM on the test machine (see the baseline in the summary); that reduces the memory available to all tools equally.

Do not run other GPU workloads while the suite runs. A full run takes roughly 1.5-2 hours. The suite does not change any
Windows setting; it creates Harness definitions, LM Studio hard links and Ollama models that `cleanup.py` removes.
