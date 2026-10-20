# Harness Studio Features

[Türkçe](../tr/FEATURES.md)

Harness Studio manages local AI models, servers, tools, and connected applications. For setup and walkthroughs, start with the [user guide](../guide/en/README.md). Planned work is in the [roadmap](ROADMAP.md).

| Area | What You Can Do |
| --- | --- |
| Model Discovery | Browse and search Hugging Face, filter models, check hardware fit, and download supported variants. Sign in for private or gated repositories. |
| Model Library | Manage local models, import existing files, create model definitions and aliases, and choose load settings. |
| Runtime Management | Install managed runtimes or choose custom binaries; select supported GPU backends and review updates. |
| Servers | Start and stop models, use saved profiles, inspect memory estimates, and recover from startup errors. |
| Playground | Chat with local or linked models, use supported image/audio capabilities, attach files, branch conversations, and export history. |
| Context Management | Summarize older turns automatically or with `/compact`, keep recent messages, and undo compaction without deleting originals. |
| Tools | Enable built-in tools, install JavaScript packages, generate tools, and review file and network permissions. |
| Agent Skills | Install or add `SKILL.md` instructions, enable relevant skills, and load their resources through tool calls. |
| MCP | Connect external MCP servers or expose Harness model management to coding clients through stdio or HTTP. |
| Integrations | Configure compatible coding tools and apps, preview changes, test connections, and undo Harness configuration. |
| Gateway | Use OpenAI-, Anthropic-, and Ollama-compatible endpoints, model aliases, on-demand loading, and optional named-token restrictions. Compatibility varies by endpoint and client. |
| Remote Models | Use Harness Link to connect another Harness instance and select its models locally. |
| Cloud Providers | Add OpenAI, Gemini, Anthropic, OpenRouter, or an OpenAI-compatible endpoint with your own API key, kept in the system keychain, and chat with its models through the gateway. |
| CLI and Control API | Manage models, servers, conversations, and tools from scripts; run headless serving and use the published API schema and SDKs. |
| Performance and Analytics | Run your own benchmarks, inspect request timing and errors, monitor RAM/VRAM, and use optional metrics export. |
| Modules and Agents | Install optional capabilities and connect Hermes Agent to a selected model with task, stop, history, and permission controls. |

## Simple and Advanced Navigation

Simple mode shows Dashboard, Find a Model, My Models, Chat and Media, Connect an App, and Settings. Advanced also exposes Servers, Create, Performance, Analytics, and detailed settings. Switching modes preserves saved settings.

## Engines and Compatibility

Supported engines cover text, vision, images, speech recognition, speech generation, and other media workloads. Availability depends on the model, runtime, hardware, and operating system. Experimental engines are labeled in the app. A downloadable model is not necessarily compatible with every engine.

## References

- [CLI](CLI.md), [MCP](MCP.md), and [Integrations](INTEGRATIONS.md)
- [Writing and Creating Tools](TOOLS.md) and [Hermes Agent](HERMES_AGENT.md)
- [Security and Privacy](SECURITY_MODEL.md) and [Third-Party Notices](THIRD_PARTY.md)
- [Release Notes](RELEASE_NOTES_0.1.2.md) and [Changelog](../CHANGELOG.md)
