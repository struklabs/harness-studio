# Discover Public GitHub Content

Open **Discover → GitHub** to search public repositories or paste an
`https://github.com/owner/repository` URL. GitHub discovery is separate from the
official Harness catalog and does not guarantee that a repository can be installed.
When a repository matches a catalog package, prefer the suggested official package.

Search works without authentication. An optional GitHub token can be saved in the
OS keychain; save an empty token to remove it. Tokens are sent only to the GitHub
API. Disk-cached responses expire after 15 minutes. If GitHub limits requests,
wait for the displayed cooldown before retrying.

Review a repository to see its README excerpt, latest release, license, stars,
update time, topics, and unverified platform hints. Missing metadata stays unknown.

For skills, the review checks frontmatter and that every file or directory
SKILL.md refers to exists, and lists scripts that SKILL.md never mentions. A
**Security Summary** scans the text of each script for risky patterns without
running anything: network access, shell and process execution, dynamic code
(`eval`, decode-and-run), credential paths and environment secrets, and compiled
files that cannot be read. It shows a risk level (none, low, medium or high)
and the flagged lines, grouped by category. These static checks are not a
security audit: a clean scan does not prove a skill is safe, and instructions in
SKILL.md can still ask the model for commands or network access.

**Pin to Version and Install as User Content** opens a confirmation showing the
exact commit SHA, the scripts, the risk level, and the install scope. Skills
always install in the user scope (the user skills directory, shared by every
project), never into a project. A high-risk skill needs a second checkbox
confirming you read the flagged lines. Confirming copies the reviewed skill and
resources into the user skills directory without running scripts or overwriting
existing skills. The install fails if the files or the risk level at that commit
differ from what you reviewed. The skill starts disabled; review it under
installed skills before enabling it. Large repositories, unsafe paths, symlinks,
missing references, and oversized resources are refused.

MCP servers are never installed automatically. Review the copyable configuration
templates and declared environment variables, secrets, transport, and arguments.
Metadata may come from repository manifests or the public MCP Registry, which is
separate from the Harness official catalog. Templates need manual completion;
unknown transports and inferred environment hints require verification against
the repository's documentation. Copying metadata does not start a server.

**Registry Metadata and Protocol Checks** evaluate a repository's `server.json`
and any matching official MCP Registry entry without contacting the server. They
check that the name uses the reverse-domain format and that its `io.github.<owner>`
namespace matches the repository owner, that a description, version and official
schema are present, that package versions are pinned (not `latest`), and that
remote addresses use HTTPS without embedded credentials. Each entry is labelled
by how it runs:

- **Local (stdio):** the server is a process on your computer, started with your
  permissions from a downloaded package. You run that package's code.
- **Remote (HTTP):** the server runs on someone else's machine and you connect
  over HTTPS. No code runs locally, but the data you send goes to that service.

Before you have a Skill, MCP server, Tool, or Connector drafted by a model,
Harness suggests matching official catalog packages and built-in
[Connectors](connectors.md) so you can use a reviewed package instead.
