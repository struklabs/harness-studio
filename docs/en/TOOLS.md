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

`equals` compares the expected and actual text after applying `trim_end` to both sides: only trailing line-ending and whitespace differences are ignored. Leading and inner whitespace must match exactly. The report keeps the original expected/actual text; run output is not truncated. The `contains`, `matches`, and `length` rules are unchanged.

The **All / Queued / Running / Installed / Awaiting approval / Failed** filters above the queue show the count for each state. Running covers the build, test, and install stages. The counters also select filters: Scanned and Planned show the whole queue, and Built shows the cards that have a draft. The selection is kept in local storage; the filter works even when storage access is disabled. Reordering by drag and arrows is available only in the All/Queued views. **Retry Failed Tools** queues failed tools the way an individual retry does; generation is continued with **Resume**.

The `expect` in a package test contains exactly one check: besides `equals`, `contains`, and `error`, you can use `matches` (a JavaScript RegExp inside the sandbox; use `^…$` for a full match) or `length` (the number of Unicode characters in the result text, zero included). For example, for a random password: `expect: { matches: /^[A-Za-z0-9]{12}$/ }`; searching the whole alphabet with `contains` is not a correct test. String results are compared as plain text; boolean/number/object/array/null results are compared as JSON text. `undefined` has no JSON equivalent and is normalized to `null`; an empty string stays empty.

The `matches` pattern takes a pattern string, a `/pattern/flags` string, or a JS regex literal. The `source` and `flags` of a RegExp object are carried over before the case is serialized. QuickJS evaluates the JS syntax (including lookahead) and the flags; each case uses a new RegExp and `lastIndex` is not shared. An invalid pattern/flag yields `bad_case`; there is no comparison with Rust regex.

The generation response is first a `json` block that contains only the manifest object, then two `js` blocks headed `index.js:` / `test.js:`; the `js index.js` / `js test.js` info-string form is also accepted. JavaScript is written raw. The older single-JSON form (`manifest`, `index_js`, `test_js`) remains backward compatible. If serde reports an invalid escape, only invalid escapes in JSON string literals are repaired once by doubling the backslash; valid escapes and other JSON errors are left alone. A truncated fence goes through the truncation/retry path. The ideation stage stays strict JSON.

The retry feedback asks to fix the test for `bad_case`, and to compare the code and the expectation against the contract for other errors. An expectation that contradicts reasonable/correct behavior may be corrected; the model must state the case name and the reason in a one-line JS comment. Weakening a valid expectation and deleting a case are forbidden. The generator requires at least two cases. If the case count in a retry drops below the previous largest suite, the harness runs the old suite against the new code, reports the failures, and prevents the draft from passing; the same bound is kept in later retries. This numeric check does not prove the semantic correctness of the expectations; a code/test review is still required.

After the first response, the generator asks for at most four corrections. As long as the context allows, each correction sends the model the previous **full response** (including code and tests) and the expected/actual results of all failed tests. In the older single-JSON form, surrounding explanations and JSON fences are accepted; in the new form, blocks must be labeled. Malformed or ambiguous multiple packages use the same retry budget. The response budget is 8192 tokens and there is no custom `stop` sequence. If the API reports `finish_reason=length` or an unclosed block, the response counts as incomplete; the previous failed draft is kept and shorter code is requested. In single- or double-quoted strings in JavaScript, a real line break is a syntax error; raw JS blocks remove the need for double JSON/JS escaping. A syntax error is not presented as a half-finished model response; the feedback points to the `index.js` or `test.js` file.

On the **Create → Automatic Tool Generator** page, select a model and press **Automatically Create Tools**. The default is 8 tools and at most 20 are planned. Harness applies no quota; a remote provider's own limits and charges may apply.

1. Built-in tools, installed packages, and connected MCP tools are scanned with their name/description/category/source information. The wait for MCP connections is limited (8 s); so that late-connecting servers are not missed, the inventory is re-read before every ideation attempt, and tools that appear later are written to the activity log as "Late tools".
2. The model proposes new purposes. Strict JSON is validated; the same name, a semantically equivalent name (a token set folded by stem/inflection and synonyms; `word_counter` ≈ `count_words`), and high description similarity are eliminated. Malformed/repeated proposals are requested once more. This lexical filtering (no embeddings or server needed) does not catch every semantic equivalence; after filtering, the queue can be shorter than requested.
3. The queue is processed automatically. You can prioritize pending cards by dragging or with the up/down buttons, and remove them. The order of the running card does not change. Pause/stop cancels the model request; resume regenerates the interrupted tool. A failed card can be retried.
4. The current generator produces a manifest, JS, and tests for each package; it passes errors to the model and corrects up to four times. Tests run without access to user files or the real network. File inputs are tested with per-case `fs` fixture contents and network inputs with `http` mock responses; pure logic and access denial can also be tested; the behavior of a real external system must be verified separately.
5. A package that passes its tests and asks for no network/file access is installed permanently. A package that asks for network or file access moves to **Awaiting approval**. The card shows Entire Internet or the domain restriction, read/write folders, and the shell ban; **Install with These Permissions** installs that package. Permissions are never widened silently. A failed package is not installed; existing tools are not overwritten.

The queue, package drafts, states, and the last 500 activity entries are stored in `auto-tools/run.json`. Leaving the page does not stop the work. If the application closes, the run is paused on reopening; select a model and continue. Endpoint credentials are not kept in this file. A new run cannot start until cards that await permission or have not yet been processed are finished or removed.

**Clear Log** in **Live Activity Log** clears both the visible and saved log. It is disabled while a control action is busy or the log is empty. It leaves the queue, results, inventory, token totals, and running generation intact.

### CLI and Control API

```powershell
harness tools inventory --json
harness tools auto start --endpoint http://127.0.0.1:11440/v1 --model Qwen3-4B-Instruct-2507-Q4_K_M --count 4 --json
harness tools auto status --json
harness tools auto start --endpoint http://127.0.0.1:11440/v1 --resume --json
harness tools auto action pause
harness tools auto action retry --id ITEM_ID
harness tools auto action reorder --order ITEM_ID_2,ITEM_ID_1
```

The CLI start command waits for completion; Ctrl+C stops the run. API: `GET /tools/inventory`, `GET /tools/automatic`, `POST /tools/automatic/start` (`endpoint`, `count`, `resume`), `POST /tools/automatic/action` (`action`, `id`, `order`). The paths are under the Control API's `/api/harness/v1` prefix. The `/tools/automatic` operations, including reading the run/drafts, require the `admin` scope. The API `approve` action accepts only a stored card id; obtain human approval in your own interface by showing the exact permissions. The CLI uses the app/API instead of permission approval.

CLI ownership: `harness tools auto` commands go to the Control API through `session::connect`; they never open the `run.json` file directly. If the GUI/`harness serve` holds the data directory's `instance.lock`, the current owner's API is used. If the API is off, `cli.owner_without_api` (exit 4) is returned; if the owner information cannot be read yet, `cli.lock_held`; a second local writer is never started. If there is no owner, a temporary instance first takes the operating system file lock and holds it for the duration of the command. In a lock race it is likewise redirected to the new owner's API or fails with an error. For this reason no new file-lock dependency was added for the CLI; the state that the automatic generator writes from memory is changed only inside this single owner.

Pause/stop is also passed to the running sandbox test's QuickJS cancellation token; a long JS loop or network wait is cut off without waiting for the 30-second test limit. A broken package manifest fails only its own card with an error message; the other tools in the queue continue to be processed.

### Smart Correction and Generator Tokens

First generation + four corrections = at most **five model calls** (`build:1`…`build:5`). A correction response gives `CHANGE: what I am changing and why` on its first line; the parser keeps this line out of the package. The note, the number of failed cases, tokens, repeat/strategy, and context-summary information are stored in the draft and in the card history of the automatic run. For models that give no note, an explicit empty-note text is shown.

If the whitespace-normalized index.js is identical to an earlier attempt, or the failed case names and actual outputs recur, the next feedback asks for a different approach. Two consecutive repeats with at least two attempts remaining switch the corrections to a test-first flow: first only test.js is reviewed (a change to index.js is rejected), and then the code is rewritten from scratch with the simplest correct approach. If only the last attempt remains, the test-review step is skipped and the code is rewritten directly; the final round is not consumed by a review that forbids code correction. The case-count guard, the `bad_case` guidance, the network-mock requirement, and keeping the previous draft on a truncated response continue to apply. These checks do not prove that the tests semantically conform to the contract.

The conversation size of each request is measured with the existing compaction counter: llama-server `/tokenize`, a compatible count_tokens, and as a last option ≈ three characters/token + message overhead. The n_ctx in `/props` is read; if unknown, a safe default of **16,384** is used. If the conversation + **8,192** response tokens do not fit, earlier attempts are turned into one short summary; the system message, the original user request, the last full response, and the last feedback are kept verbatim. The summary note carries the shortened expected/actual case feedback. If these fixed parts alone exceed the window, the original request is not trimmed; the server may reject the request.

Usage `prompt_tokens` and `completion_tokens` are totaled at the attempt, tool, and run levels; ideation calls and truncated completions are included. If API usage is missing, our own count is used, with `estimated:true` and **≈** in the UI. The fields can read older run.json files through serde defaults. An individual/batch retry is added to the same item's spending; the spending of a removed card is not deleted from the run total. The real reply average of the last 100 earlier attempts feeds the next automatic-run estimate; if there is none, 2,000 reply tokens are assumed.

The initial estimate is the planning prompt count + the measured system/request per tool and the expected reply × N × 1.25–3.5 calls (expected retries 0.25–2.5; a range for the growing retry context). The manual Create generator does not add planning. Because of model/tool length and the growth of the retry conversation, this is not a guarantee or a quota. The counter strip, the tool cards, Review Package → Attempt History, and the live `↑input ↓output` lines show the real spending. The final difference card subtracts the midpoint of the estimated range from the actual total.

Generation/ideation sends `x-harness-client: harness-auto-tools`, and manual generation sends `harness-tool-creator`. The gateway uses this for logging; a second record is not made. **Direct analytics API limit:** analytics/direct.rs keeps only a per-server/per-minute total; there is no client label or request-record API, and telemetry totals are not double-counted with a manual direct_add. A per-client direct record would need an API extension.

The manual component of the Create page is in `create/ToolGenerator.svelte`. The backend returns draft tokens on both call paths; the manual estimate/history view is on the Create page.

### Generator Error Handling

Text/data conversions (CSV/JSON, HTML, Markdown, base64) take string content and return content by default in the planning and generation prompts; a file contract is suggested only for explicit file operations. Existing packages with a file contract are supported by fixture tests; permission approval continues. QuickJS provides no DOM (`DOMParser` and `document` do not exist). For Markdown, the line/block distinction, heading levels, and a last line without a line ending are stated explicitly in the prompt.

The counter strip uses 2/3/4/7 fixed grid columns depending on the window width; a change in the number does not determine the column count or the label position. Numbers use `font-num`, filter counts use a fixed `3ch`, and card/manual tokens use a minimum `12ch` area. The live log and attempt tokens use tabular digits. A failed card gives a short TR/EN summary; the technical error, test output, and history stay under **Inspect Package**.

Copies of the code/tests/manifests of four packages from a real generation record are in `crates/harness-core/tests/data/tool-generator-reported-failures.json`. The CSV fixture regression first failed with `path does not exist: input.csv` and passed after the fix. With the real record's code, the missing file and Markdown's `<p><h1>Header 1</h1><br>#<h1>Header 2</h1><br>### Header 3</p>` output are reproduced. Base64 passes with the same code once a fixture is added; after the HTML fixture, the second error, a missing `DOMParser`, becomes visible. A late Markdown retry accepts the code fix and passes the tests in the fifth round with a fake chat endpoint. These tests do not guarantee that a real Qwen model will generate successfully; they cover the generator's handling of the recorded failures.
