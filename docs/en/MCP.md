Türkçe: ../tr/MCP.md

# Harness MCP server

Coding agents (Claude Code, Codex, OpenCode, Cursor...) use Harness as a tool: they list local models, pick the one that fits in memory, load it, get the OpenAI-compatible endpoint, read metrics/logs, and run benchmarks. Source: `crates/harness-mcp`.

## Transports

| | stdio | Streamable HTTP |
|---|---|---|
| Launch | The agent runs the `harness mcp` command itself | Control API: `POST http://127.0.0.1:11500/api/harness/v1/mcp` |
| Identity | Local session token (CLI discovery) or `--host` + `HARNESS_TOKEN` | `Authorization: Bearer <named token>` (the session token also works) |
| Scope | `--scope` (default `read,servers:write,tools`) | The token's scopes |
| Recommended | Yes: no secret is written to the configuration | Remote machines, clients that cannot launch stdio |

`harness mcp` works like the other CLI commands ([CLI.md](CLI.md)): it uses the Control API of the running owner (Settings > Developer API on in the GUI, or `harness serve`). The owner is rediscovered on every tool call, so the connection keeps working even if the GUI opens later or the session token is renewed. If no instance is running, the tools return a `cli.no_instance` error; the MCP connection is still established. With `--headless`, a temporary in-process instance is started, and it shuts down together with the servers it started when stdin closes. Only MCP messages are written to stdout; logs go to stderr.

Over HTTP, each tool sends a request to the API router in-process, with the caller's token. Scope and core rules are therefore checked once in MCP and once in the API, against the same table.

## Protocol

Hand-written, dual-era JSON-RPC:

- **2026-07-28** (modern, stateless): `params._meta` on every request (`io.modelcontextprotocol/protocolVersion`, `.../clientCapabilities`), `server/discover`, `resultType: "complete"` in results, and `ttlMs` + `cacheScope` for `tools/list`/`server/discover` (`private`, since `tools/list` varies by token). Over HTTP, `MCP-Protocol-Version`, `Mcp-Method`, and for `tools/call` `Mcp-Name` must match the body (`-32020`, 400; the `=?base64?…?=` form is decoded). An unsupported version returns `-32022` (400, with a `supported` list). A missing `_meta` field returns `-32602` (400). An unknown method returns 404 + `-32601`.
- **2025-11-25, 2025-06-18, 2025-03-26, 2024-11-05** (legacy, with an `initialize` handshake): most clients speak this today. The server keeps no session. It does not generate `Mcp-Session-Id`, there is no GET stream (GET/DELETE → 405), and notifications return 202.
- Over stdio, requests are handled concurrently (a long `load_model` does not block the others). `notifications/cancelled` cancels the relevant request.
- HTTP `Origin` header: if absent (desktop clients) it passes; if present, only origins in the Control API's CORS list; otherwise 403.

**Why not `rmcp`:** the official Rust SDK (`rmcp` 3.5, which supports both eras) comes with macros and `schemars`, and its 2026-07-28 support is very new (3.2 → 3.5 in a month). The **server** side of Harness needs only tools; no resources, prompts, sampling, or subscriptions. On the HTTP side, identity and scope are already in the API's guard. The protocol surface is small: a few hundred lines, no new dependency, pinned by tests. If resources/subscriptions are needed later, moving to `rmcp` would change only `harness-mcp`.

## Harness's MCP client

The rest of this page covers Harness's **server** side. Harness is also a **client** (`harness_core::tools::mcp_client`): it adds the tools of the servers in `mcp_servers.json` to its own tool engine. Same hand-written dual-era approach:

- **Negotiation:** over stdio, a `server/discover` probe; if a modern response does not arrive within 5 s (an error or silence), `initialize` (2025-11-25 is offered; a known version that the server answers is accepted). Over HTTP, the first request is modern (`MCP-Protocol-Version: 2026-07-28`, `Mcp-Method`, `Mcp-Name`, `x-mcp-header` → `Mcp-Param-*`); a 4xx other than a recognized modern error (`-32022`, `-32020`) is treated as a legacy server and continues with `initialize`. A modern server with no common version fails with an error and there is no fallback. The era is fixed for the connection.
- **Notifications:** in the modern era, a long-lived `subscriptions/listen` (`toolsListChanged`) request; in the legacy era, the stdio pipe or the HTTP GET stream. When the list changes, `tools/list` is re-read and the registry is updated.
- **Cancellation and timeout:** `notifications/cancelled` (`requestId`, `reason`) over stdio and legacy HTTP, closing the response stream over modern HTTP; `initialize` is never cancelled.
- **Why not `rmcp` (for the client too):** the needed surface is small (tools, list changes, cancellation, resources, legacy HTTP+SSE, and OAuth; all written with the existing `reqwest`, `sha2`, and `hf_auth`'s PKCE/redirect code, and `rmcp` is not a dependency anyway); Windows process-tree and console-window management has to be done with Harness's own helpers; `rmcp` brings macros and `schemars`, and its 2026-07-28 support is very new (3.2 → 3.5 in a month). The same reasoning as the server side, the same result: no new dependency (apart from the `with-tokio` feature of `command-group`), a few hundred lines pinned by tests.
- **Not supported:** `roots` / `sampling` / `elicitation`, prompts, and pre-registered OAuth client ids for authorization servers that do not offer dynamic registration (see below).

### Legacy HTTP+SSE transport (2024-11-05)

For a server with a URL, the `transport` field (`mcp_servers.json`; "Transport" in the Connections > MCP Servers form) selects the transport:

| Value | Behavior |
|---|---|
| absent / `auto` | Streamable HTTP first. If the server rejects the first request (POST) with a **4xx** (other than 401, 403, 407, 429), a `GET` stream is opened to the same URL, per MCP's backward-compatibility rule. |
| `http` | Streamable HTTP only. |
| `sse` | Legacy HTTP+SSE only; Streamable HTTP is never tried. |

On the legacy transport the client opens `text/event-stream` with `GET <url>`; the first event must be `endpoint` (data: a path or URL, resolved against the SSE address), and every later JSON-RPC message is `POST`ed to that address; responses and notifications arrive on the stream as `message` events. **Security:** if `endpoint` points to a different origin (scheme/host/port), the connection is rejected, so that messages and tokens do not go there. If the stream closes, the connection is considered broken and is re-established on the first call. In the status table and in the `harness tools mcp` output the transport appears as `sse` (for `auto`, the one found is shown). Other clients' `"type": "sse"` entries are now imported as `transport: "sse"` by Import.

### OAuth authorization (MCP authorization, 2025-06-18)

For a remote server that returns 401:

1. `WWW-Authenticate: Bearer resource_metadata="…"` is read; the protected-resource metadata (RFC 9728, or `/.well-known/oauth-protected-resource[/path]` if absent) and the authorization-server metadata (RFC 8414, then OpenID Connect `openid-configuration`; with path variants) are found. If there is no resource metadata at all, the MCP server's own origin is treated as the authorization server (2025-03-26 behavior). If the server declares PKCE methods but does not list `S256`, it is rejected.
2. **Dynamic client registration** (RFC 7591, a public local client, `token_endpoint_auth_method: none`). Without a registration endpoint, authorization cannot be done (a pre-registered client id is not yet supported).
3. **Authorization Code + PKCE (S256)**, the system browser, and a `127.0.0.1:<random port>/callback` redirect (the PKCE, `state`, and listener code are shared with `hf_auth`). In authorization and token requests, the `resource` parameter (RFC 8707) is the canonical address of the MCP endpoint; `scope` is the `scope` in the 401, or the resource metadata's `scopes_supported` list if absent. It waits at most 5 minutes; it can be cancelled.
4. **Storage:** the session (access and refresh token, client id, endpoints, expiry) is a single entry in the operating system keychain: `mcp-oauth:<server name>`. It is not written to JSON files or the log; `Debug` output shows no secret. If the server is renamed, the session moves; if it is deleted, the session is deleted; if the server's URL changes, the old token is not sent.
5. **Refresh:** the access token is refreshed with the `refresh_token` once, 60 s before it expires or when the server returns 401 (one refresh at a time); if it cannot be refreshed, the server becomes "Authorization Required". If the user has their own `Authorization` header, it takes precedence and OAuth is not used.
6. **Sign-out:** the session is deleted locally (the token is not revoked on the server; no revocation endpoint is used).

UI: in Connections > MCP Servers > "Harness's MCP Client", a server that returns 401 shows an "Authorization Required" badge and an **Authorize** button; if authorized, an "Authorized" badge and **Sign Out**. CLI: `harness tools mcp-authorize <server>` (the browser opens on the machine where `harness` runs; the CLI's API client times out in 3 minutes, so if approval takes longer, use the button in the app), `harness tools mcp-logout <server>`. Control API: `POST /tools/mcp/{server}/authorize` and `/logout` (`admin`).

### Resources

From a server that declares the `resources` capability, `resources/list` (all pages), `resources/templates/list`, and `resources/read` are used (`McpConnection::list_resources` / `read_resource`). The model does not see resources by itself; the user attaches them:

- **UI:** in Connections > MCP Servers > "Harness's MCP Client", **Resources** (the list and templates) under a connected server. In the Playground, the **Attach MCP Resource** button in the input area (shown when an MCP server is configured) lists the resources; adding one reads the resource, saves it under `<data folder>/mcp-resources/<server>/` with an extension the document reader recognizes (from the MIME type: `.md`, `.txt`, `.json`, `.pdf`…), and attaches it to the chat like other documents (local RAG). For template resources, the URI is entered by hand. Binary resources (images etc.) cannot be attached as documents; PDF can.
- **CLI:** `harness tools mcp-resources <server>` lists, and `--read <uri>` prints the content (text as is, a summary for binary).
- **Control API:** `GET /tools/mcp/{server}/resources`, `GET /tools/mcp/{server}/resource?uri=…` (`tools` scope).
- **Permission class:** `annotations.readOnlyHint: true` → restricted, otherwise → dangerous; the user can override (see [SECURITY_MODEL.md](SECURITY_MODEL.md)).
- **Import sources:** Claude Desktop, Claude Code (`~/.claude.json` and project `.mcp.json`), Codex (`~/.codex/config.toml`), Cursor, LM Studio.

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

Test: `tests/mcp.rs` `e2e_mcp_get_endpoint_managed_via_gateway` (`#[ignore]`, ports 19200+): `loaded: false` → a real chat to the returned address (JIT) → `loaded: true`.

## "Add Harness MCP"

Connections > Tools and Clients. Core: `harness_core::integrations::harness_mcp`.

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
- HTTP: the Control API address + a named token (Settings > Developer API). The token is written to the tool's file in plain text; this is why stdio is recommended.
- A one-line command to add by hand is also shown for Claude Code and Codex (`claude mcp add -s user harness -- <harness> mcp`, `codex mcp add harness -- <harness> mcp`).

## Tests

- Unit: `cargo test -p harness-mcp` (tool catalog, argument validation, HTTP header rules, base64), `cargo test -p harness-core harness_mcp` (the configuration formats of the four tools, preservation of comments/other entries, a corrupt file, removal).
- Harness tools: `crates/harness-api/tests/tools_mcp.rs` (real HTTP: the `tools` scope, `approved` / folder rejection, off → management tools only, on → tools and hints, the error of a call that asks for approval, policy off, `mcp:<client>` logging), `crates/harness-cli/tests/mcp.rs` `stdio_offers_harness_tools_only_when_opted_in` (the real `harness mcp` binary, the `initialize` client name, the `tool_calls` record).
- Real HTTP (`crates/harness-api/tests/mcp.rs`): 401, the legacy handshake, a 202 notification, no session header, `tools/list` filtered by scope, scope rejection, modern header rules (`-32020`, `-32022`, `-32602`, 404), GET 405, foreign Origin 403.
- Real stdio (`crates/harness-cli/tests/mcp.rs`): the real `harness mcp` binary, a line-by-line JSON-RPC client, a temporary data directory.
- End to end (`#[ignore]`, `HS_LLAMA_SERVER` + `HS_TEST_MODEL`, ports 18900+): over stdio through `harness serve`: `load_model` → `get_endpoint` → a real `/v1/chat/completions` to the returned endpoint → metrics, log, info, recommendation → `unload_model` → benchmark → comparison. `--headless` shuts llama-server down when stdin closes. The same flow over HTTP.
- Real clients (`#[ignore]`): `e2e_claude_code_connects_to_harness_mcp` sees the entry the integration wrote to a temporary `CLAUDE_CONFIG_DIR` as "Connected" with `claude mcp list`, for both stdio and HTTP. `e2e_codex_reads_harness_mcp_entry` reads the entry in a temporary `CODEX_HOME` with `codex mcp get --json`.

### OpenCode real-client verification

OpenCode **1.18.34** was installed into a separate directory only, with `npm install --prefix <dir> opencode-ai`. Per the [OpenCode configuration documentation](https://opencode.ai/docs/config/), `OPENCODE_CONFIG` does not replace the global file but merges with it; it does not provide isolation on its own. The XDG config/data/cache/state roots and `OPENCODE_TEST_HOME` were redirected to a separate profile, and project configuration and automatic updates were turned off. `debug paths` and `debug config --print-logs` confirmed that the separate paths were used instead of the real user file.

`harness_mcp::apply_in` wrote the `mcp.harness` entry to the separate profile: `type: local`, `command: [harness.exe, mcp, --headless, --scope, read]`, `environment.HARNESS_DATA_DIR` the separate data folder, in-memory secrets. `opencode mcp list` → **✓ harness connected / 1 server(s)**. OpenCode's [1.18.34 connection code](https://github.com/anomalyco/opencode/blob/v1.18.34/packages/opencode/src/mcp/index.ts) also fetches the MCP tool catalog before reaching this state; if the catalog cannot be fetched, the connection counts as failed.

An `opencode run --format json` session was sent to the installed `http://127.0.0.1:11435/v1` gateway with the already available `qwen2.5-coder-14b-instruct-q4_k_m` model. Only the `harness_list_models` tool was enabled and allowed; the MCP scope was `read`. The model wrote the call as JSON text in a code block; no `tool_use` event occurred and the tool was not executed. The connection/catalog was verified; a tool call through the model was not. The installed application's data was not used; the temporary headless MCP instance started no model server and shut down when stdin closed. Cursor was not available on the test machine and was not tried.

To re-run (PowerShell; if `HS_OPENCODE_MODEL_TURN` is not set, only the connection test runs):

```powershell
$env:HS_OPENCODE_BIN = (Resolve-Path <opencode-dir>/node_modules/opencode-ai/bin/opencode.exe).Path
$env:HS_OPENCODE_MODEL_TURN = '1'
$env:HS_OPENCODE_REPORT_DIR = (Resolve-Path <report-dir>).Path
cargo test -p harness-cli --test mcp e2e_opencode_connects_to_harness_mcp -- --ignored --nocapture
```

The test uses a temporary profile with `McpRoots`; it checks the XDG paths in the `debug paths` output before running. It does not write to the user's real OpenCode/Claude/Codex files.
