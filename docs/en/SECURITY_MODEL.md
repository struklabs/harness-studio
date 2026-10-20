# Security and Privacy

[Türkçe](../tr/SECURITY_MODEL.md)

Harness Studio runs local models on your computer. Downloads, remote models, HTTP MCP servers, and optional integrations contact the services you choose. The app sends no telemetry; diagnostic reports are stored locally. Report vulnerabilities privately using [SECURITY.md](../../SECURITY.md).

## Credentials and Network Access

- Hugging Face credentials and llama-server API keys are stored in the OS keychain. Signing out removes local Hugging Face credentials; revoke access at Hugging Face to fully disconnect the account.
- The Control API and Gateway are off by default. Enable them in Settings only when needed. Use named tokens with the narrowest scopes and revoke unused tokens.
- The Gateway accepts local requests without a token by default. Enable **Require a Token on This Computer Too** to enforce token restrictions for every local client. Otherwise, requests without a token bypass model and endpoint restrictions.
- Sharing the Gateway over a network requires a token. Review bind addresses and allowed browser origins before sharing. Harness does not configure your firewall.
- Integration setup previews configuration changes and creates backups. HTTP MCP configuration can contain plaintext credentials; protect these files and backups. Stdio avoids writing a named token into client configuration.

## Models, Runtimes, Tools, and Agents

- Review download sources and integrity warnings. Unverified third-party runtimes require explicit approval; custom runtimes remain your responsibility.
- Review tool permissions before installing or enabling a package. JavaScript packages have scoped file and public-network access and cannot start shell processes. Tool approval does not make an untrusted package safe.
- Native programs, stdio MCP servers, agents, and module backends run with your OS account's authority. Process stop controls and agent folder grants are not an OS security sandbox. Remote MCP services receive the arguments and results sent to them.
- Stop active tasks before changing grants. Disable or remove connections to prevent future use; revoke provider credentials separately. Revocation cannot undo writes or recall data already sent.
- Hermes connections use model-scoped inference tokens. Disconnect revokes the connection token and restores configuration when it has not been changed externally. See the [agent guide](HERMES_AGENT.md) for recovery and limits.

## Saved Content

- Gateway analytics record request metadata by default, without prompts or responses. Optional per-definition content recording saves plaintext content locally. Give analytics readers access only to content you intend to share.
- Stateful `/v1/responses` requests store conversation content locally by default, separately from analytics. Send `"store": false` or disable **Store Responses** in Gateway settings to stop new storage. Previously saved responses remain until expiry or **Delete All**.
- On a tokenless local Gateway, local callers share anonymous stored responses. Require separate named tokens to separate clients.
- Conversations and agent task history can contain private prompts, outputs, and granted folder names. Review them before exporting or sharing. Deleting analytics does not delete conversations.
- Analytics retention and deletion are available in Analytics; stateful response retention and deletion are in Gateway settings. Optional OpenTelemetry export sends metadata to the collector you configure; leave it disabled unless needed.

See the [user guide](../guide/en/README.md), [CLI reference](CLI.md), [Tools reference](TOOLS.md), and [third-party notices](THIRD_PARTY.md).
