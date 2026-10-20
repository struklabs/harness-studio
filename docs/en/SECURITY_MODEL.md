# Security and Privacy

[Türkçe](../tr/SECURITY_MODEL.md)

Harness Studio runs local models on your computer. Downloads, remote models, HTTP MCP servers, and optional integrations contact the services you choose. The app sends no telemetry; diagnostic reports are stored locally. Report vulnerabilities privately using [SECURITY.md](../../SECURITY.md).

## Credentials and Network Access

- Hugging Face credentials and llama-server API keys are stored in the OS keychain. Signing out removes local Hugging Face credentials; revoke access at Hugging Face to fully disconnect the account.
- Cloud provider API keys (OpenAI, Gemini, Anthropic, OpenRouter, custom endpoints) are stored only in the OS keychain of the device where you add them, never in settings files or logs. The Gateway sends a key only to its own provider's address, does not follow redirects, and never returns the provider's message for a rejected key. Requests to a cloud model leave your computer; any client allowed to use the Gateway can use these models and spend your provider credits, so restrict models with named tokens when you share the Gateway.
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

## Headless Browser Administration (0.2 W5, Slice 1)

`harness serve` serves the bundled Svelte static build on the Control API listener (see [CLI setup](CLI.md#browser-management)). Desktop IPC and the Gateway keep their existing authentication boundaries. The browser shell exposes servers, Library/definition loading, Hugging Face downloads, basic launch defaults, and named tokens; native pickers, folder reveal, and local integrations are omitted. Paths and operations refer to the server machine.

- **Bootstrap:** Until an administrator exists, the terminal prints a random 256-bit one-time setup code, valid for 15 minutes. It is held only in memory, never sent to the browser or stored in ready JSON. Successful setup atomically saves an Argon2id password hash with a random salt to private `config/browser-admin.json` and consumes the code. Minimum password length is 12 bytes. Restart generates a fresh code only while setup is incomplete. This slice has one administrator; multiple users/roles are later work.
- **Sessions:** Login rotates the cookie identifier. A host-only `__Host-harness` cookie is `Secure; HttpOnly; SameSite=Strict; Path=/`, with an eight-hour absolute lifetime. At most 64 sessions are active. Sessions and CSRF secrets live only in memory and are invalidated by listener/process shutdown. No bearer token is exposed to JavaScript or browser storage. Logout revokes the current session; Settings revokes all browser sessions. Admin bearer clients may also call `DELETE /api/harness/v1/browser/sessions`. Cookie-authenticated SSE connections, including logs and telemetry, close within one second of expiry/revocation.
- **CSRF/origins:** Cookie-authenticated writes require an exact same-origin `Origin` and a session-bound `X-Harness-CSRF` header, obtained from the same-origin session endpoint. Login/setup require the same origin too. Cross-site fetches are rejected, including origins on the API CORS allowlist; CORS never grants cookie administration. Bearer clients continue through existing scope checks. Browser administrators have the existing `admin` scope with a distinct browser principal; they cannot read local-owner-only server API keys, even on loopback. The scope includes server file/process authority; sign-in explicitly explains this grant.
- **Abuse controls:** Failed setup/login attempts use a process-wide exponential backoff (2-64 seconds) with `429` and `Retry-After`. Password verification is serialized, off the async executor; spoofed IP/proxy headers cannot bypass the backoff. The global limit deliberately trades availability under attack for bounded password work. Restart resets the limiter. Authentication responses and API data use `Cache-Control: no-store`.
- **TLS:** User-supplied PEM certificates/keys apply to the entire Control API/static UI listener. Network binds require a configured exact `public_origin`, existing network approval, and a named API token. Proxy headers are never trusted. Plain HTTP on a non-loopback bind prints a warning and refuses password login; use direct TLS. Loopback HTTP is for local development in browsers that accept Secure localhost cookies. Gateway TLS and ACME automation are outside this slice.
- **Recovery:** Restart revokes sessions without changing the password or named tokens. With local owner access, stop the server and move the private administrator hash aside to repeat setup; protect the replacement code as an administrator credential. Named API tokens remain independently scoped and revocable.
