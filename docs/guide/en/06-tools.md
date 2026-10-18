# Tools

Türkçe: [../tr/06-araclar.md](../tr/06-araclar.md)

**What you do here:** choose the tools the model may call, set permissions, and generate new tools when you need them.

## Built-In Tools

A tool is something the model does besides generating text. The **My Tools** page in the **Create** section of the left menu (an Advanced-level section) shows tools from every source. You choose which ones a chat uses in the Playground ([Playground > Use Tools and Review Approvals](04-playground.md#use-tools-and-review-approvals)); in the Simple level, the **Web and Tools** switch in the message box offers web search and your selected tools to the model.

| Function | Tool names |
| --- | --- |
| Reading and searching files | `read_file`, `file_glob_search`, `grep_search`, `get_info` |
| Writing and editing files | `write_file`, `edit_file` |
| Running commands | `exec_shell_command` |
| Date and math | `current_datetime`, `calculator` |
| Network, search, and weather | `fetch_url`, `web_search`, `geocode`, `weather` |

File and command tools need a working folder. Network tools send requests to outside services. In **Settings > Tools > Built-In Tools** you can pick a provider under **Web Search**. Keyless search can be blocked; SearXNG, Brave Search, and Tavily are also available.

## Choose a Permission Policy

1. Open **Settings > Tools > Built-In Tools** (an Advanced page).
2. Review the **Permission Policy**.
3. Set a **Default folder** for file tools. Playground chats can also choose their own working folder.
4. When a call arrives in a chat, read its name and parameters and then approve it.

| Policy | Behavior |
| --- | --- |
| **Off** | Tools do not run. |
| **Ask every time** | Safe tools run; file reads, writes, and command calls ask for approval. |
| **Allow trusted tools** | Safe tools and the ones you marked as trusted run without asking; the others ask for approval. |
| **Allow all** | All tools, including file writes and commands, run without approval. |

A model cannot widen its own permissions. Running commands has a broader effect than only reading files, so read a command before you approve it. The details of the permission boundaries are in the [security model](../../en/SECURITY_MODEL.md).

## Tool Creator

1. In the left menu open the **Create** section, then **Tool Creator**, and choose the model endpoint. A running server or the gateway must be on.
2. In **What should the tool do?**, describe a small, concrete job. Example: "Count the words in the given text and return the three most frequent words."
3. Choose **Network Access**: **No Network Access**, **Entire Internet** (default), or **Only These Domains**. For the last option, fill in **Allowed Domains**, one per line; an empty list grants no network access. **Readable folders** and **Writable folders** are optional; empty lists deny file access. Folders are relative to the working folder. Local/private addresses and Harness listener ports remain blocked in every network mode.
4. Press **Create**. The model writes code and a test, and Harness Studio runs the test in a sandbox.
5. Review **Code (index.js)**, the **Manifest**, the permissions, and the test result. A tool that does not pass its test cannot be approved.
6. Enable the tool with **Approve and Use in This Session**. A session tool goes away when the app closes. Use **Save Permanently** to keep it.

Generated JavaScript tools can reach only the network and file areas listed in their package permissions; they cannot run commands. This limit differs from the built-in `exec_shell_command` tool. Approving the installation does not remove the permission policy for later calls.

## Automatic Tool Generator

1. In the **Create** section, open **Automatic Tool Generator**.
2. Choose a **Model** and the **Number of Tools to Plan**, then press **Automatically Create Tools**.
3. Reorder the jobs in the **Build Queue** by dragging or with **Move Up** and **Move Down**.
4. Use **Pause**, **Resume**, or **Stop** if needed. The queue and the results are saved.
5. Packages that ask for network or file access stay at **Awaiting approval**. Look at one with **Inspect Package** and, if you accept it, choose **Install with These Permissions**.

Tools that need no network or file access are installed automatically. Outside access is off during testing. For failed generations, read the error detail and use **Retry Failed Tools**.

The automatic builder has the same network choices and defaults to **Entire Internet**. The selection limits what the model may request; review the actual scope before installation. In **Live Activity Log**, **Clear Log** clears the visible and persisted entries without changing the queue, results, inventory, token totals, or running generation. It is disabled while a control action is busy or the log is empty.

## Installed Tools and MCP

On **Settings > Tools > Installed Tools** you can manage package activity, **Run Test**, and **Remove**. For a ready-made package there is **Install from Folder...**; read the permissions and choose **Confirm Permissions and Install**.

Tools from MCP servers are added under **Connections > MCP Servers**. An MCP server that uses a local command runs programs with your user permissions. Exposing Harness Studio's own tools to another MCP client is a separate setting; see [CLI, Control API, and MCP](09-cli-api-mcp.md).

For the package format, tests, and how generation works, use the [Tools reference](../../en/TOOLS.md). If a model does not call tools correctly, see the [tool diagnosis notes](../../en/TOOL-DIAGNOSIS.md).

Next: [Performance and Analytics](07-performance-and-analytics.md).
