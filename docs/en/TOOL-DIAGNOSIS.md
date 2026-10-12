Türkçe: ../tr/TOOL-DIAGNOSIS.md

# Tool-call diagnosis

The question: "I keep getting tool errors with LM Studio + OpenCode. Who is at fault?" A tool turn involves three parties, and the type of error points to the culprit:

- **Server / template (runtime):** llama-server and the model's chat template. It parses the call and puts the tool definitions in front of the model.
- **Model:** the party that writes the call and picks the right tool name and valid arguments.
- **Client (OpenCode, Claude Code, Codex...):** the party that runs the tool and sends the result back in the next request.

From the outside, all of them look like "the tool did not work". Because the gateway sees every request and response, it classifies the turn and writes the result next to the request record. Code: `harness_core::gateway::tool_diag` (UI-independent, no `axum`); the gateway only wires it in (`crates/harness-gateway/src/proxy.rs`).

## What is measured

### Recovering a call left in the text

The `call_left_in_content` diagnosis stays valid for the raw upstream response; the diagnosis classifier does not change the response. Recovery lives in the agent layer: the single parser in `tools/text_call.rs` is used directly by the Playground's Tauri `tools_recover_text_calls` command and by the core's `run_agent` and `run_agent_stream` loops. Plain proxy requests are not changed automatically; the gateway's server-tool agent loop applies recovery.

Conditions: there is no real `tool_calls`, tools were offered in that request, the name inside the complete JSON is one of the offered names, and `arguments`/`parameters` (string-JSON included) is an object. Strong wrappers (`<tool_call>`, `<tools>`, `<function=name>`, `[TOOL_CALLS]`, `<|python_tag|>`) are recovered even inside an explanation. Weak forms (any language-tagged code block, XML included; bare JSON/arrays/consecutive objects) are recovered only when, once the call(s) are extracted, nothing remains but whitespace or at most one line/120 Unicode characters of lead-in before the calls. If there is a trailing explanation or an unknown/invalid array member, a weak call is not recovered and the text stays as is. The threshold accepts a short "I'm searching." lead-in and rejects multi-line narration and a trailing usage explanation; the intent of an example that carries only a short lead-in cannot be told apart from the format with certainty. The normal permission/schema/approval path is kept.

The assistant `tool_calls` with ids and the matching `tool_call_id` are written to the history; the card keeps its TR/EN recovery badge. Cancellation/errors and `length`/`content_filter` are not recovered. Normal text streams instantly even when tools are on; only the part after a possible call start and a partial marker suffix is held back, and if nothing is recovered it is shown unchanged. The TS and Rust stream filters use the same marker list in `shared/text-tool-call-markers.json`; parsing the full response is in Rust.

Startup review: the `process.rs` catalog handles `params.jinja=true` → `--jinja` and `false` → `--no-jinja`; `params.toml` defines the two flags. Jinja being on does not guarantee that the model's every call is parsed in its native format: the Qwen2.5-Coder-14B record in the repository leaves JSON inside `<tools>`. This work does not change the template or the saved startup settings.

A request counts as **involved in a tool turn** if the request sends `tools`, or if the conversation contains a tool result (`role: tool`, `tool_result`, `function_call_output`). Requests without tools write nothing (`tool_involved = 0`). Three wire formats are read: Chat Completions, Anthropic Messages, and Responses; as JSON and as SSE (streaming). Response text and tool calls are collected with bounded memory while streaming (text 32 KiB, arguments 64 KiB per call); this is independent of the content-recording setting and no content is stored, only the evidence for the finding (tool name, a short excerpt, the schema path) is written to `tool_diag_detail`.

The following are written to every request (`analytics.db` schema **v5**, `requests` table):

| Column | Meaning |
|---|---|
| `tool_involved` | whether it took part in a turn |
| `tools_offered` | number of tools in the request |
| `tool_calls` | number of tool calls in the response |
| `tool_diag_category` | `runtime` / `context` / `model` / `client`; empty if there is no problem |
| `tool_diag_code` | the exact finding code |
| `tool_diag_detail` | JSON evidence |

The migration uses `ALTER TABLE ... ADD COLUMN`: old rows are kept and read as "no tool" (tested: a v4 database opens with its data). The partial indexes `requests_tool` and `requests_tool_issue` keep filtering cheap.

## Rules

Classification, `classify(request, response, turn)`, is a **pure function**: the same input gives the same result. A turn gets at most one **primary** finding; the first match in the order below wins (solid evidence first). If the primary finding is a model/server finding and the previous turn's tool result is also an error, the client error is stored in the evidence as `also` / `client`.

| Category | Code | When |
|---|---|---|
| context | `context_exceeded` | llama-server returned `exceed_context_size_error` (the prompt did not fit the context); evidence: `n_prompt_tokens`, `n_ctx` |
| context | `output_truncated` | the finish reason is `length` and the response was cut inside a call (half an argument or half a marker) |
| runtime | `template_no_tools` | tools were sent, there is no call, and the template does not support tools (`/props` → `chat_template_caps.supports_tools = false`) |
| runtime | `call_left_in_content` | there is no call but the text carries a known tool marker (`<tool_call>`, `<tools>`, `<function=`, `[TOOL_CALLS]`, `<\|python_tag\|>` ...) or the response begins with a JSON object containing `name` + `arguments` |
| runtime | `stream_broken` | in the stream a call fragment arrived without `function` or `name`, a fragment is not JSON, or the finish reason says call but there is no call |
| context | `context_near_full` | tools are present, there is no call, and (prompt + response) / context >= 90% (context shift drops the tool definitions) |
| model | `invalid_json_args` | the arguments are not a JSON object |
| model | `unknown_tool` | the called name is not among the request's tools |
| model | `schema_mismatch` | the arguments do not match the tool's schema (`type`, `enum`, `required`, `minimum`/`maximum`, `items`, `additionalProperties`...; evidence: the field path and the rule) |
| model | `repeated_call` | the same call (name + arguments, independent of key order) for the third time in the conversation: if it was tried twice and the result did not change, it is a loop |
| model | `no_call_when_required` | `tool_choice` required a call (`required` / `any` / a named function) and there is none |
| client | `tool_result_error` | the tool result the client sent in this request is an error: `is_error: true`, a `{"error": ...}` / `success: false` JSON, or the text begins in an error form (`Error`, `No match found`, `ENOENT`, `Permission denied`...) |

Deliberate choices:

- Text patterns in tool results are **anchored at the start** (they begin with "Error: ..."); a file that merely contains the word "error" is a normal result. Only results after the last assistant turn are counted (the earlier ones were already explained by earlier rows).
- "No tool call" alone is not a problem: with `tool_choice` on auto, the model answering in text is valid. The problem starts when a call is required or when the call is left in the text.
- Another upstream error (4xx/5xx, other than context) produces no diagnosis; the status field already says it.
- Schema validation is strict because it measures the model's flaw (unlike the runtime's `validate_args`, it does not accept numbers as text). It accepts keywords it does not know (`$ref`, `pattern`, `format`): where it cannot be sure, it stays silent.

### Linking a client error to the earlier request

A client error appears in the **next request**; the call id is used to find which call it belongs to, with no guessing: llama-server writes an id for each call, and the client sends the result back with the same id (`tool_call_id` / `tool_use_id` / `call_id`). The gateway keeps call ids in an in-memory index of the last 4096 calls (`CallIndex`); when a result arrives the id is looked up and `linked_request` (the id of the request that made the call) is added to the evidence. A client that makes up its own ids is not linked; the diagnosis still says "the tool errored on the client side". The index is emptied when the gateway restarts.

## Model level (native / generic / limited)

It is not guessed from the model name; it is **measured** from the gateway log. A turn with `tools_offered > 0` is:

- **success:** a clean call (there is a call, no runtime/model finding);
- **failure:** a runtime or model finding;
- not counted: a text answer, a context finding, a client error, a cancellation (not the model's flaw).

The success rate is `ok / (ok + bad)`; at least **5 turns** are required, otherwise the level stays "not measured yet".

| Level | Condition |
|---|---|
| `native` | rate >= 0.85 (and the template supports tools or this is unknown) |
| `generic` | 0.50 <= rate < 0.85; or the rate is high but the template does not support tools (calls work through llama.cpp's generic format) |
| `limited` | rate < 0.50 |

The level is shown in the agent-model note in Connections (`AgentModelNotes`), under the existing tool-call verification (`Verified` / `Failed` / ...); the fields of `harness_core::integrations::agent_models::AgentModel` are `tool_rounds_ok`, `tool_rounds_bad`, `tool_level`, `recommended_tools`.

### Recommended maximum number of active tools

Every active tool takes space from the context (its definition is about 150-250 tokens) and enlarges the list the model must choose from; small models quickly make mistakes with a long list (OpenCode and Claude Code send 10-30 tools). The rule depends on model size:

| Parameters | Maximum tools |
|---|---|
| < 2 B | 4 |
| 2 - 5 B | 8 |
| 5 - 10 B | 12 |
| 10 - 20 B | 20 |
| >= 20 B | 32 |
| unknown | 8 |

The measured level lowers the cap: `generic` at most 8, `limited` at most 4. This is a recommendation, not a limit; the gateway does not restrict the number of tools. The first real OpenCode measurement is below; changing the thresholds and size steps requires broader, comparative evidence.

## Threshold measurement

### Setup and method

Windows 11, Ryzen 7 9700X. Because the RX 7900 GRE had only about 1.5 GB of free VRAM, all four models ran on the **CPU**. The official [llama.cpp b11342 Windows CPU build](https://github.com/ggml-org/llama.cpp/releases/tag/b11342) was used with the chat template inside the GGUF and `--jinja -ngl 0 -c 32768 -np 1 -t 8`; the model files were opened read-only from their existing location. The gateway loaded only one measurement model at a time.

The prebuilt headless `harness` binary (0.1.0) was run with `harness serve --no-restore --gateway --gateway-port 11535 --port 11536`. `HARNESS_DATA_DIR`, the analytics database, and temporary working files were in a throwaway directory outside version control. In the gateway process, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `CLAUDE_CONFIG_DIR`, and `CODEX_HOME` were also redirected under that directory; `HARNESS_DATA_DIR` alone is not enough to isolate client settings.

OpenCode **1.18.34** came from a separate npm install and was run with `opencode run --pure --model measurement/<alias> --format json`. `OPENCODE_CONFIG`, `OPENCODE_CONFIG_DIR`, the four XDG directories, and `OPENCODE_TEST_HOME` were isolated; project settings, Claude Code settings, external skills, and model-catalog reading were turned off. The fixture is a separate git repository; the inherited `PWD` and `INIT_CWD` were also pinned to the fixture. `small_model` is used for title generation; these helper requests, in which no tools are offered, do not enter the level calculation.

Eight fixed tasks were given in separate sessions, each time with the same starting files: read the first line of the README, list the `.txt` files with glob, find the `NEEDLE_642` line with grep, edit `mode=old` → `mode=new`, write a file containing `CREATED_642\n`, `node --version`, read/edit/re-read, and write a summary from two files and re-read it. The tools are OpenCode's own list of `bash`, `edit`, `glob`, `grep`, `read`, `skill`, `todowrite`, `webfetch`, `write` (**9 tools**). No client-specific shortened system prompt or hand-made tool request was used. The real prompt of this isolated version came to about 6.5 thousand tokens; an earlier observation of ~29 thousand tokens did not recur in this setup.

The output limit was 2048 tokens, `tool_choice: auto`, the agent step limit 8, and the task timeout 600 seconds. Temperature/seed were not forced on the request side; the results belong to this version's default sampling. Task success was assessed separately by examining OpenCode's answer together with the actual file contents in the fixture; an "I did it successfully" sentence was not accepted as enough. Even if a parallel response contains three calls, **one gateway request is one turn**.

Counts were computed from the isolated `analytics.db` under the same conditions as `analytics/db.rs::agent_signals` and `agent_models.rs::build`: `tools_offered > 0`, `cancelled = 0`; `ok` if there is a call and the primary category is not `runtime`/`model`, and `bad` if the category is one of those two. The presence of `tools`/`tool_call` in the GGUF template and the tensor parameter total were checked with the product's GGUF reading rule. `MIN_ROUNDS = 5`, 0.85 / 0.50, and the existing tool recommendations were applied as they are. A detail of the code: a `client`/`context` row that carries a call can be `ok` in the current SQL; the category breakdown is given separately.

### Results

| Model | Parameters | Tools in template | Counted turns | Clean (`ok`) | Failed (`bad`) | Rate | Level | Recommended tools | Task success (verified against files) | Avg. task time |
|---|---|---|---|---|---|---|---|---|---|---|
| Qwen2.5-0.5B-Instruct Q4_K_M | 630 M | yes | 8 | 8 | 0 | 1.00 | native | 4 | 3/8 | 42 s |
| Qwen2.5-1.5B-Instruct Q4_K_M | 1.8 B | yes | 9 | 9 | 0 | 1.00 | native | 4 | 6/8 | 74 s |
| Qwen3-4B-Instruct-2507 Q4_K_M | 4.0 B | yes | 14 | 14 | 0 | 1.00 | native | 8 | 7/9 | 80 s |
| gemma-3-4b-it Q4_K_M | 3.9 B | no | 8 | 0 | 8 (`runtime`: `template_no_tools`) | 0.00 | limited | 4 | 0/8 | 10 s |

Uncounted requests: title/helper requests with no tools offered (8-9 per model) and turns that ended in text without calling a tool. No model had a finding in the `model` category (invalid JSON, a nonexistent tool, arguments that do not match the schema); llama.cpp enforces the tool name and schema with a grammar. Qwen3-4B's `multi` task was tried twice, and in both the summary file was written incompletely. In every task gemma-3-4b wrote the call as plain text inside a ```` ```tool_code ```` block or gave only an introductory sentence; OpenCode ran no tools.

### Observations and decision

- **0.85 / 0.50 unchanged.** The measured rates clustered at the extremes (the three Qwen models 1.00, gemma 0.00); no model fell between the thresholds, so there is no evidence to move them. All four models landed at a level that fits their observed call behavior: the Qwen models gave calls as structured `tool_calls` every turn, and since gemma's template does not support tools, no runnable call ever appeared.
- **The level does not measure task success.** The clean-call rate describes only the format and schema of the call: the 0.5B model was `native` but completed only 3/8 of the tasks correctly (an empty read range, believing an unchanged file had changed, writing the task text instead of a summary). The level in the UI should therefore be read as "tool-call format"; semantic success is not visible separately.
- **Size steps unchanged.** OpenCode sent 9 tools. The recommendation for models under 2 B is 4: these models struggled noticeably with 9 tools (3/8 and 6/8). Qwen3-4B's recommendation is 8: with 9 tools it got 7/9 and failed only the multi-step task. The observation does not contradict the steps. Steps above 5 B were not measured in this round (the 14B size did not fit in VRAM).
- **Limits:** a single client (OpenCode 1.18.34), 8-14 turns per model, CPU, default sampling; Claude Code was not measured. The thresholds should be re-evaluated when in-between rates (0.5-0.85) are seen.

In the first eight tasks of the 0.5B model, the concrete result was 3/8 (listing, a one-line edit, shell). On a read it asked for an empty range with `limit: 0`; on a new file it dropped the final line ending; in a multi-step edit it said it changed a file without changing it; in the summary file it wrote the description from the task instead of the real values. Because these carried a valid tool name and JSON/schema, the clean-call rate stayed high. Grep reported the file but did not give the requested line number.

The 1.5B model produced concrete results on 6/8 of the first eight tasks. Grep lowercased the searched constant and missed the match. In the summary file it wrote `$(cat ...)` expressions as **plain text** and then claimed it had made a correct summary. The other file operations and the shell task were completed. So the `native` level cannot be read here as "completes agent tasks reliably"; it describes only the observed call format/schema success.

### Measurement isolation

In the measurement, the gateway's `HARNESS_DATA_DIR`, `USERPROFILE`, `APPDATA`, and `LOCALAPPDATA` directories are separated; the digest of the real client configuration files does not change during the measurement.

This work exposed a product bug that has been fixed (`integrations::drift::follow_gateway_port`): port following always treated the default 11435, on which the gateway never listened, as the "old gateway port", and an instance with a separate data folder could pull the user's real client settings to its own port. The migration now happens only from ports in this data folder's gateway port history; the warning (address-drift detection) continues to count the default port.

## Surfaces

- **Request Inspector** (Analytics): a one-line "category: finding" in the row (the recommendation on hover), a diagnosis section in the detail drawer (title, recommendation, evidence, link to the request that made the call), and a "Tool issue" filter (all / with issues / clean turns / category).
- **Control API:** `GET /analytics/requests?tool_issue=true` and `tool_category=runtime|context|model|client`; `RequestRow` carries the new fields.
- **CLI:** `harness requests --tool-issue [--tool-category model]`; `harness requests <id>` prints the "Tool diag" lines.
- Recommendation texts are in the UI language (`src/lib/locales/toolDiag.ts`); the core produces only codes and evidence. The context recommendation ("32k") is computed from the evidence: `n_prompt_tokens × 1.25 + 2048` or twice the current context, whichever is larger, rounded to the steps 8k / 16k / 32k / 64k / 128k / 256k.

## Tests

Unit tests run with **real llama-server captures** (`crates/harness-core/tests/data/tooldiag/`, b11342, file name: `b11342-<model>.<status>.<req|resp>`): Qwen3-4B-Instruct-2507 (a clean call as JSON and SSE in three wire formats, an answer after a tool result, an answer after an error result, a call cut by `max_tokens`, a context-overflow error body, an answer with no call when a call is required), Qwen2.5-Coder-14B (a call left in the text with `<tools>`, JSON and SSE), gemma-3-4b (no tools in the template), Qwen2.5-0.5B and gpt-oss-20b (a call repeated three times). Because llama.cpp enforces names and schema with a grammar, no real "nonexistent tool name" or "arguments that do not match the schema" could be captured on these models; those two rules are tested by evaluating a really captured response against another request's tool list (stated in the test description), and the schema validator has its own unit tests. The bytes the client itself writes (the Anthropic `is_error` result) are client data, not real model output.

End to end (`#[ignore]`, `crates/harness-gateway/tests/tooldiag_e2e.rs`): through the gateway with a real llama-server: a local call is clean in three wire formats, a client error is linked to the earlier request, truncation at the output limit, a call left in the text (JSON and streaming), a model whose template does not support tools; a request with no tools is not part of a turn.

## Gateway text-call recovery

In Settings > Gateway > Routing, **Convert Tool Calls Left in Text into Real Calls** is enabled per model. `GatewaySettings.text_tool_calls` is a map of definition id → boolean; it is **on by default** (Qwen2.5-Coder-14B writes every call as `<tools>{json}</tools>` text; with recovery off the tool test scored 5/38, and with it on 36/38). Only models for which it is turned off are saved as `false` (`settings.json` > `gateway.text_tool_calls`: `{"<definition id>": false}`); with no record, recovery runs. If the request has no tools, the gateway does not touch the response at all. The switch is visible for each model under Settings > Gateway > Routing > "Convert Tool Calls Left in Text into Real Calls". It is saved as the same gateway setting as server-side tools; a change applies to the next request, and the model is not restarted. For a model whose tool level is `limited`, Connections shows a link to this setting.

Recovery is applied in `proxy.rs`, in the Chat Completions request after the compatibility step, with the tools offered in the request, and only on a successful (200) response. The `harness-core::tools::text_call` parser and `TextCallStreamBuffer` are reused; the markers come from `shared/text-tool-call-markers.json`. Strong wrappers are accepted inside an explanation as well; fenced/bare JSON is accepted only if it follows the core's rules for a response consisting only of a call / a short lead-in. Tool schema echoes, unoffered names, responses that contain a native call, and `length`/error/incomplete endings are not recovered. Validating arguments against the schema and running the call are the client's responsibility; the gateway converts only the wire format.

In JSON, for the single `choices[0].message`, `tool_calls` is written, and if empty, `content: null` and `finish_reason: tool_calls`; the explanation and usage are kept. Multi-choice responses are not converted. In SSE, ordinary text is forwarded as it arrives, and text after a possible call marker is buffered. On the `stop` event the shared parser evaluates the full text; the calls are forwarded before the finish event as `delta.tool_calls` events with index/id/type/function.name/function.arguments. If there is no recovery, the buffer is sent unchanged as content. The upstream usage and `[DONE]` order are preserved; UTF-8/TCP boundaries and CRLF can be split. If the full response text exceeds 1 MiB, recovery is abandoned and the buffer is flushed.

In JSON, `X-Harness-Compat: text_tool_calls=<n>` is a normal response header. Because SSE headers are sent before the normal text and the count is known only at the end, `Trailer: x-harness-compat` is declared; the count goes after `[DONE]` as an **HTTP trailer** of the same name (an HTTP/1 client must send `TE: trailers`). Clients that do not read trailers can see the count from the analytics record. It does not buffer the whole stream from the start, so it does not delay TTFT.

Diagnosis keeps observing the upstream bytes: the `call_left_in_content` finding is kept, `recovered: true` and `recovered_calls` are added to `tool_diag_detail`, and the count is written to the `compat` record. `tool_calls` is the native counter; recovered calls are not counted as native successes. The analytics request detail shows the number of recovered calls and explains the recovery in the recommendation text. Recovered ids are recorded in `CallIndex`; a later client tool-error result can be matched to the first request. Even if an SSE client closes the connection after `[DONE]`, the recovery marker is kept in the record. If the old classifier does not recognize a format that the shared parser knows, the recovery evidence is recorded as `call_left_in_content`; the earlier different finding is kept in `upstream_finding`.

Anthropic Messages and Responses are not translated to Chat Completions in the gateway; they are forwarded upstream to the same endpoint. This setting applies only to the Chat Completions proxy path. The server-side tool loop is a separate path and continues to use the existing core recovery; this flag does not change its behavior. The Ollama format is out of scope.
