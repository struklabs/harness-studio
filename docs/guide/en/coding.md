# Coding

**What you do here:** let a local or remote model write code, fix bugs, refactor and run tests inside one project folder, review every change as a diff, and undo what you do not want. Coding is not an IDE: it has no editor. It works in the folder of a [Project](04-playground.md) and shows you what changed.

Open **Coding** in the sidebar. The model runs through the Harness Gateway, so any model the gateway offers works, local or from a cloud provider. The Gateway must be running; if it is not, the page says so and links to **Connections**.

## Start a Task

1. Pick a project in the list on the left. Only projects with a folder appear; create one on the **Projects** page (**Manage Projects**).
2. Read **Workspace Permissions** (below).
3. Choose a **Model**. Models that can call tools are listed first; use one that can, because every action is a tool call.
4. Type the task in **What should be done?**, or press a starter (**Write Code**, **Fix a Bug**, **Refactor**, **Run Tests**) and finish the sentence.
5. Press **Run**. **Progress** lists each tool call as it happens. **Stop** ends the run.

Before the model touches anything, Harness Studio takes a checkpoint of the folder.

## Workspace Permissions

The card at the top of the page shows what a run may do:

- **Read and write** only inside the project folder. `..`, absolute paths outside it and symbolic links out of it are refused. Extra folders of the project are read grants for other features; Coding does not use them.
- **Protected:** `.git` is never written.
- **Reading and editing files** run without a question: edits are checkpointed, so you can undo them.
- **Commands** always wait for your approval. The **Command Approval** card shows the full command and marks commands that delete files, discard Git work, install packages, reach the network or talk to a remote repository. A short list of commands is refused outright, for example `sudo`, wiping a drive, or piping a download into a shell. This is a safety net, not a sandbox: an approved command can do what you can do, so read it. A command is limited to 60 seconds.
- If the tool policy in **Settings** is **Off**, no tool runs. A policy of allow-all or a trusted shell tool does not let a command through without your approval in Coding.

## Review and Undo

**Review Changes** lists the files changed since the selected checkpoint, with added and removed line counts and a colored diff for the file you pick. The expand button opens a focused full-page review; **Esc** closes it.

- **Undo This File** puts one file back.
- **Undo All** puts every changed file back, including files a command created or deleted.
- **Checkpoints** lists the restore points; every run and every patch takes one. **Create Checkpoint** takes one by hand, for example before you edit the folder yourself.

How a checkpoint works depends on the folder:

- **Git** (the folder is the top level of a Git repository and `git` is installed): the folder is saved as a Git tree through a private index kept in Harness Studio's data folder. Your repository's index, branches, stash and working state are not touched; a hidden ref under `refs/harness/checkpoints/` keeps the data. Ignored files are neither reported nor restored.
- **Snapshot** (anything else): a copy of the folder's files in Harness Studio's data folder, without `node_modules`, `target`, `build`, `dist`, `.venv` and similar folders, files over 8 MB and symbolic links. Folders with more than 20,000 files or 512 MB are refused with a hint to use Git or a smaller folder.

The last 30 checkpoints of a project are kept.

## Apply a Patch

**Apply Patch** takes a patch in unified diff format (what `git diff` prints). The whole patch is checked first: every path must stay in the project folder and every hunk must match the file as it is now. If anything does not fit, nothing changes and no checkpoint is made. Otherwise a checkpoint is taken, the files are changed (created, edited or deleted) and the patch appears in **Review Changes** like any other change.

## Other Coding Tools

OpenCode, Codex, Claude Code, Continue, Cursor and Zed keep working against the same gateway; setup is on the **Connections** page, described in [Connections and the Gateway](05-connections-and-gateway.md#connect-a-coding-agent) and the [integrations reference](../../en/INTEGRATIONS.md). Use them when you want their terminal or editor experience; use **Coding** when you want a folder-bound run with permissions, a diff review and undo in one place, with models that never have to leave your machine.

### How They Compare

This is a short, practical comparison drawn from each tool's public documentation at the time of writing; their behavior changes, so check their own documentation before relying on a detail.

| Tool | Where it lives | How it works | Safety model |
| --- | --- | --- | --- |
| Claude Code | Terminal | An agent loop with file, search and shell tools and a project memory file | Per-tool permission prompts and permission modes; checkpoints that can rewind its file edits |
| Codex | Terminal | An agent that edits through a patch format and runs commands | Approval modes plus an optional sandbox limiting writes to the workspace |
| OpenCode | Terminal UI | Provider-neutral agent with separate planning and building modes | Configurable permissions per tool; undo and redo of a session's file changes |
| Continue | Editor extension | Chat, inline edit and agent modes inside VS Code or JetBrains | Edits appear as editor diffs you accept or reject |
| Cursor | Editor (VS Code fork) | An agent working in the editor, with project rules | Inline diffs to accept or reject; checkpoints to restore earlier states |
| Zed | Editor | An agent panel in a fast native editor | A review view of the agent's edits to accept or reject; tool permissions |

### What We Borrowed

- **Approval per command** (Claude Code, Codex): shell commands are the one thing that can reach beyond the project, so each one is put to you with its full text.
- **A workspace boundary** (Codex's sandbox idea): file tools see one folder; the shell is not confined, so it is gated by approval instead of pretended to be safe.
- **Checkpoints before work and undo** (Claude Code, Cursor, OpenCode): a restore point before every run, and the same undo covers files a shell command changed, not only the model's own edits.
- **A review view of the whole change** (Zed, Cursor, Continue): the files and diffs the run produced are shown together, with undo per file.
- **Unified diffs as the exchange format for patches** (Codex's patch idea, plain `git diff` output): easy to read, easy to produce from any tool.

### What We Left Out

There is no editor, no inline completion and no language-server features: those are what the editors above are for. There is also no process sandbox; permissions, approval and undo take its place for now.

Next: [Tools](06-tools.md).
