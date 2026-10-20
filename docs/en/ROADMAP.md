# Roadmap

[Türkçe](../tr/ROADMAP.md)

Harness Studio brings local models, tools, and optional engines into one desktop app. These are directions, not release commitments; priorities may change with user feedback.

## Delivered in 0.2

- Optional modules you can install, update, repair, and remove while your models and data stay.
- An official registry for tools, MCP servers, and skills, with Agent Skills (SKILL.md) support.
- Agents: Hermes Agent, OpenClaw, and Paperclip, with task history, per-run permissions, and an audit log.
- Cloud providers with your own API key, routed through the gateway.
- A connection to your own n8n instance.
- Knowledge bases for local retrieval with cited sources, and Projects that group chats, files, and agent tasks.
- A prioritized GPU job queue with cancellation.
- Browser admin and server mode for headless machines.
- A first-run Setup window and an uninstall window with per-item removal.
- Opt-in local diagnostics reports; nothing is uploaded.

## Next

- Discover skills and MCP servers from GitHub.
- Improve agent setup, model recommendations, and recovery.
- Make model capabilities and compatible tools clearer before starting a workload.
- Improve Linux compatibility, application updates, and accessibility.
- Expand SDK availability and supported engine options.

## Later

- Optional training and richer project workflows.
- More remote-device and cloud-provider connections.
- Better context management and document retrieval.

## Open Source Discovery

Discover includes a separate GitHub search and repository URL view. Public
repository discovery is distinct from installability and the official catalog:
skills require a pinned commit and manual script confirmation, while MCP servers
provide configuration for manual setup. See the [GitHub discovery guide](../guide/en/github-discovery.md).

See [Features](FEATURES.md) for current capabilities and the [changelog](../CHANGELOG.md) for shipped changes. Share suggestions in [Discussions](https://github.com/struklabs/harness-studio/discussions) or [Issues](https://github.com/struklabs/harness-studio/issues).
