# Setup Scenarios: First Model, First Agent, Remote Server

**What you do here:** follow one of three guided setups, see what each one is limited to, and recover when a step fails.

The **Get Started** list on the **Dashboard** overview tracks all three. It reads the real state of the app, so progress is never lost, and you can hide it with **Hide** and bring it back with **Open Get Started** (or the command palette entry **Open Get Started**). **Reopen the Setup Wizard** in the same list runs the first-run wizard again. It works the same way in Simple and Advanced.

## Scenario 1: First Model

1. Finish the setup wizard, or choose **Install Engine** in **Get Started**. The managed engine is the official llama.cpp release.
2. Open **Find a Model** (**Model Hub** in Advanced), open a chat model's **Files and quants**, check **Hardware fit**, and press **Download**.
3. In **Dashboard**, press **Choose a Model**, pick the file, and press **Start Server**.
4. Open **Chat and Media**, then **Chat**, and send a message.

Known limits and versions:

- Managed llama.cpp comes from the official `ggml-org/llama.cpp` GitHub releases. The installed build number (for example `b1234`) and backend (CUDA, Vulkan, Metal, CPU) show under **Settings > Runtime > Installed Runtime**. Updates install only when you press **Check for Updates** or turn on **Update Automatically**.
- **Hardware fit** is an estimate. A model marked as fitting can still fail when other programs use memory.
- Downloads need a network connection; chatting afterwards does not.

If something fails:

| Problem | What to do |
| --- | --- |
| Download fails or stops | Check the network and free disk space, then retry from the **Download Queue**. A partial file resumes where it stopped. A license-gated model needs a Hugging Face account (see [Troubleshooting](10-troubleshooting.md#license-gated-model-and-hugging-face-account)). |
| Model does not fit | Choose a smaller quant (Q4) or a smaller model, or lower **Conversation Length**. See [memory recovery](03-dashboard-and-servers.md#when-memory-runs-out-oom). |
| Server does not start | Read the start failure message and the server log; reinstall or switch the engine build in **Settings > Runtime**. See [Troubleshooting](10-troubleshooting.md#the-runtime-engine-is-missing-or-will-not-start). |

## Scenario 2: First Agent

1. Have a local chat model ready (Scenario 1, steps 1 to 3). Start the local Gateway in **Settings**.
2. Open **Modules > Agents**. Read the installation permissions and install Hermes Agent.
3. Choose a chat model (a recommended one shows hardware fit and tool-calling evidence) and press **Connect**.
4. Type a first task, grant the shown access for this task, and press **Run Task**.

Known limits and versions:

- Hermes Agent is installed with the official upstream installer from Nous Research, with a pinned source commit. The bootstrap script itself is not hash-pinned and download sizes are estimates. An existing external installation keeps its own upstream update flow.
- Tasks run inference only by default. You can grant read-only access to up to eight folders; shell and write access cannot be granted. This is an application-level restriction, not an operating-system sandbox.
- Speech and the managed Hermes runtime prepare their own Python through uv. Harness uses uv 0.12.20 and the Python 3.12 series, downloaded on first use and kept inside the app data folder.
- Details: [Connect an Agent and Complete a Task](../../en/HERMES_AGENT.md).

If something fails:

| Problem | What to do |
| --- | --- |
| Installation fails | Read **Install Output**, install the missing dependencies it names (Python, uv, git), then retry or use **Repair**. |
| **Connect** is unavailable | Start the local Gateway in **Settings**, select a chat model, and enable the token requirement if you want the restriction to bind all local clients. |
| Task fails | Read the error under the output, choose a model that supports tool calling, and run the task again. **Stop** ends the task process. |
| Connection left in a bad state | **Disconnect** restores your original `config.yaml`. If it refuses because the file changed, inspect `config.yaml.harness.bak` and restore it by hand. |

## Scenario 3: Remote Server

1. On the other machine, install and start `harness serve` (see [Run Harness as a Linux Server](10-server-mode.md)). For first checks, open `http://localhost:11500/` on that machine, or through an SSH tunnel, and set the administrator password.
2. Create a named token for this computer on the server.
3. In **Connections** (**Connect an App** in Simple), open **Harness Link**, add the server's address and token, and press **Test Connection**.
4. Its models appear in **Chat and Media** and **My Models** as remote entries.

Known limits:

- Plain HTTP is accepted only for local or private-network addresses and needs an explicit confirmation. Use trusted TLS for anything else.
- Remote models are listed read-only: you can use them, but you cannot download, edit, or delete them from this computer.
- Renaming a link changes its model IDs.

If something fails:

| Problem | What to do |
| --- | --- |
| Server unreachable | Check that the server runs, the address and port, the firewall, and the TLS setting. Try an SSH tunnel first. See [Cannot Connect to the Server](10-troubleshooting.md#cannot-connect-to-the-server). |
| Unauthorized | The token is missing, revoked, or for another address. Create a new named token and enter it again. |
| Models do not appear | Press **Test Connection**, then reload; a disabled link hides its models. |
