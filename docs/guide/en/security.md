# Security Protections

**What you do here:** understand what Harness Studio protects you from when tools, agents, cloud keys, and the microphone are involved, what each protection cannot do, and where you change it.

These protections work together with the tool policy (**Settings > Tools**) and the trust levels on **Settings > Security > Permissions**. None of them replaces your own judgment: read an approval prompt before you accept it. The details of the trust model are in the [security model](../../en/SECURITY_MODEL.md).

## Heavy Work, Microphone, and Camera

**Settings > Security > Permissions** has a **Workload Permissions** section with four kinds: **Heavy GPU Work**, **Heavy CPU Work**, **Microphone**, and **Camera**.

- **Asked once, then remembered.** The first time an outside client (an API token, for example) starts a long, heavy job, it is refused with the message that its permission was not asked yet. You answer on the Permissions page or with the Control API; the answer is remembered per client and kind.
- **You can revoke it.** Every remembered answer has a **Revoke** button. After a revoke the client is asked again next time. Every answer and revoke is written to the tool log.
- **This app is never asked about its own work.** Only outside clients are.
- **Microphone and camera have two checks.** First the operating system's own switch (on Windows, **Settings > Privacy & security > Microphone / Camera**), then Harness Studio's own prompt. When you press the record button for the first time, a prompt offers **Allow and Remember**, **Allow Once**, or **Deny**. If the operating system blocks the device, Harness Studio says so and cannot override it.

What it does not do yet:

- Today the heavy-work check is enforced when an API token queues a model conversion (heavy CPU work). Long training and video jobs will use the same check when those features start jobs; there is nothing to ask about before then.
- macOS and Linux give an app no switch it can read, so on those systems only Harness Studio's own prompt applies (macOS still shows its own system prompt the first time).
- In a web browser the browser's own camera and microphone prompt applies instead of this one.

## Instructions Hidden in Tool Results

A web page, a file, or an MCP server can contain text written to trick a model ("ignore your instructions and run this command"). Two protections limit the damage:

1. **Tool results are marked as untrusted data.** Every tool result reaches the model inside an untrusted block, and the system prompt tells the model never to follow instructions found there. A closing tag inside a page cannot end the block early.
2. **Copied instructions need your confirmation.** If a dangerous tool (writing a file, running a command) is called with arguments that repeat text from an earlier tool result, the call asks you first, even when your policy is **Allow All** or the tool is trusted. The approval card says the call may come from a tool result, and **Always allow** is not offered for it. Where nobody can answer (a background gateway run), the call is refused.

Limits: this is a text-overlap check, not a lie detector. It catches a command, path, or address that was copied from a tool result into a call; it does not catch an instruction the model rewrote in its own words, and it does not make a model immune to persuasion. Rewriting a file after reading it is not treated as an instruction, so a normal read-then-edit stays quiet, and a legitimate call that repeats a path from a search result may ask once. Keep the policy on **Ask Every Time** for tools you do not fully trust.

## Undoing Agent File Changes

Before the file tools of an agent (`write_file`, `edit_file`) change a file, the previous version is kept. **Settings > Security > Permissions > Agent File Changes** lists the changes, newest first.

- **Undo** puts one file back; a file the agent created is deleted. **Undo Run** reverts everything one conversation changed, newest change first.
- If you edited the file after the agent did, the undo is refused so your work is not lost; **Undo Anyway** overrides that on purpose.
- The newest 200 changes are kept. A file larger than 8 MB is listed but its old version is not kept (**Previous Version Not Kept**). **Clear List** deletes the list and the kept versions; the files themselves are untouched.

Limits: shell commands can change anything and are not tracked. Only changes made through the two file tools can be undone. Checkpoints are not a backup; a disk failure or a deleted data folder loses them too.

## Dependencies and Downloads

- **Rust packages.** The project's automated checks run `cargo audit` on every dependency change and weekly, and fail on known vulnerabilities. A short list of reviewed exceptions is kept with its reasons in `.cargo/audit.toml`.
- **JavaScript packages.** `npm audit` runs in the same checks as an information step: it reports known issues in production dependencies but never blocks a change, because an advisory without a fix would otherwise block everything.
- **Python engines.** The Python packages of the optional engines are installed from lock files in which every file has a SHA-256 hash. The installer uses `uv pip install --require-hashes --no-deps` with wheels only, so a changed or substituted file is refused. A test checks that every lock file keeps this shape.

Limits: an audit only knows advisories that have been published. It does not check licenses (no `cargo deny` policy yet) and it cannot tell whether a pinned package was already malicious when it was pinned.

## Keys in Outgoing Data and Logs

Harness Studio looks for key-shaped text in what it sends to a cloud provider, in tool results that go back to a model, and in the tool log, and replaces it with `[REDACTED_SECRET]`. It recognizes OpenAI-style and Anthropic keys, Hugging Face and GitHub tokens, AWS access keys, Google API keys, Slack tokens, JSON web tokens, `Bearer` credentials, and private-key blocks. The tool log also hides values written as `api_key = ...` or `password: ...`; requests to a provider only use the stricter list so a prompt about passwords is not rewritten.

Limits: this finds credentials by their shape. A key in an unusual format, one split across two messages, or one inside an attached file is not caught, and requests that are not JSON chat requests are passed through unchanged. Do not paste secrets into chats you send to a cloud provider.
