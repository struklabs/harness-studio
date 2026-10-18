# CLI, Control API, and MCP

Türkçe: [../tr/09-cli-api-mcp.md](../tr/09-cli-api-mcp.md)

**What you do here:** understand what the terminal tool, the management API, and MCP are for, and find the detailed references.

You do not need a terminal to chat, download models, or start servers. This chapter is for people who want another program to use Harness Studio.

| Route | What it provides | Detail |
| --- | --- | --- |
| CLI (`harness`) | Managing models, servers, profiles, logs, and benchmarks from a terminal; chatting and running without the interface (headless). It also covers speech, images, GGUF conversion, LoRA scales, router mode, and analytics from the command line. | [CLI reference](../../en/CLI.md) |
| Control API | Lets other programs manage Harness Studio's models and servers. It is a separate service from the gateway you talk to models through. | [Security model](../../en/SECURITY_MODEL.md), [CLI reference](../../en/CLI.md) |
| Harness MCP server | Lets clients such as Claude Code, Codex, OpenCode, or Cursor use tools to list and load models and read metrics. | [MCP reference](../../en/MCP.md) |

The Developer settings and MCP connection pages below are **Advanced** pages. Switch using **Simple / Advanced** or open the page through the command palette; the interface level does not change CLI or API permissions.

## Turn on the Control API

1. Open **Settings > Developer > Control API**.
2. Enable **Enable the API** and press **Apply**.
3. Check the **Base URL**. The default is `http://127.0.0.1:11500/api/harness/v1`; if the port is busy, the address shown can differ.
4. To see the API's description, use **Open** in the **OpenAPI Schema** field. The default OpenAPI schema address, `http://127.0.0.1:11500/api/harness/v1/openapi.json`, only makes sense on this computer while the API is on; if the port changed, use the link in the app.

Management requests are protected by a token. The exceptions for local `/health` and schema access, and the token scopes, are explained in the [security model](../../en/SECURITY_MODEL.md). When it manages the app's data folder, the CLI reaches it through this API; if the API is off, the CLI cannot connect to the owning instance.

On the **CLI** page (**Settings > Developer > CLI**), **Use in a Terminal** shows the command-line tool and **Add to PATH** makes `harness` available from a terminal; open a new terminal afterwards. CLI output is in English.

## Use the API from Code

The repository has ready-made clients for the Control API in [`sdk/`](../../../sdk/README.md): a TypeScript package and a Python package. They are generated from the OpenAPI schema, so every endpoint, parameter and answer has a type. Install TypeScript with `npm install @struklabs/harness-studio-sdk`; install Python from the repository folder (PyPI publication is planned).

Both find the running Harness Studio by themselves: the app, or `harness serve`, writes its address to `instance.json` in its data folder, and the session token is in `api-token` next to it. On another computer, give the address and a named token instead.

```python
from harness_studio import connect, unwrap
from harness_studio_api.api.system import get_status

status = unwrap(get_status.sync_detailed(client=connect()))
print(status.version)
```

An error answer becomes an exception (`HarnessError` in Python, `HarnessApiError` in TypeScript) that carries the same `code` the app shows, for example `api.insufficient_scope` when the token lacks a scope.

## Two Directions of MCP

When Harness Studio is an **MCP server**, other agents call its management tools. `harness mcp` is the local command route; the Control API also offers an HTTP MCP endpoint. Follow the [MCP reference](../../en/MCP.md) for setup and scopes. You can also add it to a client from **Connections > Tools and Clients > Add Harness MCP**.

When Harness Studio is an **MCP client**, it brings the tools of other MCP servers into the Playground. Add servers, or import them from other applications, in the **MCP Servers** area of **Connections**. This connection and connecting to a model's chat endpoint are separate things.

Offering the built-in file, command, and network tools to other MCP clients is off by default. Turn it on with **Offer Tools to MCP Clients** in **Settings > Tools > Built-In Tools**. The permission policy still applies; calls that cannot ask you for approval over MCP are denied. Set the permissions of the tools you will use according to the [Tools chapter](06-tools.md).

See also the [coding agent integrations](../../en/INTEGRATIONS.md) and the [security model](../../en/SECURITY_MODEL.md).

Next: [Troubleshooting and FAQ](10-troubleshooting.md).
