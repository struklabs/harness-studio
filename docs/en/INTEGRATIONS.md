Türkçe: ../tr/INTEGRATIONS.md

# Integrations and `harness launch`

The reference for the Connections > Tools and Clients screen, `harness launch`, and persistent integrations. Formats were written according to the official documentation linked below (last checked September 2026). Rows marked "Not verified" were read from documentation but not tried with a real client.

Code: `crates/harness-core/src/integrations/` (`keyed.rs` the edit and undo engine, `launch.rs` launch plans, `conntest.rs` the connection test, `mod.rs` the registry), `crates/harness-cli/src/launch.rs`, `src/lib/components/connections/`.

## 0. Windows launch, Zed, and VS Code notes

- GUI path: `LaunchDialog.svelte` → the `launchTool` IPC → `launch_integration_tool` → `LaunchInput/LaunchPlan`. An OpenCode npm install resolves `opencode.cmd` → `opencode.exe`; no arguments, with the environment `OPENCODE_CONFIG_CONTENT` + `HARNESS_API_KEY`. The Harness CLI is not part of this GUI chain. The CLI's `spawn_and_wait` runs the client in the same console and waits; the GUI's wait thread only does cleanup.
- Windows console handling: opening a new console window does not by itself connect the client's input/output/error streams to the console (verified: a test with the old `CREATE_NEW_CONSOLE` + redirected parent handles gave `True/True/True` for input/output/error). The private `launch.ps1` connects the streams with `CreateFile(CONIN$/CONOUT$)`, `SetHandleInformation`, and `SetStdHandle`. `.NET ProcessStartInfo` starts the client without redirection (PowerShell's normal `&` call is not used because it can create output pipes). For `.cmd/.bat`, `cmd /d /s /c` is used; there is no `start`, `DETACHED_PROCESS`, or `wt.exe` hand-off chain. Windows chooses the new console's host. The plan/token is in a private file, not in process arguments.
- `windows_launcher_reconnects_all_tools_to_console_and_keeps_plan`: real plan environments of five tools, arguments with Unicode/spaces/apostrophes/metacharacters, the working directory, removed environment variables, and file cleanup; the streams are `False/False/False` under native and npm batch simulation. A double quote/control character in a batch argument is safely rejected.
- Zed's [official key documentation](https://zed.dev/docs/ai/use-api-access#api-keys-and-environment-variables) says keys are kept in the credential store instead of settings.json, and that an OpenAI-compatible provider uses an environment variable in upper snake case + `_API_KEY`. The [source code](https://github.com/zed-industries/zed/blob/main/crates/language_models/src/provider/open_ai_compatible.rs) returns `NoApiKey` if there is no key. `Harness Studio` → `HARNESS_STUDIO_API_KEY`. You need to copy the key from the row and paste it into the Zed LLM Providers → Harness Studio field; for a keyless gateway `harness-local` is enough. Persistent environment variables are not changed.
- VS Code's [current official format](https://code.visualstudio.com/docs/agent-customization/language-models#add-a-custom-endpoint-model) uses a root array, `vendor: customendpoint`, `apiType: chat-completions`, `models[].url`, capability/token limits, and, when needed, `apiKey: ${input:...}`. The existing writer is compatible; `github.copilot.chat.customOAIModels` is not written. The [1.122 release notes](https://code.visualstudio.com/updates/v1_122#_custom-endpoint-provider-in-stable) confirm Stable support; the older OpenAI Compatible path was in Insiders. For `[]`, Detected is only an installation detection, and Configure has not been done yet. An isolated test verifies configure/update/undo.
- Real user settings and persistent environment variables are never changed by these tests.

## 1. Endpoint, model name, and key

- If the gateway is on, everything is written to the **managed endpoint** (`http://127.0.0.1:11435/v1`) and the model name is the Model Definition's alias. If the model is not loaded, the gateway loads it on the first request (JIT).
- If the gateway is off, the selected server's direct address and the model name the server reports are used.
- **When the port changes:** Every application (`apply`) writes its request to the ledger without the key (`LedgerEntry.request`, `StoredRequest`: address, model, context, model list, whether there was a key). If the gateway falls back to a spare port because 11435 is held by another program, or returns from the spare, the settings Harness wrote for one of the gateway's earlier ports (`config/ports.json` → `gateway_history`) are rewritten to the new address at startup (`drift::follow_gateway_port`; only the address changes, and the tool's default model is not touched). Settings written with a key and settings written to another loopback port (for example a llama-server's own port) do not change by themselves: Connections shows a "Tool is set to a different address" warning and a **Fix** button (`check_integration_endpoint_drift`, the normal preview/apply flow). Settings applied before this record existed (no request in the ledger) are evaluated from the file itself: if the address Harness wrote in the tool's file (OpenCode/Kilo/Open Design `provider.harness.options.baseURL`, Claude Code `env.ANTHROPIC_BASE_URL`, Codex `model_providers.harness.base_url`, Continue `apiBase`, VS Code, Zed, Goose, LibreChat) is one of the gateway's past ports (`gateway_history` and the default 11435) and differs from the current port, the card shows the same warning. If there is no key, only the address's port is changed in place (`keyed::retarget`: a backup is taken, comments and other settings are preserved, ledger hashes are updated; Undo keeps working); the user's address elsewhere or another port is not touched, and if there is a key, the file is not touched and the warning + Fix remain. The check runs once at every gateway start; if the gateway returns to the default port (the configured port is tried first if 11435 is free again), settings written to 11436 are moved back. The spare-port notice in Connections/Gateway states how many client settings were actually moved and which are waiting for Fix (`GatewayStatus.clients_moved/clients_pending`).
- Key:
  - A loopback gateway requires no token by default; tools get the placeholder `harness-local`.
  - `harness launch` and the GUI's "Launch" give the gateway client token (`<data>/gateway-token`, which changes at every start) only to the child process's environment. On a remote instance, `HARNESS_TOKEN` (a named token with the `inference` scope) is used.
  - On a direct endpoint, the llama-server key (`--api-key` / `HARNESS_SERVER_API_KEY`) is used.
- Anthropic-style clients (Claude Code, Claude Desktop, the Anthropic SDK, `langchain_anthropic`) want the base address **without `/v1`**; they add `/v1/messages` themselves. `@ai-sdk/anthropic` wants it with `/v1`. Everything OpenAI-style uses `/v1`.

## 2. `harness launch`

```
harness launch claude|codex|opencode|aider|goose [--model <alias|name|file.gguf>]
               [--prepare | --no-prepare] [--direct] [--dry-run] [-- <tool arguments>]
```

Flow: model selection (a list on a TTY, otherwise `--model` or the only running model) → context check → "Prepare Server" if needed → endpoint (a JIT warm-up request if there is a gateway, otherwise `/load`) → the tool starts with only the process environment and temporary files → the temporary folder is deleted when it exits. The exit code is the tool's. No global configuration is written.

| Tool | Minimum / recommended context | How it is passed |
|---|---|---|
| Claude Code | 32,768 / 65,536 | Environment: `ANTHROPIC_BASE_URL` (host, no `/v1`), `ANTHROPIC_AUTH_TOKEN` (Bearer; no sign-in prompt appears), `ANTHROPIC_MODEL`, `ANTHROPIC_DEFAULT_{OPUS,SONNET,HAIKU}_MODEL`, `CLAUDE_CODE_SUBAGENT_MODEL`, `CLAUDE_CODE_MAX_CONTEXT_TOKENS`, `CLAUDE_CODE_MAX_OUTPUT_TOKENS`, `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`, `CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS=1`, `ANTHROPIC_CUSTOM_HEADERS: X-Harness-Client: claude-code`. `ANTHROPIC_API_KEY` and the Bedrock/Vertex/Foundry variables are removed. The same values are also written to a temporary file and passed with `--settings <file>`: `env` in `~/.claude/settings.json` overrides the process environment, and `--settings` overrides that. |
| Codex | 32,768 / 65,536 | `-c model_provider=harness -c model_providers.harness.{name,base_url,wire_api=responses[,env_key=HARNESS_API_KEY]} -c web_search=disabled [-c model_context_window=N] -m <alias>`. Codex speaks only the Responses API. `web_search` is turned off: if "server-side tools" is not on in the model, the gateway rejects the hosted tool with `400 unsupported_tool_type` instead of dropping it silently (if it is on, `web_search` is served locally in the gateway; Settings > Gateway). If there is no key, `env_key` is not given; Codex asks for no sign-in on a local provider. |
| OpenCode | 49,152 / 65,536 | `OPENCODE_CONFIG_CONTENT` (the last configuration layer to merge): `provider.harness` (`@ai-sdk/openai-compatible`, `baseURL`, `apiKey: {env:HARNESS_API_KEY}`, the model's `limit.context/output`, `tool_call`) and `model: harness/<alias>`. Program: `opencode` on PATH, otherwise the `opencode-cli` that the desktop version brings (§3a). |
| Aider | 8,192 / 32,768 | `--model openai/<alias> --openai-api-base <base> --model-metadata-file <temporary> --no-show-model-warnings`, `OPENAI_API_KEY`/`OPENAI_API_BASE` in the environment; `AIDER_*` model variables are removed. |
| Goose | 32,768 / 65,536 | Environment: `GOOSE_PROVIDER=openai`, `GOOSE_MODEL`, `OPENAI_HOST` (host), `OPENAI_BASE_PATH=v1/chat/completions`, `OPENAI_API_KEY`, `GOOSE_CONTEXT_LIMIT`, `OPENAI_CUSTOM_HEADERS`. If no arguments are given, `goose session`. |

"Prepare Server" (`harness_core::prepare`) writes the following to the definition's load settings: the **largest context** the model supports and that fits the graphics card, Flash Attention, the K/V cache, and 1 slot; it then stops the definition's running instances, and the gateway loads with the new settings on the first request.

Selection rule: (a) the model limit: the GGUF `context_length`; if `rope.scaling.type` (yarn/linear) + factor + `original_context_length` are declared, `original × factor`, otherwise the training context is not exceeded; (b) memory: full GPU load, Flash Attention, 1 slot, with the `vram` estimate (corrected with measurement) at most 90% of the budget; (c) the tool's limits: the minimum, the recommended (Open Design 131,072; Claude Code/Codex/OpenCode/Goose 65,536; Aider 32,768), and an upper bound (twice the recommended; Open Design 262,144). First the recommended context and above with `q8_0` KV is searched, and if it does not fit, `q4_0`; if the recommended cannot be reached, the same order is tried down to the minimum. The alternatives: `q8_0`/`f16` KV at a smaller context, or partial CPU spill at the recommended context (slow; only with explicit confirmation). If even the minimum context does not fit, nothing is applied silently. With no terminal, `--prepare` or `--no-prepare` is required; `--prepare` confirms only a full-GPU choice.

`--dry-run` prints the plan without starting anything; tokens are hidden as `***`.

The GUI's "Launch" uses the same plan and opens the tool in a new terminal window (the project folder can be chosen):

- **Windows:** a new console window.
- **Linux:** a self-deleting `/bin/sh` launcher script with 0600 permissions; `$TERMINAL` → `x-terminal-emulator` → known terminals (`harness_core::integrations::terminal::pick_terminal`).
- **macOS:** the same launcher script, opened with `osascript` in Terminal.app (`do script "exec sh '<script>'"`) or in iTerm2 (`create window with default profile command`). iTerm2 is chosen only if it is installed (`/Applications` or `~/Applications`) and is the default application for shell scripts (`public.unix-executable` / `.command`) (`defaults read com.apple.LaunchServices/com.apple.launchservices.secure LSHandlers`); otherwise Terminal.app. If an iTerm2 window cannot be opened, Terminal.app is tried. On first use, macOS asks the "Harness Studio wants to control Terminal" (Automation) permission. Only the script's path appears in the AppleScript; the token and environment variables are inside the script (not visible in argv and `ps`).
- If a terminal cannot be opened (no terminal application, `osascript` refused), the corresponding `harness launch` command is copied.

Sources: code.claude.com/docs/en/llm-gateway-connect, /llm-gateway-protocol, /env-vars, /model-config, /settings; developers.openai.com/codex/config-reference, /config-advanced, /auth, /noninteractive; opencode.ai/docs/config, /providers; aider.chat/docs/llms/openai-compat.html, aider.chat/docs/config/adv-model-settings.html; github.com/aaif-goose/goose `documentation/docs` (providers, environment variables).

## 3. Persistent integrations (Harness writes the file)

Each shows a preview (a colored diff) first. Only Harness's keys change; comments and other settings are preserved (JSON/JSONC: the `jsonc-parser` CST, TOML: `toml_edit`). Comments cannot be preserved in YAML; the preview warns about this.

| Tool | File | What is written |
|---|---|---|
| Claude Code | `~/.claude/settings.json` (`CLAUDE_CONFIG_DIR`) | `env.*`: the Claude Code variables in §2. |
| Codex | `~/.codex/config.toml` (`CODEX_HOME`) + `~/.codex/harness.config.toml` | `[model_providers.harness]` (`name`, `base_url`, `wire_api = "responses"`, `http_headers`). The profile is in a separate file (Codex 0.134+ profile files): `model_provider`, `model`, `model_context_window`, `web_search = "disabled"`. Use: `codex --profile harness`. |
| OpenCode | `~/.config/opencode/opencode.json(c)` | `provider.harness`: the selected model first, then all chat models on the gateway (`models.<alias>.limit.context` is the definition's context), so the model can be changed from inside OpenCode. The global `model`: if the request has no `set_default_model`, only if empty or already `harness/*`; if `true`, always; if `false`, never. In the GUI, the "Make this OpenCode's default model" mark controls this field (the default mark is the same rule). |
| Open Design | Open Design's `app-config.json` files | The same as the earlier version. |
| Continue | `~/.continue/config.yaml` | `title: Harness Studio (llama.cpp)` in `models[]`. |
| VS Code Copilot (BYOK) | `%APPDATA%\Code\User\chatLanguageModels.json` (macOS `~/Library/Application Support/Code/User`, Linux `~/.config/Code/User`) | In the root array `name: "Harness Studio"`, `vendor: "customendpoint"`, `apiType: "chat-completions"`, model `url` = `<base>/chat/completions`, `toolCalling`, `maxInputTokens`/`maxOutputTokens`. If there is a key, `apiKey: "${input:harnessStudioApiKey}"` (VS Code asks once and keeps it in its secret store). The path was verified on a real VS Code installation (§8). |
| Zed | `%APPDATA%\Zed\settings.json` (others `~/.config/zed/settings.json`) | `language_models.openai_compatible."Harness Studio"`: `api_url`, `available_models[]` (`max_tokens`, `max_output_tokens`, `capabilities`). Zed asks for the key in its own UI. The environment variable is derived from the provider name: `HARNESS_STUDIO_API_KEY`; Harness does not write the key to the file (Zed's documentation also says not to). |
| Kilo Code | `~/.config/kilo/kilo.json(c)` | `provider.harness` in OpenCode's format (`npm: @ai-sdk/openai-compatible`, `options.baseURL`/`apiKey`); Kilo's documentation gives `C:\Users\<user>\.config\kilo\kilo.jsonc` on Windows too. Not tested on a real installation. |
| Goose | `custom_providers/harness.json` (Windows `%APPDATA%\Block\goose\config`, others `~/.config/goose`) | A file that belongs entirely to Harness: `engine: openai`, `base_url` = the full `<base>/chat/completions`, `models[].context_limit`, and if there is a key `api_key_env: HARNESS_API_KEY`. |
| LibreChat | The `librechat.yaml` path the user gives | `name: "Harness Studio"` in `endpoints.custom[]` (`apiKey`, `baseURL`, `models.default/fetch`, `titleConvo`, `titleModel`). If the file does not exist, it is created with `version: 1.3.17`. In Docker, `host.docker.internal` is needed. |

### 3a. OpenCode desktop

The desktop application reads only the persistent configuration (at startup), and `OPENCODE_CONFIG_CONTENT` cannot be used (if the app is open, a new process only brings the existing window to the front). If the desktop app is installed, the Launch window offers "Configure and Open in Desktop": the preview in §3 → apply → the app opens as a detached process (its environment has no Harness setting). `harness_core::integrations::opencode_desktop`:

| Platform | Location | Status |
|---|---|---|
| Windows | `%LOCALAPPDATA%\Programs\@opencode-aidesktop\OpenCode.exe` (Electron, 1.18.x; no CLI) | Verified |
| Windows | `%LOCALAPPDATA%\OpenCode\OpenCode.exe` + `opencode-cli.exe` (the older Tauri version, 1.2.x; the CLI is a full TUI) | Verified; a real OpenCode round ran with the CLI |
| Windows | Uninstall records (HKCU/HKLM, 32-bit view included), with `DisplayName` starting with "OpenCode": `DisplayIcon` or `InstallLocation` + `MainBinaryName` | Verified |
| Windows | Start menu `OpenCode.lnk` | Not read (it points to the exe the uninstall record points to) |
| macOS | `/Applications/OpenCode.app`, `~/Applications/OpenCode.app` (CLI `Contents/MacOS/opencode-cli`) | Assumption |
| Linux | `/opt/OpenCode/{opencode-desktop,OpenCode}`, `/usr/bin/opencode-desktop`, `~/Applications/OpenCode*.AppImage` | Assumption |

Whether the application is running is read from the process list (the exe path); if it is running, a note says it must be restarted.

### 3b. Model selection, tool calling, and the first response

`harness_core::integrations::agent_models` gives, for every definition (embedding/rerank excluded): context, parameter count, the tool-calling verdict, and prompt-processing speed. The tool-calling verdict, starting from the strongest:

1. **Trial** ("Test Tool Calling"): the Chat Completions request with tools in §7; if it returned `tool_calls`, verified; if not, failed (if the response looks like `{"name":…,"arguments":…}`, `<tool_call>`, or `<tools>`, "wrote plain text"). It is stored in `config/tool_checks.json` with the model file and template override; it goes stale when the definition moves to another file.
2. **Gateway record:** this alias's responses that ended with `finish_reason` `tool_calls` / `tool_use` / `function_call`.
3. **GGUF template:** a template that does not handle tools → llama-server cannot return a tool call; a template that does → only "possible".

The template alone is not enough: the templates of Qwen2.5-Coder-14B and Qwen3-4B carry the same `<tool_call>` markers, and llama-server (b11284) reports `chat_template_caps.supports_tools = true` for both; with the same request Qwen3 returns `tool_calls`, while the coder model writes the call as `<tools>{"name": "get_weather", …}</tools>` text (RX 7900 GRE). There is no fixed model list. The first-response estimate: the tool prompt (OpenCode ~29,000 tokens) / the prompt speed in the record (uncached tokens / `prompt_ms`, if at least 2,000 tokens were measured).

Sources: code.visualstudio.com/docs/agent-customization/language-models; zed.dev/docs/ai/use-api-access; kilo.ai/docs; the goose `custom_providers` documentation; librechat.ai/docs/configuration/librechat_yaml/object_structure/custom_endpoint; docs.continue.dev.

## 4. Step-by-step setup (Harness writes no file)

The screen has steps and copyable fields (Base URL, key, model, and the context if needed).

| Tool | Note |
|---|---|
| Claude Desktop | Developer → Configure Third-Party Inference → Gateway; the URL without `/v1`; the model name is added to the list (claude.com/docs/third-party/claude-desktop/gateway). |
| Cursor | Requests go through Cursor's servers; it cannot reach `127.0.0.1`. It works only with an address reachable from the internet; local use is not recommended. |
| Cline | API Provider "OpenAI Compatible", Context Window by hand (docs.cline.bot). |
| Roo Code | The project was shut down on 2026-05-15 (docs.roocode.com/sunset); the steps are the same as Cline's. |
| JetBrains AI Assistant | Settings → Tools → AI Assistant → Providers & API keys (2026.2). |
| Jan | An OpenAI-compatible provider, Base URL with `/v1`; an empty key is not accepted. |
| Open WebUI | Admin Panel → Settings → Connections or `OPENAI_API_BASE_URL`/`OPENAI_API_KEY`; in Docker, `host.docker.internal`. |
| Obsidian Copilot | "Add Custom Model", provider "OpenAI Format"; for a CORS error, the CORS option on the custom model. |
| Aider | `harness launch aider` or the copied command. |

## 5. SDK examples

OpenAI (Python/JS, `base_url` with `/v1`), Anthropic Python (`base_url` without `/v1`, `auth_token`), LangChain (`ChatOpenAI` with `/v1`, `ChatAnthropic` without `/v1`), LlamaIndex (`OpenAILike`, `is_chat_model`, `is_function_calling_model`, `context_window`), Vercel AI SDK (`@ai-sdk/openai-compatible`, `createOpenAICompatible({ name, baseURL })`), and cURL. All of them use the actual endpoint and model name at that moment.

## 6. Undo and backups

- **Ledger:** `<data>/config/integrations.json`. For every application, per file: the path of every key written, the SHA-256 of the value written (not the value itself), the previous value, which parent path the key created, the marker (a whole table/item belonging to Harness) flag, the file's last SHA-256, and whether Harness created the file.
- **Undo:** only keys that Harness wrote and that still carry the value Harness wrote are removed or return to their previous value; empty parent tables and files that Harness created are deleted. Keys the user changed afterwards stay in place and are reported as a warning. If there is no ledger (an older version, a hand-added entry), only the marker entries (`model_providers.harness`, `provider.harness`, the `"Harness Studio"` items...) are removed. A file that cannot be parsed is not touched.
- **Backup:** before every write and undo, `<file>.harness-YYYYMMDD-HHMMSS.bak`; the last 5 generations per file are kept. The number of backups and their paths are visible on screen.
- **Drift warning:** if the file's SHA-256 differs from the ledger's, an "Edited by hand" badge appears on screen, and a warning appears on apply and undo.

### 6a. Stale model name (alias drift)

A tool stores the model name (the definition alias) that Harness wrote. If the alias is renamed, the definition is deleted, or a profile gets its own definition so that the served name becomes `<file>-<profile>` instead of `<file>`, the tool keeps asking for the old name and gets "model not found". The Connections screen finds this itself (`integrations::drift`, Tauri `check_integration_alias_drift`):

- Read-only: the model names given to Harness are read from the file of every configured integration (OpenCode and Kilo Code: the default `model` + the `provider.harness.models` keys; others: the single model name) and compared case-insensitively with the definition aliases and the names currently served (a running server, the gateway). If there is neither a definition nor a served name, no judgment is made.
- If a name is missing, the card shows a warning and a single "Fix" button. The suggestion: a single valid alias that starts with (or is started by) the missing default name; if there is more than one, no suggestion is made and the served model is used. "Fix" opens the tool's normal preview (the file diff to be written is visible); once confirmed, this write's backup and ledger are kept as usual; for OpenCode-style tools the provider block is rewritten with the gateway's chat models, so old names drop out.
- A name change that occurs when a profile is given its own definition is caught through the same path (suggested by the `<file>` → `<file>-<profile>` prefix match).
- **Hand-written provider blocks:** Apart from Harness's own `harness` block, if a block the user wrote targets the Harness gateway (the base address is loopback and the gateway's current or past port: `config/ports.json` `gateway_history` + the default), the model names in that block go through the same check: OpenCode and Kilo Code `provider.<name>` (+ the default `model` `<name>/<model>`), Codex `[model_providers.<name>]` (+ the top `model` if `model_provider` is that name), and Continue `models` entries whose `apiBase` is the gateway. It is only detected; these blocks are never rewritten automatically. The card shows the same warning with the block's name (even if the tool has no configuration written by Harness); "Fix" opens only the usual preview that writes Harness's own block, and the user updates the name in their own block themselves. Addresses that do not point to the gateway (cloud, another local server, Ollama 11434) do not count. Out of scope: "guide" tools such as Cursor and Cline, and hand-written configurations other than these three formats.

### 6b. Control API spare port and Harness MCP

When the Control API's 11500 is held by another program, it falls back to a spare port (or returns from the spare to the default). Entries that add Harness MCP over HTTP (Claude Code `mcpServers.harness`, Codex `[mcp_servers.harness]`, OpenCode `mcp.harness`, Cursor) carry the address, so they are updated automatically (`harness_mcp::follow_api_port`):

- Each time it opens, the Control API writes the port it listens on to `config/ports.json` (`control_api_history`, at most 8). If the entry's address is loopback and its port is in this instance's own history, only the port changes to the new port; the token (Bearer) and the rest of the file stay as they are, and a `<file>.harness-<date>.bak` backup is taken before writing. The default 11500 is not added to the history automatically: an instance that works with a separate data folder and has never listened on 11500 does not move the real application's entry to its own port.
- Stdio entries, remote/LAN addresses, and ports the Control API has never used are not touched.
- It is gated: only the real application and `harness serve` turn it on (`harness_api::enable_mcp_follow`), and it works only for the application's own data folder; a Control API started by tests never rewrites the settings in the user's real home directory (the same as the gateway's `enable_client_follow` rule).

## 7. Connection test

"Test Connection" sends a single small request with a tool to the API the tool speaks: `POST /v1/messages` (Anthropic) for Claude Code and Claude Desktop, `POST /v1/responses` for Codex, and `POST /v1/chat/completions` for the others. A `get_weather` tool is defined; the result is shown with the status code, latency, whether the model called the tool, and error detail.

### Real client verification (Windows)

One real round in each client through the gateway with Qwen3-4B (`q4-vulkan`):

| Client | Path | Result |
|---|---|---|
| OpenCode 1.2.17 (`opencode-cli`) | `harness launch opencode --prepare -- run "…"` (`OPENCODE_CONFIG_CONTENT`) | "pong"; context prepared from 8192 to 65536 |
| OpenCode | Connections' persistent write, in a temporary home directory (`USERPROFILE`), a copy of an existing `opencode.json` | backup `.harness-<date>.bak`; `opencode-cli run -m harness/q4-vulkan` "pong"; Undo returned the file to its old state byte for byte |
| Aider 0.86.2 (isolated venv) | `harness launch aider --prepare -- --no-git --yes --message "…"` | "pong", 605 tokens. Aider writes no persistent file (`guide_only`) |
| Goose 1.52.0 (official zip) | `harness launch goose --prepare -- run -t "…"` | "pong" |
| Goose | persistent `custom_providers/harness.json` (a temporary `APPDATA`), `GOOSE_PROVIDER=harness` | "pong"; Undo deleted the file and left a backup |
| Claude Code 2.1.283 | the "Add Harness MCP" write to a temporary home directory; the file used with `claude -p --mcp-config … --strict-mcp-config --allowedTools mcp__harness__list_models` | `list_models` was called, answer "11" (the definition count in that instance); haiku, 3 turns |

Findings: (1) If `%LOCALAPPDATA%\OpenCode` is on PATH, `opencode` resolves to the GUI exe (`OpenCode.exe`, case-insensitive); when it is not on PATH, Harness correctly picks `opencode-cli.exe`. (2) On a non-English Windows console, Aider output gives the warning "Terminal does not support pretty output (UnicodeDecodeError)", which does not affect the run. (3) Goose gave the same result both with only `harness launch` environment variables (`GOOSE_PROVIDER=openai`) and with a persistent custom provider (`harness`); the first run creates `%APPDATA%\Block\goose`.

Official SDKs (gateway :21435): `openai` Python 3.22.1 and JS 7.25.0, `ollama` Python 0.6.3 and JS 0.6.4; chat, streaming, tool calling, embeddings, and `show`/`ps`/`list` passed. Real Ollama 0.34.4 `pull` (`hf.co/...:Q8_0`) and vision (`images`) are covered in the Ollama compatibility notes in [ARCHITECTURE.md](ARCHITECTURE.md).

### Creating a model with the Ollama CLI (`ollama create`)

The gateway's Ollama-compatible layer runs the `ollama create` / `cp` / `rm` commands on model **definitions** (details: ARCHITECTURE §13.4):

```
set OLLAMA_HOST=127.0.0.1:21435
ollama create my-model -f Modelfile   # FROM ./model.gguf (the CLI uploads the GGUF with /api/blobs) or FROM <Harness model>
ollama cp my-model copy
ollama rm copy
```

- `FROM <name>`: an existing model on the gateway (the `ollama list` / `/api/tags` name); `FROM ./file.gguf`: the CLI uploads the file under `ollama-blobs/` (SHA-256 is verified). Ollama registry names (`llama3`) are not resolved, and `hf.co/...` works only for downloaded files.
- `PARAMETER`, `SYSTEM`, `MESSAGE`, `ADAPTER` (an uploaded GGUF LoRA), and `TEMPLATE` are mapped with the same rules as the Library's Modelfile import; those that cannot be mapped appear as `warning:` lines in the CLI.
- There is no `--quantize` (501), no `ollama push` (501, there is no registry), and no safetensors (GGUF only).
- Deletion: `ollama rm` deletes only the definition; downloaded/library GGUF files stay. Blob files uploaded with `ollama create` and not used by another definition are deleted.
- These endpoints go through the gateway's authentication (a key is required if there is one); in keyless local use, any program on the same machine can create/delete definitions.

## 8. Not verified

- Kilo Code and JetBrains AI Assistant were not tested on a real installation; they rest on the official documentation only.
- The macOS/Linux desktop paths are assumptions. (On Windows, real clients were tried: OpenCode 1.2.17, Aider 0.86.2, Goose 1.52.0, Claude Code 2.1.283; see above.)
- The GUI parts (sections, "Launch", the test, backup badges) have been exercised in automated checks (`svelte-check`) but their manual behavior in a real window on every platform is not documented here.

### Client paths and key formats: verification sources (checked September 2026)

| Client | What was verified | Source |
|---|---|---|
| VS Code | Custom endpoint `chatLanguageModels.json`; `vendor: customendpoint`; `apiType` `chat-completions`/`responses`/`messages`; the model `url` is the full endpoint; `${input:...}` for `apiKey`; the default header is `Authorization: Bearer`. Path: on Windows, `%APPDATA%\Code\User\chatLanguageModels.json` exists on a real installation (inspected read-only); the documentation does not state the path, and macOS/Linux assume the standard user directory. | https://code.visualstudio.com/docs/copilot/customization/language-models, https://github.com/microsoft/vscode-docs/blob/main/docs/agent-customization/language-models.md |
| Zed | `language_models.openai_compatible.<name>.api_url` + `available_models[]`; the key is not written to settings.json; the environment variable is `<NAME>_API_KEY` (UpperSnake; in the source `format!("{}_API_KEY", id).to_case(Case::UpperSnake)`), i.e. `HARNESS_STUDIO_API_KEY`. | https://zed.dev/docs/ai/use-api-access, https://github.com/zed-industries/zed/blob/main/crates/language_models/src/provider/api_compatible.rs |
| Kilo Code | The global configuration is `~/.config/kilo/kilo.json[c]`, on Windows `C:\Users\<user>\.config\kilo\kilo.jsonc`; the older `opencode.json[c]` is read too; `provider.<name>.options.{baseURL,apiKey}`, the `{env:VARIABLE}` syntax. | https://kilo.ai/docs/getting-started/settings, https://kilo.ai/docs/code-with-ai/platforms/cli, https://github.com/Kilo-Org/kilocode/blob/main/packages/opencode/src/kilocode/skills/kilo-config.md |
| JetBrains AI Assistant | Settings \| Tools \| AI Assistant \| Providers & API keys (2026.2); fields URL, API Key, Tool calling, Test Connection. | https://www.jetbrains.com/help/ai-assistant/use-custom-models.html |
