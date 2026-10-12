# Harness Studio vs LM Studio, Ollama and llama.cpp

Measured on 2026-10-11 on one Windows machine, with the same GGUF files for every tool. The full tables, raw JSON, and runner live in
[benchmarks/2026-10-11/summary.md](../benchmarks/2026-10-11/summary.md), [benchmarks/2026-10-11/raw/](../benchmarks/2026-10-11/raw/), and
[`bench/compare/`](../../bench/compare/). The runner re-creates every number on another machine. The feature-by-feature matrix (what exists in which tool) is in
[FEATURES.md](FEATURES.md), which also carries these measurements in its §0.

## Short version

- **Speed with no tuning:** on par with LM Studio and ahead of Ollama and plain llama.cpp for single-request decoding. **2.2× LM Studio
  and 2.7× Ollama** when four requests arrive at once.
- **Tool calling:** the only tool that turned the 14B coder model's text-formatted tool calls into real calls. Harness scored 36/38;
  the others scored 4–5/38.
- **Long prompts and agents:** the out-of-box context fits a 20k-token prompt and a 10.8k-token agent conversation. LM Studio (8k) and
  Ollama (4k) return a context error until the user raises the limit by hand.
- **Memory safety:** Harness picks settings that keep the model out of shared system memory. It refuses requests that cannot fit and
  explains why, instead of loading them and running slowly. The trade-off is that a 14B model at a 65k context decodes slower than the
  competitors' spilled loads in a short-prompt test.
- **Model switching through one endpoint:** 6.5 s (Ollama 5.6 s). LM Studio, with its default JIT setting, answered silently with the
  wrong model. llama.cpp needs a restart.

Where the competitors lead is listed below with the same weight.

## Test setup

| | |
|---|---|
| Machine | Windows 11 Pro, AMD Ryzen 7 9700X, 32 GB RAM, AMD Radeon RX 7900 GRE 16 GB (Vulkan) |
| Harness Studio | 0.1.0, release build, managed llama.cpp b11556 (Vulkan), `harness serve --gateway` |
| LM Studio | lms CLI 69d945a, engine llama.cpp-win-x86_64-vulkan-avx2 2.51.0 |
| Ollama | 0.35.1, a separate clean `ollama serve` instance |
| llama.cpp | b11026 (winget) and b11552, `llama-server`, Vulkan |
| Models | Qwen3-4B-Instruct-2507 Q4_K_M (2.5 GB), Qwen2.5-Coder-14B-Instruct Q4_K_M (9.0 GB): identical files for every tool |

**Run conditions**

- Tools ran one at a time. Before each load, every other backend was stopped and dedicated VRAM had to return to its baseline.
- Streaming OpenAI `chat/completions` requests with `temperature 0` and `seed 42`. Each request had a unique nonce so prompt caches
  could not flatter cold requests.
- Each value is the median of 5 decode runs, 3 prompt-processing runs, and 3 concurrency rounds, after a discarded warm-up.

**Profiles**

- **out of box:** each tool loads the model with its own defaults.
- **equal:** every tool gets the same explicit settings: 32k context, full offload, flash attention, f16 KV, 1 slot.

Full methodology: [summary.md](../benchmarks/2026-10-11/summary.md#methodology).

## Results

Higher is better for tok/s; lower is better for seconds and shared-memory growth. "Context error" means HTTP 400 "exceeds the available
context". Harness 14B out of box: 49/49 layers, 16k context, q8_0 KV, 2 slots. Harness 4B: 37/37 layers, 32k context, f16 KV, 4 slots.

| Test | Harness Studio | LM Studio | Ollama | llama.cpp |
|---|---|---|---|---|
| 4B decode, out of box (tok/s) | **140.9** | **142.0** | 128.9 | 114.5 |
| 4B prompt processing, 4k tokens (tok/s) | 2460 | 2308 | **2997** | 1381 |
| 14B decode, out of box (tok/s) | **48.6** | 42.0 (repeated measurement 50.7) | **49.0** | 40.9 |
| 14B prompt processing, 4k tokens (tok/s) | 737 | 442 (repeated measurement 822) | **1109** | 402 |
| 4B decode, equal settings (tok/s) | 144.3 | 143.3 | 128.9 | **146.9** |
| 14B, equal settings (needs ~14.6 GB, 12.1 GB free) | refused with an explanation | loaded, spilled to shared memory | loaded, spilled | loaded, spilled |
| 20k-token prompt, out of box | **correct** | context error | context error | **correct** |
| Tool calling, 4B (38 cases) | **38/38** | **38/38** | **38/38** | **38/38** |
| Tool calling, 14B coder (38 cases) | **36/38** | 5/38 | 4/38 | 5/38 |
| Agent test, cached turns 2/3 TTFT (s) | **0.45 / 0.25** | context error | context error | 0.95 / 0.30 |
| Agent test, cold first turn (s) | **6.6** | context error | context error | 27–30 |
| 4 concurrent requests, out of box (tok/s total) | **344** | 157 | 126 | 152 |
| 4 concurrent requests, 4 slots each (tok/s total) | **399** | 156 | 266 | **398** |
| 14B at 65k context, decode (tok/s) | 15.5 | **46.0** | 43.8 | 14.6 |
| 14B at 65k context, shared-memory growth (MiB) | **98** | 12376 | 10326 | 5208 |
| Switch 4B → 14B through one endpoint, time to first token (s) | 6.5 | wrong model answered | **5.6** | not supported (restart 22.9) |

<details>
<summary>Before and after: the benchmark also fixed Harness Studio</summary>

The first run of the day used the previous Harness build. It exposed four weak defaults:

- AMD/Windows driver reserve was too conservative (4B on 29/37 layers, 14B on 33/49).
- Gateway loads used a single slot.
- Shared KV pools ran without `--cache-ram 0`.
- Text tool-call recovery was opt-in.

All four were fixed before release, and the Harness rows were measured again on the release build. Competitor rows were not re-run,
apart from an LM Studio control run.

| Test | Earlier build | Release build |
|---|---|---|
| 4B decode (tok/s) | 47.9 | 140.9 |
| 14B decode (tok/s) | 14.2 | 48.6 |
| 14B tool calling | 5/38 | 36/38 |
| 4 concurrent, out of box (tok/s) | 53 | 344 |
| 4 concurrent, 4 slots (tok/s) | 170 | 399 |
| Model switch TTFT (s) | 9.1 | 6.5 |

Raw files for the earlier build are in [raw/pre-fix/](../benchmarks/2026-10-11/raw/pre-fix/).
</details>

## What the numbers mean

- **Defaults matter more than peak speed.** With equal settings, all four engines decode at about the same speed, because they share
  the llama.cpp core. The differences come from what each tool picks for the user:
  - context size
  - layer count
  - KV cache type
  - slot count
  - whether a text tool call is recovered
  - whether a model that does not fit is loaded anyway
- **Context that works without tuning.** Harness sizes the context from the model, up to 32k, and from free VRAM. LM Studio (8k) and
  Ollama (4k) failed the 20k-token prompt and the agent test until the limit was raised. Plain llama.cpp picked an 85k context that
  partly spilled to shared memory, which is why its cold agent turn took 27–30 s.
- **Tool calls that clients can use.** Qwen2.5-Coder writes tool calls as `<tools>{json}</tools>` text. Only Harness turns this into
  OpenAI `tool_calls` by default. Clients such as Claude Code, OpenCode, Zed, or VS Code then get real calls.
- **Concurrency.** Gateway loads open four slots sized for real requests, and they avoid the llama.cpp prompt-cache stall in shared
  pools. Out of box, four parallel requests run 2.2× faster than LM Studio and 2.7× faster than Ollama, with no flags set.

## Where the competitors lead

- **Prompt processing:** Ollama processed 4k-token prompts fastest in this test: 2997 vs 2460 tok/s on the 4B, 1109 vs 737 on the 14B.
- **Large context on a too-small card:** at 65k, LM Studio and Ollama decoded short prompts about 3× faster than Harness by spilling
  10–12 GB into shared memory. This run did not measure how much that spill slows long prompts. Harness kept the model out of shared
  memory and gave up decode speed by moving layers to the CPU.
- **Explicit settings that do not fit:** Harness refuses them (HTTP 503, with the required and free memory) instead of loading slowly.
  Users who prefer "load anyway" have to lower the settings.
- **Maturity and reach:**
  - Harness Studio is a first 0.1.0 release with a single maintainer.
  - LM Studio and Ollama have large user bases, model catalogues, macOS/MLX support, and SDKs.
  - LM Studio has a 30+ language interface. Ollama has cloud models and `ollama launch`.
  - llama.cpp has the widest set of hardware backends, and its router mode now covers several features that used to need a separate
    gateway.
- **Missing features in Harness today:**
  - Official TypeScript and Python SDKs (planned)
  - Decision-model endpoints (`/v1/decisions`, `/v1/systemone`) and remote access to another Harness (Harness Link) (both planned)
  - An MLX engine and a macOS build verified on real machines
  - Interface languages other than Turkish and English
  - Ollama `push` (by design: Harness has no model registry)

## Limits of this comparison

- One machine and one GPU vendor: AMD with Vulkan on Windows. NVIDIA/CUDA, Linux, and macOS were not part of this run.
- Competitor rows come from the first run. A repeated LM Studio control measurement matched the first run on the 4B (142.7 vs 142.0
  tok/s) but was about 20% faster on the 14B (50.7 vs 42.0). Run-to-run variance on the 14B is about 20%; treat smaller 14B differences as noise.
- Two models from one family each. Other architectures, vision models, and MoE models were not tested.
- The tool suite has 38 cases and runs once per case. It shows the format-recovery effect clearly, but it does not grade reasoning
  quality.
- The Harness author wrote the runner. It is open in [`bench/compare/`](../../bench/compare/) so others can re-run it.
