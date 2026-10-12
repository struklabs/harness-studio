## Observations (factual, from the tables above)

- **History (read first).** The first run of the day used the previous build (before the fixes); its Harness rows are kept in the
  "previous build" rows and in `raw/pre-fix/`. Those rows exposed three product defaults (AMD/Windows driver reserve made the
  automatic layer count too conservative: 4B 29/37 layers, 14B 33/49; gateway loads used 1 slot; shared pools without
  `--cache-ram 0`; text tool-call recovery was opt-in). They were fixed in the release build and the Harness rows were re-measured in
  a later session against a freshly built and installed release build (managed runtime b11556). The competitors' rows were not re-run, except one
  control: LM Studio out_of_box speed (4B and 14B), repeated in the same session with the same GPU baseline (4188 vs 4375 MiB
  dedicated, dwm.exe 5.85 GiB). Control 4B: 142.7 tok/s decode, 2315 tok/s PP (first run 142.0 / 2308), so the 4B conditions are
  comparable; the 14B control decoded at 50.7 tok/s (first run 42.0), i.e. a run-to-run variance of up to about 20 % exists
  for the 14B and applies to every 14B comparison.
- Harness Studio (release build) out of the box, 4B: 37/37 layers, 32768 context, f16 KV, 4 slots, 141 tok/s decode (previous
  build 48 tok/s, 29/37 layers, 1 slot) - on par with LM Studio (142) and ahead of Ollama (129) and plain llama.cpp (112-115).
  14B out of the box: 49/49 layers at a **16384 context with q8_0/q8_0 KV and 2 slots** (new behaviour: the product shrinks the
  context and quantises the KV cache so that nothing spills into shared memory on this card; previously 32768/f16 with 33/49
  layers), 48.6 tok/s (previous 14.2; LM Studio 42.0 first run / 50.7 repeated, Ollama 49.0, llama.cpp 40-41, those with 4096-33280
  contexts). The trade-off is a smaller context than llama.cpp's automatic 33k for the 14B.
- Harness refused (HTTP 503 with the numbers) the 14B `equal` request (32768 f16, explicit settings) again because about 14.6 GB
  were needed and about 12.1 GB were free; the other tools loaded it and spilled into shared memory (Ollama: 234 tok/s prompt
  processing at 4k, LM Studio 349, llama.cpp 359-390; Harness did not run it). Unchanged by the fix: explicit settings are not
  silently reduced.
- Long-prompt integrity (20k tokens, 4B, out of the box): Harness and plain llama.cpp answered correctly (contexts 32768 and 85248);
  LM Studio (context 8192) and Ollama (context 4096) returned a clear HTTP 400 "exceeds the available context". No tool truncated
  silently in this test.
- Tool calling, 4B: all five engines 38/38. 14B (Qwen2.5-Coder): every tool returned the call as `<tools>{json}</tools>` text; only
  Harness converted it. Since the fixes the text tool-call recovery is on by default: Harness scores 36/38 out of the box (the two
  misses are spurious calls on Turkish no-tool prompts); the previous build scored 5/38 without the opt-in setting, like LM Studio
  and llama.cpp (Ollama 4/38).
- Prompt cache (agent test, 10.8k-token prompt): all tools reuse the prefix in turns 2-3 (0.25-0.55 s TTFT) once the context is large
  enough; LM Studio (ctx 8192) and Ollama (ctx 4096) failed the out-of-box agent test with a context error. The cold first turn took
  6.6 s on Harness (7.1 s on the previous build, no flash attention) versus 27-30 s on plain llama.cpp out of the box, whose automatic 85248-token
  KV cache was partly in shared memory.
- Concurrency out of the box: Harness now opens 4 slots for gateway loads and aggregates 344 tok/s (previous build: 1 slot, 53 tok/s);
  LM Studio and llama.cpp default to 4 slots (157 / 152 tok/s), Ollama 126. With 4 slots and equal settings: llama.cpp 398,
  Harness 399 (previous 170), Ollama 266, LM Studio 156 tok/s.
- Model switch with the 4B loaded: Harness 6.5 s (previous build 9.1 s) and Ollama 5.6 s to first token (including the 14B load);
  llama.cpp cannot (restart: 22.9 s); LM Studio with JIT loading inactive did not switch and silently answered with the already
  loaded 4B.
- Memory safety, 14B at 65536: all four loaded it. Harness adapted (q8_0/q4_0 KV, 34/49 layers, 15.5 tok/s, 98 MiB shared-memory
  growth; previous build 35/49 layers, q8_0/q8_0, 16 tok/s, 1126 MiB growth); LM Studio, Ollama and llama.cpp kept all or most
  layers on the GPU and put 5-12 GB into shared memory, decoding 44-46 tok/s (LM Studio, Ollama) and 14.6 tok/s (llama.cpp). The
  decode probe uses a short prompt, so it does not show how much the shared-memory KV cache would slow long prompts down. Harness
  is the only tool that stays out of shared memory here, at one third of the short-prompt decode speed of LM Studio/Ollama.

## Methodology

- One machine, one session, tools run strictly one at a time. Before every load the runner stopped all other backends (Harness
  servers via `harness stop --all`, `lms unload --all`, its own Ollama/llama-server processes) and waited until dedicated VRAM was
  back to the baseline measured at the start of the run.
- Same GGUF bytes for every tool (LM Studio via hard link, Ollama via `ollama create ... FROM <path>`, which copies the file into
  Ollama's blob store; those copies were removed afterwards). No pre-pulled library tags were used.
- Client: Python `httpx`, OpenAI `chat/completions`, streaming for speed tests (client-side timing: TTFT = first content or
  tool-call delta, decode tok/s = (completion tokens - 1) / (last token time - first token time)), `temperature 0`, `seed 42`.
  Server-reported timings (`timings.predicted_per_second`, `prompt_per_second`) are shown where the backend returns them
  (llama.cpp based servers do; Ollama and LM Studio do not in the OpenAI stream).
- Every request carries a unique nonce at the start so prompt caches do not make cold requests look cheap, except in the
  agent test where cache reuse is the thing measured.
- Speed: 1 warm-up discarded, 5 measured runs (decode), 3 measured runs (4k prompt processing), medians. Concurrency: 1 warm-up
  round, 3 measured rounds. Agent test: 1 warm-up conversation, 3 measured. Needle: 2 runs. Tool suite: 1 run per case.
- Tool-calling suite (`bench/compare/tools_suite.py`): 38 cases - single (5), choose among 8 (5), parallel (3), none expected (4),
  nested JSON (3), enum (3), Turkish (10), many tools / 38 definitions (5). A case passes only if the expected number of
  structured `tool_calls` came back with the right function names and arguments (valid JSON, expected values), and no call
  when none is expected. `parallel_tool_calls: true` is sent to all tools. A call written as text in `content` counts as a
  failure unless the backend converted it into a structured call; occurrences are counted separately.
- Token counts: the filler text is a deterministic generator calibrated against the Qwen tokenizer (4.54 characters per token);
  the prompt token counts reported by each server are printed in the tables.

## Caveats (read before quoting any number)

- The Windows desktop compositor (`dwm.exe`) held about 5.7 GB of the 16 GB GPU memory for the whole session, plus browsers and
  other apps; only about 10 GB were really free. This affects every tool, but tools estimate free memory differently
  (see the settings tables): llama.cpp's built-in fit sees the whole adapter and can over-commit into shared system memory,
  Harness measures free memory and adapts, Ollama and LM Studio have their own estimators. Shared-memory growth is recorded per run.
- Harness Studio ran as `harness serve --gateway` (the same core and gateway as the GUI app), not as the GUI window, in both runs; in the
  re-run the GUI was closed during the run and restarted from its normal installed location afterwards. The re-run
  used the installer-built release build (managed runtime b11556; the llama.cpp control from the first run still used b11552).
- The user's own Ollama instance is configured with `OLLAMA_FLASH_ATTENTION=1`, `OLLAMA_KV_CACHE_TYPE=q8_0`,
  `OLLAMA_NUM_PARALLEL=1`, `OLLAMA_MAX_LOADED_MODELS=1`. The benchmark used a separate `ollama serve` on port 11437 started with
  those variables removed (out_of_box) or set explicitly (equal), so the numbers describe stock Ollama behaviour, not that
  machine's tuning.
- LM Studio: the CLI cannot set flash attention or the KV cache type; the values it chose are recorded from its REST API. Just-in-time
  model loading was reported as inactive by the server log on this installation (that is the state the app was in, not changed
  by the benchmark).
- Qwen3-4B-Instruct-2507 and Qwen2.5-Coder-14B are only two models; tool-calling percentages depend on the model, the chat
  template each tool applies and the strictness of the expectations in `tools_suite.py`. The suite measures reliability under
  default sampling settings of the OpenAI API, not the best a tool can do with hand-tuned templates.
- Single machine, AMD/Vulkan only. NVIDIA/CUDA and Linux results will differ.
- The "equal" 14B runs request a 32768-token f16 KV cache that does not fit the free VRAM of this machine; tools that cannot
  adapt will spill into shared memory (slow), tools that adapt change the requested settings. Read the settings table for what
  actually ran.
