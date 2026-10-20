Türkçe: ../tr/MCP.md

# Harness MCP server

Coding agents (Claude Code, Codex, OpenCode, Cursor...) use Harness as a tool: they list local models, pick the one that fits in memory, load it, get the OpenAI-compatible endpoint, read metrics/logs, and run benchmarks.

## Transports

| | stdio | Streamable HTTP |
|---|---|---|
| Launch | The agent runs the `harness mcp` command itself | Control API: `POST http://127.0.0.1:11500/api/harness/v1/mcp` |
| Identity | Local session token (CLI discovery) or `--host` + `HARNESS_TOKEN` | `Authorization: Bearer <named token>` (the session token also works) |
| Scope | `--scope` (default `read,servers:write,tools`) | The token's scopes |
| Recommended | Yes: no secret is written to the configuration | Remote machines, clients that cannot launch stdio |

`harness mcp` works like the other CLI commands ([CLI.md](CLI.md)): it uses the Control API of the running owner (Settings > Developer > Control API on in the GUI, or `harness serve`). The owner is rediscovered on every tool call, so the connection keeps working even if the GUI opens later or the session token is renewed. If no instance is running, the tools return a `cli.no_instance` error; the MCP connection is still established. With `--headless`, a temporary in-process instance is started, and it shuts down together with the servers it started when stdin closes. Only MCP messages are written to stdout; logs go to stderr.


## Connect External MCP Servers

Harness can also use tools from external MCP servers. In **Connections > MCP Servers**, add a stdio command or HTTP endpoint, review its configuration, and enable the connection. Native stdio programs run with your user account's authority; HTTP servers receive the arguments you send. See [Security and Privacy](SECURITY_MODEL.md).

For an HTTP server that requires OAuth, use **Authorize** in the app and complete the browser sign-in. CLI users can use `harness tools mcp-authorize <server>` and `harness tools mcp-logout <server>`.

For a connected server that offers resources, open **Resources** to browse them. In Playground, **Attach MCP Resource** reads a selected resource and attaches it as a document; enter the URI for a template resource. The CLI equivalents are `harness tools mcp-resources <server>` and `harness tools mcp-resources <server> --read <uri>`. Binary images cannot be attached as documents; PDFs can.

## Harness built-in tools (opt-in)

Besides the model-management tools, Harness's own tool engine can also be offered to MCP clients: `read_file`, `file_glob_search`, `grep_search`, `get_info`, `write_file`, `edit_file`, `exec_shell_command`, `current_datetime`, `calculator`, `fetch_url`, `web_search`, `geocode`, `weather` (name, schema, and result text are the same as the Control API's `GET /tools`). Source: `crates/harness-mcp/src/harness_tools.rs`.

- **Off by default.** Settings > Tools > "Offer to MCP Clients" (`settings.tools.mcp_expose`). When off, `tools/list` shows only the management tools below; old clients do not suddenly see file and shell tools. The setting can also be changed from the Control API with `PUT /tools/policy` (admin).
- **Scope `tools`.** It does not include `read`; over HTTP the token must carry `read` + `tools`, and over stdio the default `--scope read,servers:write,tools` already includes it (if narrowed, such as `--scope read`, the tools are hidden).
- **Which tools.** Only tools whose source is built in. The tools of servers that Harness consumes with its own MCP client (`<server>_<tool>`) are not re-offered: if Harness consumed its own `harness mcp`, it would be an infinite reflection, a client can connect to that server directly, and a two-layer name/permission confusion would result. If a Harness tool has the same name as a management tool, the management tool stays.
- **The same policy, no approval channel.** The call is decided by the saved policy through `POST /tools/{name}/run`. MCP cannot ask the user, so only the calls that the policy lets run without asking will run: safe tools always (unless the policy is off), and the others when "allow trusted tools" is set and they are on the trusted list, or when "allow all" is set. A tool that will not run without asking still appears in the list but says so in its description; if called, it returns `isError: true` and a text saying "Make it trusted in Settings > Tools" (`tools.approval_required`). If the policy is "off", no Harness tool is offered (`tools.disabled`). The client cannot send `approved` and cannot choose the working folder; file and shell tools run in the default folder in Settings > Tools (otherwise a `no_workspace` tool error).
- **Hints from the class.** Safe and restricted tools get `readOnlyHint: true`, dangerous tools (`write_file`, `edit_file`, `exec_shell_command`) get `readOnlyHint: false` + `destructiveHint: true`; restricted tools that do not use a working folder (network: `fetch_url`, `web_search`, `geocode`, `weather`) get `openWorldHint: true`.
- **Log.** Every call (denied ones included) is written to `tool_calls`; `client` = `mcp:<client name>` (stdio: `clientInfo.name` from `initialize`; over HTTP, since there is no session, the token name, or `io.modelcontextprotocol/clientInfo` in `_meta` on a modern request). It is visible in Analytics > Request Inspector > Tool Calls with tool and client filters.
- Error contract and result: a tool that runs returns the `content` text, `isError: true` if the tool fails (an `error` code); the result is truncated at 16,000 characters.

## Tools and scopes

`tools/list` shows only the tools that the caller's scope allows. If a hidden tool is called, a tool error (`api.insufficient_scope`) naming the required scope is returned. Every tool has MCP hints (`readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint`) and a `title`. The model argument can be a definition alias, a library model id/file name/name, or an absolute `.gguf` path.

| Tool | Scope | Hint | What it does |
|---|---|---|---|
| `list_models` | read | read-only | Library (type, architecture, quant, size, parameters, context, capabilities), definitions (aliases), the servers it is loaded on, ongoing downloads. `kind` filter |
| `get_model_info` | read | read-only | GGUF metadata, definitions, memory estimate (VRAM/RAM for `context_size`, class, recommended `-ngl`; corrected with measured data) |
| `hardware_status` | read | read-only | CPU, RAM, GPUs (`--list-devices` total/free VRAM), recommended backend, runtime, number of running servers |
| `recommend_models` | read | read-only | Ranks local models by how well they fit this machine's memory (full GPU > tight > hybrid > CPU-heavy > OOM risk; within a class, larger models first). `purpose`: chat, tools, reasoning, vision, embedding, rerank |
| `load_model` | servers:control | write | Creates a definition from the file name if there is none, reuses a stopped instance that last served the same model, and waits until ready. On a failed start: a classified error (`servers.out_of_memory`...), the end of the log, and OOM recovery options |
| `unload_model` | servers:control | write, idempotent | Stops the servers serving the model (or a single server) |
| `list_servers` | read | read-only | Servers: state, served model id, endpoint, profile, last error |
| `start_server` | servers:control | write | Starts and waits for a saved profile (or a known instance). Network rules apply |
| `stop_server` | servers:control | write, idempotent | Stops a server |
| `get_endpoint` | read | read-only | The OpenAI-compatible `base_url` (`.../v1`), the `model` to use in requests, and whether an API key is required. The key itself is never returned |
| `get_metrics` | read | read-only | Live telemetry (t/s, slots, context, KV, RAM/VRAM); 1 Hz history with `history` |
| `get_logs` | read | read-only | The latest log lines; continue with `since` |
| `run_benchmark` | servers:write | write | A llama-bench sweep (`n_prompt`, `n_gen`, `gpu_layers`, `flash_attn`...) and the result when done |
| `compare_performance` | read | read-only | The best pp/tg t/s per model in saved benchmarks + the live average of running servers |
| `list_conversations` | conversations | read-only | Playground conversations (`query`, `limit`); searches titles and content |
| `get_conversation` | conversations | read-only | The conversation's current path; `last` returns only the last N messages (`messages_omitted`) |
| `export_conversation` | conversations | read-only | `format`: `markdown` (default) or an importable `json` document |
| `compact_conversation` | conversations | write | Replaces old turns with a summary (`keep_turns`, `instruction`, `model`...); the gateway must be running. Original messages are not deleted |
| `pull_model` | downloads:write | write, open-world | Download from Hugging Face (queued); `wait` waits for completion |
| `delete_model` | models:write | **destructive** | Deletes model files (rejected if in use, linked, or outside the library) |

The `conversations` scope exposes whole conversations (private content); it is therefore not part of `read` and is not in the default `--scope` (`--scope read,conversations`). `servers:write` covers `servers:control`, and `models:write` covers `downloads:write`; the older `--scope read,servers:write` and `read,servers:write,models:write` settings therefore open the same tools.

Opening to the LAN cannot be done from MCP: no tool changes the bind address or gives the "Open Anyway" confirmation (`/exposure-ack` requires admin and does not exist in MCP). Starting a profile that binds to the network with `start_server` runs into the same network rule (`network.lan_without_key`).

### `get_endpoint` and the managed endpoint

If the gateway is running and the requested model resolves to a Model Definition, `get_endpoint` returns the managed endpoint:

```json
{
  "kind": "managed",
  "base_url": "http://127.0.0.1:11435/v1",
  "chat_completions_url": ".../v1/chat/completions",
  "responses_url": ".../v1/responses",
  "messages_url": ".../v1/messages",
  "model": "<alias>",
  "loaded": false,
  "api_key_required": false,
  "direct": null,
  "note": "..."
}
```

- It returns even if the model is not loaded (`loaded: false`); the gateway loads it on the first request (JIT) and unloads it after the idle time. If loaded, the `direct` field carries llama-server's own endpoint.
- If the gateway is bound to loopback and the request goes to a remote Harness, the managed endpoint is not given (the agent cannot reach it); the direct endpoint is returned.
- If `api_key_required: true`, a named token with the `inference` scope is needed; the token itself is never returned.
- If the gateway is off, as before: `kind: "direct"` (llama-server, which goes away when the server is unloaded), and `mcp.not_loaded` if the model is not loaded.


## "Add Harness MCP"

Connections > Tools and Clients.

| Tool | File | Entry | Source |
|---|---|---|---|
| Claude Code | `~/.claude.json` (`$CLAUDE_CONFIG_DIR/.claude.json` if `CLAUDE_CONFIG_DIR` is set), user scope | `mcpServers.harness`: `{"type": "stdio", "command", "args", "env"}` or `{"type": "http", "url", "headers"}` | code.claude.com/docs/en/mcp |
| Codex | `~/.codex/config.toml` (`CODEX_HOME`) | `[mcp_servers.harness]`: `command`, `args`, `env` or `url`, `http_headers` | learn.chatgpt.com/docs/extend/mcp |
| OpenCode | `~/.config/opencode/opencode.json(c)` | `mcp.harness`: `{"type": "local", "command": [...], "environment", "enabled"}` or `{"type": "remote", "url", "headers", "oauth": false}` | opencode.ai/docs/mcp-servers |
| Cursor | `~/.cursor/mcp.json` | `mcpServers.harness`: `{"type": "stdio", ...}` or `{"url", "headers"}` | cursor.com/docs/context/mcp |

- Preview: the start/end of the file to be written. For large files (`~/.claude.json` can be thousands of lines), only the entry to be added is shown.
- The same write path as provider integrations (`keyed`): only the `harness` entry is edited in place, JSONC/TOML comments, indentation, and key order are kept; before every write a `<file>.harness-<date>.bak` backup (the last 5 per file) and an atomic write. A file that cannot be parsed is not touched.
- The ledger (`config/integrations.json`, `harness-mcp:<tool>`): "Remove" deletes the entry, also removes the parent objects that Harness created, and deletes the file again if Harness created it; if the file has not changed since Harness wrote it, it returns to its old state byte for byte. Old entries with no ledger are found and removed by the name `harness`. Other MCP servers and settings are preserved. After removal, the named tokens in Harness's backups are replaced with `removed-by-harness`.
- stdio command: the `harness` that ships with the app (sidecar; see [CLI.md](CLI.md)), then the `harness` on PATH (`cli_install::locate`); if neither is found (for example, a development build without `cargo build -p harness-cli`), the user enters the path. "Allow downloads and model deletion" adds `--scope read,servers:write,tools,models:write`. If the GUI's data directory is not the CLI default, the `HARNESS_DATA_DIR` environment variable is written.
- HTTP: the Control API address + a named token (Settings > Developer > Control API). The token is written to the tool's file in plain text; this is why stdio is recommended.
- A one-line command to add by hand is also shown for Claude Code and Codex (`claude mcp add -s user harness -- <harness> mcp`, `codex mcp add harness -- <harness> mcp`).
