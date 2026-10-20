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

The desktop application reads only the persistent configuration (at startup), and `OPENCODE_CONFIG_CONTENT` cannot be used (if the app is open, a new process only brings the existing window to the front). If the desktop app is installed, the Launch window offers "Configure and Open in Desktop": the preview in §3 → apply → the app opens as a detached process (its environment has no Harness setting).:

| Platform | Location | Status |
|---|---|---|
| Windows | `%LOCALAPPDATA%\Programs\@opencode-aidesktop\OpenCode.exe` (Electron, 1.18.x; no CLI) | Verified |
| Windows | `%LOCALAPPDATA%\OpenCode\OpenCode.exe` + `opencode-cli.exe` (the older Tauri version, 1.2.x; the CLI is a full TUI) | Verified; a real OpenCode round ran with the CLI |
| Windows | Uninstall records (HKCU/HKLM, 32-bit view included), with `DisplayName` starting with "OpenCode": `DisplayIcon` or `InstallLocation` + `MainBinaryName` | Verified |
| Windows | Start menu `OpenCode.lnk` | Not read (it points to the exe the uninstall record points to) |
| macOS | `/Applications/OpenCode.app`, `~/Applications/OpenCode.app` (CLI `Contents/MacOS/opencode-cli`) | Assumption |
| Linux | `/opt/OpenCode/{opencode-desktop,OpenCode}`, `/usr/bin/opencode-desktop`, `~/Applications/OpenCode*.AppImage` | Assumption |

Whether the application is running is read from the process list (the exe path); if it is running, a note says it must be restarted.

### 3b. Model Selection and Tool Calling

Choose a chat model with sufficient context for the client. Use **Test Tool Calling** to check whether the selected model returns a usable tool call. A compatible template alone does not guarantee success. Large client prompts can delay the first response.

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

## 6. Undo and Backups

Harness previews configuration changes and keeps timestamped backups next to the original file. **Undo** restores settings Harness changed when they have not been edited since. Other settings are preserved; malformed files are not overwritten.

If an alias changes, Connections can warn about a stale model name. **Fix** opens a configuration preview; inspect it before applying. Update hand-written provider entries yourself.

Local HTTP MCP entries can follow a changed Control API port automatically. Remote entries and stdio connections are unaffected.

## 7. Connection test

"Test Connection" sends a single small request with a tool to the API the tool speaks: `POST /v1/messages` (Anthropic) for Claude Code and Claude Desktop, `POST /v1/responses` for Codex, and `POST /v1/chat/completions` for the others. A `get_weather` tool is defined; the result is shown with the status code, latency, whether the model called the tool, and error detail.

### Creating a model with the Ollama CLI (`ollama create`)

The gateway's Ollama-compatible layer runs the `ollama create` / `cp` / `rm` commands on model **definitions** (details: private hs-internal architecture notes):

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
