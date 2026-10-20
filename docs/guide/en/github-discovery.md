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

For skills, review frontmatter and resource validation, the script list, and
network-use hints. These static checks are not a security audit: instructions can
request additional commands or network access. **Pin to Version and Install as
User Content** opens a confirmation showing the exact commit SHA and scripts.
Confirming copies the reviewed skill and resources into the user skills directory
without running scripts or overwriting existing skills. The skill starts disabled;
review it under installed skills before enabling it. Large repositories, unsafe
paths, symlinks, missing references, and oversized resources are refused.

MCP servers are never installed automatically. Review the copyable configuration
templates and declared environment variables, secrets, transport, and arguments.
Metadata may come from repository manifests or the public MCP Registry, which is
separate from the Harness official catalog. Templates need manual completion;
unknown transports and inferred environment hints require verification against
the repository's documentation. Copying metadata does not start a server.
