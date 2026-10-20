# Connect an Agent and Complete a Task

[Türkçe](../tr/HERMES_AGENT.md)

The first 0.2 W2 slice uses **Hermes Agent by Nous Research**. Open Modules → Agents. Inspect the installation permissions, install if needed, start Harness Gateway in Settings, select a chat model, and choose Connect. Enter a simple question and choose Run Task. Stop terminates the Harness-owned task process tree. Disconnect restores the original configuration and revokes the connection's token. Removal disconnects and releases the module record; it retains the upstream installation, conversations, and user files.

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

Harness downloads and executes the official `https://hermes-agent.nousresearch.com/install.ps1` on Windows or `https://hermes-agent.nousresearch.com/install.sh` on POSIX. It passes the upstream noninteractive, skip-browser, and skip-computer-use options. It does not fork Hermes, run onboarding, configure messaging, or install a service. Upstream downloads and prepares its own dependencies; installation approval permits this installer to run with the user's privileges. It is not a hash-pinned Harness runtime, and the download/disk sizes are estimates. Source-build dependencies may be required. Installation output is bounded and cancellation kills the installer process tree.

Detection checks PATH and standard source-install launcher locations, probes `--version` with a deadline, and reports missing bootstrap commands or a broken/missing Python API runtime. Version probes are cached briefly. Source installations using the current PM launcher and legacy virtual environments are supported. Desktop-only bundles without a discoverable source/API runtime are reported as missing that dependency; use the official source installation. Updates remain owned by upstream (`hermes update` for source installs), not by Harness.

## Connection, Permissions, and Recovery

Connect previews the running local Gateway address and chosen model. It creates a named token using the existing token store: only `inference`, exactly the selected model alias, and only chat/model-list endpoint groups. No control, tool, shell, or admin scope is granted. Keys never enter IPC or process arguments. The agent needs its token in its own private configuration; Harness stores only its salted token digest and connection metadata. On a Gateway that accepts tokenless loopback calls, other local clients can still bypass token policy; enable the local token requirement to make restrictions binding for all clients.

The complete original configuration is saved privately as `config.yaml.harness.bak`. A private `harness-connection.json` journal is written before the atomic configuration replacement; corrupt YAML and existing backups are refused. Unrelated settings are retained. Disconnect revokes first and restores the original bytes, or removes a config that did not previously exist. If another interface edited the config, restoration refuses to overwrite it and retains the backup/journal for recovery. Inspect the backup, restore its bytes to `config.yaml`, then retry Disconnect. An orphan backup from interrupted cleanup must be inspected before reconnecting.

Harness tasks use a temporary Hermes home, explicit local model credentials, a cleared credential environment, no user context/memory/skills/hooks/MCP configuration, and `enabled_toolsets=[]`. Execution fails closed if upstream exposes tools anyway. This is an application-level tool restriction, not OS sandboxing of trusted upstream Python code. Other Hermes interfaces keep their own permission rules and remain available. Output is bounded to 64 KiB and the task token is redacted. Tasks have a ten-minute deadline, at most two iterations and 512 generated tokens. Temporary task state and output are not persisted by Harness.

## Verification

Rust tests cover exact backup/restore, absent and corrupt configurations, external edits, token scopes/revocation, detection, process errors, and cancellation. The opt-in integration test is:

```powershell
$env:HS_LLAMA_SERVER = 'C:/path/to/llama-server.exe'
$env:HS_TEST_MODEL = 'C:/path/to/chat.gguf'
cargo test -p harness-gateway --test hermes_e2e -- --ignored --nocapture
```

The Windows run used existing upstream Hermes v0.14.0, real Harness Gateway authentication, llama-server, and Qwen3-4B-Instruct-2507 Q4_K_M. It verified output, Stop, byte-exact restoration, and revocation in temporary directories. The existing installation and user configuration were not modified. macOS/Linux and newer Hermes releases still require platform end-to-end verification; this test does not claim a fresh install was performed.
