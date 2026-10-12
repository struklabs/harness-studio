Türkçe: ../tr/COMPACTION.md

# Context compaction: research and measurements

The research behind context compaction in the Playground, and measurements of the chosen method. Design and schema: [ARCHITECTURE.md §9.1](ARCHITECTURE.md) (context compaction, `harness_core::compaction`). Sources were read in October 2026. Items marked **[unverified]** could not be found in official documentation or source code.

## 1. What other tools do

| Tool | Trigger | What is kept | Notes |
|---|---|---|---|
| Claude Code | Automatic: before the window fills. On a 1M model about 967K (≈33K margin); `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` can only lower the threshold. Manual: `/compact [instruction]` | System prompt, CLAUDE.md, and the 5 most recently edited files are reloaded; the rest becomes a structured summary | Summary instruction via `/compact <instruction>` or the CLAUDE.md "Compact instructions". `/rewind` offers "summarize up to here" [1][2][3] |
| Anthropic API | `clear_tool_uses_20250919`: by default at 100K input tokens it clears old tool results and keeps the last 3; `clear_at_least` makes sure clearing is worth breaking the cache. `compact_20260112`: summary at 150K (minimum 50K) | Recent tool uses; the summary is a `compaction` block | Clearing tool results is the "lightest touch" [4][5][6] |
| OpenAI Codex CLI | 90% of the context (`model_auto_compact_token_limit` is capped at 90%) | Recent user messages kept verbatim up to 20K tokens | "CONTEXT CHECKPOINT COMPACTION… handoff summary": progress, decisions, constraints, next steps [7][8] |
| OpenAI Responses API | `/responses/compact` or `compact_threshold` | An opaque, encrypted `compaction` item | The default threshold is undocumented [9] |
| Claude.ai | Summarizes earlier messages when nearing the limit (if code execution is on) | The full history is stored | [10] |
| ChatGPT | **[unverified]** old messages are dropped silently, then a "maximum length" message appears | — | [11] |
| Cursor | Automatic at the limit, `/summarize` manually (1.6) | — | Reportedly writes the summary with a smaller model **[unverified]** [12] |
| OpenCode | Input limit − `reserved` (10K) ≈ 96–99% | `prune`: the most recent 40K tokens of tool output are kept; older ones become "[Old tool result content cleared]" | Summary template: Goal, Instructions, Findings, Done, Files [13][14] |
| Open WebUI | Off by default; 80,000-token threshold | 40% of messages kept | The summary model is the chat model [15] |
| LM Studio | `contextOverflowPolicy`: `stopAtLimit`, `truncateMiddle` (system + first message stay), `rollingWindow` | — | No summary, truncation only [16] |
| SillyTavern Summarize | Every 10 messages | The summary is placed in the prompt as a system message | Extends the previous summary [17] |
| Roo Code / Cline | Roo: threshold slider, default 100%; Cline: ~80% **[unofficial]** | — | Custom summary prompt, manual "Condense" [18][19] |
| Aider | History budget: input limit/16 (1024–8192) | Recent messages up to half the budget kept verbatim | The older part is summarized recursively with a "weak model" [20] |
| LangChain | `ConversationSummaryBufferMemory` 2000 tokens; `SummarizationMiddleware` last 20 messages | The oldest messages are folded into the summary | [21][22] |
| LlamaIndex | `ChatSummaryMemoryBuffer`: 75% of the context | — | "Write a concise summary…" [23] |

Findings:
- **Quality drops as the context grows** (context rot, 18 models), and information in the middle is recalled worst (lost in the middle). Compaction helps accuracy, not only fitting [24][25][26].
- **The weak point of summaries is concrete detail** (which file, which value): in Factory's evaluation of 36,611 messages the lowest score was "artifact trail" [27]. Anthropic recommends tuning "recall first, then precision" [24].
- **The llama.cpp prompt cache matches on prefixes:** `cache_prompt` is on by default; the server compares the new prompt with the previous one and processes only the differing tail. Replacing the leading messages with a summary invalidates almost the entire cache, so compaction must be **rare and in large chunks**. Context shift (`--context-shift`) is off by default; when it is off, a request that fills the context ends with an error [28].
- llama-server `POST /tokenize` (`content`) counts tokens; `GET /props` returns the per-slot context in `default_generation_settings.n_ctx`; `/v1/messages/count_tokens` counts including the template (the gateway passes it through) [28].

## 2. Chosen method and defaults

- **Rolling summary + recent turns verbatim:** the oldest turns become a single summary, the last N turns stay as they are. The next compaction folds the previous summary into the new one. The unit is a turn (user message + all assistant/tool turns): a tool call and its result are never separated.
- **Threshold 80% (default; a percentage or a fixed token count can be chosen):**
  - With a local model, the same model usually writes the summary, in the same context. The part to summarize (threshold − remaining turns) + the instruction + the summary output must fit in one request. At 80% in a 4096-token context, about 3.3K input + 512 summary fit. Codex's 90% or Claude Code's ~95% is for large-context cloud models; a small local context would need chunked summarization (supported, but slow).
  - In llama-server context shift is off by default: if the context fills, the request fails. The 20% margin leaves room for the new message and the reply.
  - It sits in the same region as Open WebUI (80K), LlamaIndex (75%), and Cline (~80%); acting early is also good against context rot.
- **Last 2 turns verbatim:** the latest topic and the reply style stay lossless. If these turns exceed 40% of the context, fewer are kept (Aider keeps half the budget, Open WebUI 40%).
- **Automatic compaction summarizes at least 10% of the context in text:** for less, breaking the prompt cache is not worth it (the same logic as Anthropic's `clear_at_least`). With less, the message is sent as is.
- **Long tool results are shortened (on by default):** the summarizer gets the first 300 characters plus how many characters were dropped. Which tool was called with what is kept (Anthropic, OpenCode).
- **Summary budget:** 1/8 of the context, 256–2048 tokens (Anthropic sub-agent summaries are 1–2K).
- **The summary goes in the system message:** a `<conversation_summary>` block is appended to the end of the system prompt. Some Jinja templates (Gemma, Mistral) reject two consecutive user messages, while a system message at the start is accepted by every template.
- **Documents are not summarized:** they are retrieved again on every turn; the summary request receives only their names.
- **Not applied when there is no gain:** if the summary + remaining turns are not shorter than the original prompt, compaction is discarded (`compaction.no_gain`). Observed in testing: summarizing a single turn made a 2,108-token prompt grow to 2,243 tokens with a 940-token summary prompt.
- **Reversible:** the original messages are never deleted; undoing a compaction stops automatic compaction on that branch (otherwise it would be redone on the next message).

## 3. Strategy comparison (real model)

`cargo test -p harness-core compaction::e2e::compare_summary_strategies -- --ignored --nocapture` (`HARNESS_E2E_RUNS=3`). llama-server b11312 (Vulkan), Qwen3-4B-Instruct-2507 Q4_K_M, `-c 4096`, single slot. The conversation was built with the model itself: 8 facts (code name, budget, meeting date/time, team lead, IP, cat name, report format, delivery date), followed by long technical answers up to 75% of the context (27 messages, 3,224 tokens). Each strategy compacted a copy of the chat, keeping the last 2 turns. Then the 8 questions were asked separately (temperature 0); a fact counts as "recalled" if the answer contains the expected value.

| Strategy | Recalled | Prompt (tokens) | Summary (tokens) | Time | Requests |
|---|---|---|---|---|---|
| No compaction (full history) | 8/8 | 3246 | — | — | — |
| Truncation (no summary, last 2 turns) | 1/8 | 879 | 0 | — | — |
| `brief` (one paragraph, "concisely") | 1/8 · 1/8 · 1/8 | 1257–1295 | 338–376 | 4.6–7.0 s | 1 |
| **`structured`** (sectioned, concrete details verbatim) | **8/8 · 8/8 · 8/8** | 1351–1394 | 432–475 | 6.2–9.0 s | 1 |
| `incremental` (same prompt, in chunks of 1/4 of the context) | 8/8 · 8/8 · 8/8 | 1432–1433 | 512 (budget filled) | 16.4–21.3 s | 3 |

- `brief` summarizes the technical topics and drops the facts; it is no better than truncation. Only a general preference such as "report PDF" survives.
- `structured` keeps every fact, shrinks the prompt by 58%, and finishes in one request. **This is the default.**
- `incremental` has the same accuracy but is 2–3× slower, and the summary fills the budget and is cut off. The chunked path is used only when the part to summarize does not fit in one request (a very large fixed threshold, or a summary model with a small context); in that case the `structured` prompt carries the summary across chunks.

Automatic flow (`auto_compaction_with_a_real_model`): after the 7th filler turn, usage 3310 + 30 ≥ 3276 (80%) triggered compaction. 26 messages (13 turns) were summarized and 2 turns remained; 5.9 s, one request. The computed prompt went from 3312 to 1353 tokens (by the server's tokenizer), and the next real request was 1373 tokens. The model correctly answered "KESTREL-47" and "Zeynep Arıkan" from the summarized part. After undoing, the request returned to 31 messages and 3314 tokens.

## 4. Within a response turn (long tool loop)

A single response can fill the context through tool turns; the trigger that runs before a new user message cannot catch that. After each tool turn, and before the next request, the Playground checks the same threshold and compacts if needed (ARCHITECTURE.md §9.1, "Compaction within a response turn"). Unlike [Anthropic's tool-result clearing](#sources), the method does not delete results: the older turns and their results are summarized (results are given to the summarizer as their first 300 characters), the last turn and the question stay verbatim, and the marker can be undone.

With a real model and real tools (`compaction::e2e::compaction_in_the_middle_of_a_tool_loop`, Qwen3-4B-Instruct-2507 Q4_K_M, `--jinja`; the model reads `docs/ROADMAP.md`, `docs/ARCHITECTURE.md`, and `docs/FEATURES.md` one after another with the `read_file` tool, then states each file's title; each read is about 2,400 tokens):

| Attempt | Context / threshold | Result |
|---|---|---|
| First version: the question is not in the summary, and the summary does not count "calls made" | 8192 / 45% | Compaction triggered on every turn. The summary misread the question as "the user asked for a summary" and wrote "no tool was called"; the model read the same files again and could not finish in 9 turns. |
| The question is given to the summarizer as context, and the instruction makes it count the calls; the question stays verbatim in the request | 12288 / 40% | One compaction after turn 1: 4385 → 2240 tokens (summary 300 tokens). The model read the remaining two files and finished in 4 requests; the first file's title (from the summarized turn) was stated correctly. |

Findings: (1) The question being answered must travel verbatim next to the summary rather than inside it, and the summarizer must see it; otherwise the summary is written for the wrong target. (2) The summary must explicitly carry "which tool was called", or the model repeats the work. (3) If the threshold is too tight for results of several thousand tokens per file, compacting on every turn gains nothing; the `compaction.no_gain` and "one failed attempt per response" safeguards exist for this reason.

## Sources

1. https://code.claude.com/docs/en/context-window
2. https://code.claude.com/docs/en/costs
3. https://code.claude.com/docs/en/model-config and https://code.claude.com/docs/en/env-vars
4. https://platform.claude.com/docs/en/docs/build-with-claude/context-editing
5. https://platform.claude.com/docs/en/build-with-claude/compaction-threshold
6. https://platform.claude.com/docs/en/build-with-claude/compaction
7. https://raw.githubusercontent.com/openai/codex/main/codex-rs/protocol/src/openai_models.rs
8. https://raw.githubusercontent.com/openai/codex/main/codex-rs/core/src/compact.rs and https://raw.githubusercontent.com/openai/codex/main/codex-rs/prompts/templates/compact/prompt.md
9. https://developers.openai.com/api/docs/guides/compaction
10. https://support.claude.com/en/articles/11647753-how-do-usage-and-length-limits-work
11. https://www.techradar.com/ai-platforms-assistants/chatgpt/i-pushed-chatgpt-toward-its-hidden-chat-limit-heres-what-actually-happens-when-you-reach-it
12. https://cursor.com/changelog/1-6
13. https://opencode.ai/docs/config/
14. https://gist.github.com/badlogic/cd2ef65b0697c4dbe2d13fbecb0a0a5f (source-code review, third party)
15. https://docs.openwebui.com/troubleshooting/context-window/
16. https://lmstudio.ai/docs/typescript/api-reference/llm-prediction-config-input
17. https://raw.githubusercontent.com/SillyTavern/SillyTavern/release/public/scripts/extensions/memory/index.js
18. https://roocodeinc.github.io/Roo-Code/features/intelligent-context-condensing
19. https://cline-cline.mintlify.app/models/context-windows and https://github.com/cline/cline/issues/14329
20. https://raw.githubusercontent.com/Aider-AI/aider/main/aider/models.py and https://raw.githubusercontent.com/Aider-AI/aider/main/aider/prompts.py
21. https://raw.githubusercontent.com/langchain-ai/langchain/langchain==0.3.27/libs/langchain/langchain/memory/summary_buffer.py
22. https://raw.githubusercontent.com/langchain-ai/langchain/master/libs/langchain_v1/langchain/agents/middleware/summarization.py
23. https://raw.githubusercontent.com/run-llama/llama_index/main/llama-index-core/llama_index/core/memory/chat_summary_memory_buffer.py
24. https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents
25. https://www.trychroma.com/research/context-rot
26. https://arxiv.org/abs/2307.03172
27. https://factory.com/news/evaluating-compression
28. https://raw.githubusercontent.com/ggml-org/llama.cpp/master/tools/server/README.md
