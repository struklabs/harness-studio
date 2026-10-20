Türkçe: ../tr/TOOLS.md

# Writing and Creating Tools

Harness tools share the same registry and permission policy in the Playground, the CLI, and the Control API. **Create → My Tools** lists all built-in, installed/generated, and MCP tools. MCP tools are grouped by server, and each source is grouped by function. Disabled or broken installed packages are listed too; they do not count as runnable tools.

## Writing a Package by Hand

Put `manifest.json`, `index.js`, and preferably `test.js` in a folder. An optional `README.md` appears in the tool details.

```json
{
  "name": "text_reverse",
  "version": "0.1.0",
  "category": "text",
  "description": "Reverses the character order of a text.",
  "input_schema": {
    "type": "object",
    "properties": { "text": { "type": "string" } },
    "required": ["text"]
  },
  "output": "The reversed text.",
  "permissions": {
    "network": [],
    "fs": { "read": [], "write": [] },
    "shell": false
  },
  "runtime": "js",
  "entry": "index.js",
  "test": "test.js"
}
```

| Field | Rule |
|---|---|
| `name` | `[a-z][a-z0-9_]{1,47}`; the tool-call name and the folder name. Must not collide with an existing tool. |
| `version` | Semantic version, for example `0.1.0`. |
| `category` | `file`, `web`, `calculation`, `datetime`, `text`, `other`. `other` if missing; an unknown value is rejected. |
| `description` | Up to 1024 characters explaining when the model should call the tool. |
| `input_schema` | A JSON Schema with `type: object`; parameters via `properties` and `required`. |
| `output` | What the result text contains. |
| `permissions` | Network scope and folder permissions relative to the workspace; an empty list grants no access. For network, `["*"]` grants the entire public internet. |
| `runtime` | `js` (QuickJS). |
| `entry`, `test` | Relative file paths inside the package; absolute paths and `..` are forbidden. `test` is optional. |

`index.js`:

```js
function run(args) {
  if (typeof args.text !== 'string') throw new Error('text must be a string');
  return args.text.split('').reverse().join('');
}
```

The result can be a string or any value that converts to JSON. The `btoa`, `atob`, `TextEncoder`, `TextDecoder`, `JSON`, `Math`, `Date`, and `RegExp` globals are available. Missing web globals are provided in pure JS; existing implementations are not changed and no new file, network, or process access is granted. `btoa`/`atob` use Latin-1 binary strings; `btoa` input outside Latin-1 or invalid base64 raises an error. `atob` accepts ASCII whitespace and missing padding. To encode Unicode text, first convert it to UTF-8 bytes with `new TextEncoder().encode(text)`, then to a binary string, and then use `btoa`. `TextEncoder.encode` returns a Uint8Array; `TextDecoder.decode` decodes an ArrayBuffer or typed-array view as UTF-8. The polyfill decoder supports UTF-8 only and no streaming; it replaces malformed bytes with U+FFFD by default and throws with `{fatal: true}`. `require`, `import`, `fetch`, `process`, and timers do not exist. There are limits on memory, time, and result size.

### Network and File API

- `harness.http.get(url)` and `harness.http.post(url, jsonBody)` are synchronous and return `{status, content_type, url, body}`. `permissions.network` has three scopes: `[]` denies network access, `["*"]` permits the entire public internet, and other lists permit only those domains (`example.com`, `*.example.com`; no protocol or path). `*.example.com` covers subdomains but not the root domain; `"*"` cannot be combined with other entries. Existing empty lists still deny network access. Every scope blocks loopback, link-local/metadata, private networks, CGNAT, IPv6 ULA, and Harness ports. Every DNS answer and redirect is checked; connections are pinned to validated addresses and system proxies are disabled.
- `harness.fs.read(path)` reads text, `harness.fs.write(path, text)` writes, and `harness.fs.list(path)` lists a directory. Paths are relative to the chat's workspace. Read and write permissions are separate; write does not grant read. `"."` covers the whole workspace, and `"out"` covers only that folder and below. Symbolic links cannot reach outside the boundary.
- `shell` can only be `false`. JS packages cannot start processes.

`test.js`:

```js
const cases = [
  { name: 'reverse', args: { text: 'abc' }, expect: { equals: 'cba' } },
  { name: 'invalid input', args: {}, expect: { error: 'must be a string' } },
  {
    name: 'network call',
    args: { q: '123' },
    http: [{ url_prefix: 'https://example.com/', status: 200, body: 'mocked' }],
    expect: { equals: 'mocked' }
  }
];
```

Each `expect` contains exactly one option: `equals`, `contains` (a string or a list of strings), or `error` (`true` or an error text). Each test case runs in its own empty temporary workspace. For file input, add `fs: [{path: "input.csv", content: "name,age\nAlice,30"}]` to the case; the runner writes the UTF-8 content before `run(args)`. Nested folders are created. Relative paths are used; absolute paths, `.`/`..`, empty path components, and duplicate files are rejected. At most 32 files and 1 MiB of total content are accepted per case. Preparing a fixture does not grant the tool any permission: the manifest's read/write scopes apply unchanged. Files written by a case are not carried over to the next case; the temporary folder is cleaned up when the case ends. A `setup` callback is not supported and is rejected with an explicit error; a `file://` mock inside `http` does not create files. Tests of packages that make network calls can define an optional `http` array (URL match, HTTP status, headers, and body). If a test case defines `http`, `harness.http` calls use ONLY the mock responses and never reach the network (an unmatched request is rejected with an error). In the automatic generator's tests, real network access is off and file access is directed to a temporary workspace containing only the fixtures; the automatic generator always writes the tests of every network tool with `http` mocks, otherwise the test fails with a `network` denial and the model is asked to fix it. Network tests of hand-installed packages (and of cases that use no mocks) can reach the real network.

### Installing and Managing

In **Create → My Tools → Installed Tools**, select the folder. Review the permissions shown and confirm the installation. The package is copied under `tools/<name>/` in the application's data directory. Enable/disable, test, inspect details, and remove are in the same place. The installed-tool management in Settings uses the same component. Installing does not loosen the permission policy for tool calls.

## Tool Creator

On the **Create → Tool Creator** page, select the model endpoint and the model; the default endpoint is the currently running server/gateway. Describe the tool, choose its network scope, and optionally list allowed domains and relative folders. The model produces the package and its tests; it repairs invalid JSON, permission overruns, or test failures with up to four correction attempts. You can read the manifest, code, tests, and permissions and approve them for the session, then save permanently. The **Create Tool…** button in the Playground opens a generator dialog using the same backend. Small models may write faulty packages more often.

The tool builder defaults to **Entire Internet**. Choose **No Network Access** or **Only These Domains** to restrict it. The domain textarea applies only to the last option; an empty list grants no access. Your choice is the upper bound on permissions the model may request; overruns are rejected and sent back to the model for correction. Tools requesting network/file access, including in the automatic builder, require explicit approval at install; the permission summary labels the unrestricted public scope **Entire Internet**.

`null` and `undefined` returns are the four-character `"null"` result text not only in tests but in normal execution; an empty string `""` stays empty. The Playground result card shows `null` and sends the same text in the next model turn. The gateway's shared tool loop also passes it to the model as a successful tool result; it is not an error or cancellation marker. A tool that must give the user a meaningful confirmation should return an explicit result string.

## Automatic Tool Generator

Select a model on **Create → Automatic Tool Generator**, then choose **Automatically Create Tools**. Review the planned queue, reorder pending tools, and pause or resume generation. Use **Retry Failed Tools** to retry failures.

Generated packages include tests. Review the code, test results, and permissions before saving. Network or file access requires explicit approval. Tests use temporary files and network mocks; passing them does not guarantee that an external service will work. Model quality varies, and remote provider charges may apply.

Use the queue filters and activity log to inspect progress. Token estimates are approximate; attempt history shows reported usage. Failed packages remain available for inspection and retry.

### Agent Skills

In **Create → My Tools** or **Settings → Tools**, open **Agent Skills**. **Open Skills Folder** opens `<data directory>/skills`. Add `<skill-name>/SKILL.md` with YAML frontmatter containing `name` (matching its folder) and `description`, followed by Markdown instructions. Optional metadata is accepted; `allowed-tools` does not grant permissions. See the [Agent Skills specification](https://agentskills.io/specification).

Refresh to discover local changes. Enable/disable preserves the folder; removal asks for confirmation and deletes the skill and its resources. Invalid skills show an error and are excluded from the model catalog. Official registry Skills can be reviewed and installed from their pinned catalog revision. The current registry distributes standalone `SKILL.md` files; local skill folders can also contain resources.

With Playground Tools enabled, only enabled skill names/descriptions enter the system context. When relevant, the model calls `load_skill` with `name` to retrieve instructions, then optionally `resource` (for example `references/guide.md`) for a UTF-8 file relative to that skill. Files are bounded to 1 MiB and confined to that folder, including symlink resolution. Resource files and scripts are read, never automatically executed. Existing tool permissions still apply. Loading depends on the model's tool-calling support and selection; unrelated skill bodies are not attached to every request.
