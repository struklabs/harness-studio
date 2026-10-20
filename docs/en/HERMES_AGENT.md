# Connect an Agent and Complete a Task

[Türkçe](../tr/HERMES_AGENT.md)

The first 0.2 W2 slice uses **Hermes Agent by Nous Research**. Open Modules → Agents. Inspect the installation permissions, install if needed, start Harness Gateway in Settings, select a chat model, and choose Connect. Enter a simple question and choose Run Task. Stop terminates the Harness-owned task process tree. Disconnect restores the original configuration and revokes the connection's token. Removal disconnects and removes the managed runtime while retaining external installations and user data by default. See [Module Lifecycle](MODULE_LIFECYCLE.md) for update, repair, rollback, and explicit deletion choices.

Simple mode shows the connection and task controls. Advanced also shows executable/configuration/backup paths and installer output. Failed installation output is visible in either mode. Model recommendations reuse native tool-call evidence and hardware fit estimates; unverified template support is labeled as such. Linked chat aliases can also be selected, but local hardware recommendations apply only to local files.

## Upstream Comparison

Research checked on 2026-10-20. These are upstream capabilities, not promises that every distribution is supported by this adapter.

| Area | OpenClaw | Hermes Agent |
| --- | --- | --- |
| macOS/Linux | Official shell installer; npm package also available. Requires supported Node. | Official shell source installer; macOS also offers a desktop package. |
| Windows | Native Hub, PowerShell CLI installer, or WSL2. WSL remains the most Linux-compatible Gateway option. | Native PowerShell source installer or desktop package; WSL2 can use the shell installer. |
| Configuration | JSON5 at `~/.openclaw/openclaw.json`; `OPENCLAW_CONFIG_PATH` overrides it. | `config.yaml` and `.env` under `HERMES_HOME`; default `~/.hermes` on POSIX, `%LOCALAPPDATA%\hermes` on current Windows installations. |
| OpenAI-compatible model | `models.providers.<id>` with `baseUrl`, `apiKey`, protocol/model entries; select the provider/model as the default. | `model.default`, `model.provider: custom`, `model.base_url`, `model.api_key`. |
| Task/output/status | `openclaw agent --message … --json`; `--local` embeds a single run. Gateway runs expose sessions/run IDs; inspect `openclaw gateway status`. | `hermes chat -q … -Q`; current CLI supports stream JSON. The documented `AIAgent.run_conversation` API returns completion/output/error information. CLI sessions can be resumed and inspected with `/status`. |
| License | MIT | MIT |

Sources: [OpenClaw installation](https://docs.openclaw.ai/install), [Windows options](https://docs.openclaw.ai/platforms/windows), [configuration](https://docs.openclaw.ai/gateway/configuration), [custom providers](https://docs.openclaw.ai/concepts/model-providers/custom-providers), [agent CLI](https://docs.openclaw.ai/cli/agent), [MIT license](https://github.com/openclaw/openclaw/blob/main/LICENSE); [Hermes installation](https://hermes-agent.nousresearch.com/docs/getting-started/installation), [configuration](https://hermes-agent.nousresearch.com/docs/user-guide/configuration), [CLI](https://hermes-agent.nousresearch.com/docs/user-guide/cli), [Python API](https://hermes-agent.nousresearch.com/docs/guides/python-library), [MIT license](https://github.com/NousResearch/hermes-agent/blob/main/LICENSE).

**Decision:** Hermes has a small, documented in-process API with an explicit empty tool allowlist and native Windows source installation. It fits this task without provisioning an agent gateway/service or reconciling OpenClaw's provider/model catalog and state ownership. This is an integration judgment, not a claim that OpenClaw requires WSL. Modern Hermes CLI chat can delegate to a detached gateway; the adapter deliberately uses its Python API so Stop owns the complete execution lifetime.

## Installation and Compatibility

Harness downloads and executes the official `https://hermes-agent.nousresearch.com/install.ps1` on Windows or `https://hermes-agent.nousresearch.com/install.sh` on POSIX. It passes the upstream noninteractive, skip-browser, and skip-computer-use options and runs only the selected backend stages in the managed runtime directory. The source commit is pinned; the bootstrap script is not hash-pinned. It does not fork Hermes, run onboarding, configure messaging, or install a service. Upstream downloads and prepares its own dependencies; installation approval permits this installer to run with the user's privileges. It is not a hash-pinned Harness runtime, and the download/disk sizes are estimates. Source-build dependencies may be required. Installation output is bounded and cancellation kills the installer process tree.

Detection checks PATH and standard source-install launcher locations, probes `--version` with a deadline, and reports missing bootstrap commands or a broken/missing Python API runtime. Version probes are cached briefly. Source installations using the current PM launcher and legacy virtual environments are supported. Desktop-only bundles without a discoverable source/API runtime are reported as missing that dependency; use the official source installation. Harness-managed runtime updates use the bundled source pin and rollback protocol described in [Module Lifecycle](MODULE_LIFECYCLE.md). External source installations keep their own upstream update workflow.

## Connection, Permissions, and Recovery

Connect previews the running local Gateway address and chosen model. It creates a named token using the existing token store: only `inference`, exactly the selected model alias, and only chat/model-list endpoint groups. No control, tool, shell, or admin scope is granted. Keys never enter IPC or process arguments. The agent needs its token in its own private configuration; Harness stores only its salted token digest and connection metadata. On a Gateway that accepts tokenless loopback calls, other local clients can still bypass token policy; enable the local token requirement to make restrictions binding for all clients.

The complete original configuration is saved privately as `config.yaml.harness.bak`. A private `harness-connection.json` journal is written before the atomic configuration replacement; corrupt YAML and existing backups are refused. Unrelated settings are retained. Disconnect revokes first and restores the original bytes, or removes a config that did not previously exist. If another interface edited the config, restoration refuses to overwrite it and retains the backup/journal for recovery. Inspect the backup, restore its bytes to `config.yaml`, then retry Disconnect. An orphan backup from interrupted cleanup must be inspected before reconnecting.

Harness tasks use a temporary Hermes home, explicit local model credentials, a cleared credential environment, and no user context/memory/skills/hooks/MCP configuration. Each run requires a fresh grant in the UI. By default `enabled_toolsets=[]`: inference only. Optionally grant up to eight absolute folder paths to the read-only `harness_read_file` tool. It resolves symlinks/junctions before checking folder containment, accepts only regular files, and limits each read to 32 KiB. Execution fails closed unless upstream exposes exactly the granted tools. Shell, writes, and additional network access cannot be granted. Sensitive actions and interactive approvals are unavailable in this adapter, rather than silently approved. This is an application-level restriction, not OS sandboxing of trusted upstream Python code. Other Hermes interfaces retain their own permission rules. Output is bounded to 64 KiB and the task token is redacted. Tasks have a ten-minute deadline, at most four iterations and 512 generated tokens per iteration.

## Task History and Audit

Harness stores its own private `agents/history.json` under the app data root. It lists running and past Harness tasks with the model, prompt, start/finish times, duration, token count when reported by upstream, errors, and reopenable output. Runs also record the granted folders, tool calls/denials, and start/stop events. A run left running after an app interruption is marked Interrupted on the next history read. Delete History Entry removes only that Harness entry and its audit/output; it never edits Hermes sessions, databases, configuration, or user files. Running entries must be stopped before deletion. External Hermes sessions are not imported. Treat saved prompts/output as private user data.

## Existing Paperclip Instances

Generate Configuration creates a separate, named Paperclip inference token restricted to the selected local or Harness Link chat alias and chat/model-list endpoints. The Gateway must require tokens. Inspect/copy the endpoint and configuration; the plaintext credential is shown only in this handoff and is not saved in Harness history. Revoke it here or in Settings. The endpoint must be reachable from Paperclip's agent execution host; loopback requires the same host.

Automatic Apply targets an **existing `opencode_local` agent** in an existing Paperclip instance. It uses the documented [GET/PATCH agent API](https://github.com/paperclipai/paperclip/blob/master/docs/api/agents.md) and [per-agent environment configuration](https://github.com/paperclipai/paperclip/blob/master/packages/adapters/opencode-local/src/server/execute.ts), with OpenCode's documented [inline config](https://opencode.ai/docs/config/#precedence-order) and [OpenAI-compatible provider](https://opencode.ai/docs/providers/#custom-provider). Apply preserves unrelated adapter settings, environment entries, inline permissions and other providers; it updates the selected model and the `harness` inline provider. Malformed or secret-reference inline configuration is refused instead of overwritten. HTTPS or loopback HTTP is required; redirects are disabled. API credentials are used transiently and response bodies are not exposed in errors. Paperclip retains its own execution/tool permissions. No installation, startup, updates, or other lifecycle management is performed. Other adapter types can be configured manually using their supported OpenAI-compatible settings; Apply does not guess their schema.

## Verification

Rust tests cover backup/restore, external edits, token scopes/revocation, grant validation, history recovery/deletion, process errors, descendant cancellation, Paperclip configuration preservation, and a local GET/PATCH API fixture with redirect refusal. `python scripts/test-hermes-adapter.py` exercises the shipped adapter's tool allowlist, file bounds, traversal/symlink denials, extended Windows paths, and audit events with a deterministic upstream API double. Windows UNC/device paths and reparse points are rejected before resolution to avoid ungranted network I/O. The opt-in integration test is:

```powershell
$env:HS_LLAMA_SERVER = 'C:/path/to/llama-server.exe'
$env:HS_TEST_MODEL = 'C:/path/to/chat.gguf'
cargo test -p harness-gateway --test hermes_e2e -- --ignored --nocapture
```

The Windows run used existing upstream Hermes v0.14.0, real Harness Gateway authentication, llama-server, and Qwen3-4B-Instruct-2507 Q4_K_M. It verified inference, a granted file read with an actual tool-call audit event, token usage, history reopening/deletion, Stop, revoked-token rejection, byte-exact restoration, and revocation in temporary directories. The existing installation and user configuration were not modified. Paperclip's documented API is covered by a local fixture, not a live Paperclip instance. macOS/Linux and newer Hermes releases still require platform end-to-end verification; this test does not claim a fresh install was performed.
