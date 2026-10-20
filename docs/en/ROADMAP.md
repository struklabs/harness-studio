# Roadmap

[Türkçe](../tr/ROADMAP.md)

Harness Studio manages local text, image, speech, and tool workloads in one place. Local first, with optional engines and cloud connections. This roadmap shows direction; priorities may change with feedback. Share suggestions in [Discussions](https://github.com/struklabs/harness-studio/discussions) or [issues](https://github.com/struklabs/harness-studio/issues). See the [changelog](../CHANGELOG.md) for details.

## Shipped in 0.1.2

- [x] **Playground:** generative UI for tool results, tables, charts, and simple UI blocks.
- [x] **Model Discovery:** Best for My Hardware ranking and fit badges; sorting that matches Hugging Face.
- [x] **Everyday Controls:** Simple and Advanced modes, polished page layouts, a section menu that stays open, Entire Internet tool permissions, and Clear Log in Automatic Tool Builder.
- [x] **Security:** archive extraction rejects symbolic links and paths outside the destination and removes privileged permission bits.

## Shipped in 0.1.1

- [x] **Engines:** transcribe.cpp, llama.cpp Qwen3-TTS, mflux images on Apple Silicon, and broader MLX speech support including CSM.
- [x] **Model Coverage:** more ONNX vision, NLP, and audio tasks, image editing, video generation, and audio processing with speaker diarization.
- [x] **Connections:** Harness Link brings remote Harness models to Playground and the local gateway.
- [x] **Desktop Experience:** accessibility and high interface scale fixes, first-run improvements, and more responsive long chats and model libraries.
- [x] **Platforms and Stability:** macOS updates, Linux AppImage support, and fixes for memory pressure, downloads, and model startup.
- [x] **SDKs:** TypeScript SDK published on npm; TypeScript and Python clients checked against the Control API in CI.
- [x] **Linux Updates:** AppImage and `.deb` packaging; AppImage in-app updates and manual `.deb` update guidance ([PR #101](https://github.com/struklabs/harness-studio/pull/101), [PR #106](https://github.com/struklabs/harness-studio/pull/106), [changelog](../CHANGELOG.md)). Broader end-to-end verification remains open.

## Existing Foundation

- [x] **Headless Serving:** `harness serve`, Control API, and Gateway are implemented ([source](../../crates/harness-cli/src/serve.rs)); browser management and full server operations remain planned.

## Release Follow-Ups

- [ ] Verify Linux on more AMD/NVIDIA machines and distributions; complete first-run, accessibility, and benchmark checks before dropping preview.
- [ ] Verify AppImage updates end to end and the manual `.deb` update path; packaging and updater integration have shipped.
- [ ] Add CUDA vLLM after Linux NVIDIA verification.
- [ ] Publish the Python SDK on PyPI through Trusted Publishing.
- [ ] Sign the Windows installer after the website and signing service are ready.
- [ ] Enable LFM2.5-Audio output when upstream llama.cpp supports it.

## 0.2

Deliver vertical slices in order: each wave includes setup, use, permissions, and recovery. Every third wave (W3, W6, …) improves delivered flows. Module foundations do not move Later features into 0.2.

### W1 — Discover and Install a Module

- [ ] Keep the core lightweight; distinguish Modules (functional areas), shared Components, Skills, MCP servers, Connectors, Tools, and Client Integrations. Plan optional Training, Agents, Automations, Paperclip, and REA areas with clear navigation.
- [ ] Let users inspect a module's functions, dependencies, permissions, and disk estimate, approve installation, and open its workspace from the same menu. Separate first-run onboarding, module installation, and reusable job setup.
- [ ] Use versioned manifests, API/IPC boundaries, and isolated backend processes with compatibility checks. Ship module UI with the app; download only backends/dependencies, without downloaded screens or external JavaScript UI.
- [ ] Extend the existing `struklabs/harness-registry` for official StrukLabs Skills, MCP servers, and Tools only. Verify existing content; provide search, sorting, filters, descriptions, version details, and CI checks via a versioned index and maintainer PRs, without waiting for a website or automatic publishing.

### W2 — Connect an Agent and Complete a Task

Delivered slices: [Hermes Agent connection, tasks, scoped read permissions, history/audit, and existing Paperclip model handoff](HERMES_AGENT.md). The broader OpenClaw, upstream update, and Tool Builder items below remain open.

- [ ] Install OpenClaw or Hermes Agent on demand, or connect existing installations; manage upstream versions, setup, start/update, missing dependencies, and first-run tests while preserving external interfaces.
- [ ] Supply local or Harness Link models to OpenClaw, Hermes Agent, and an existing Paperclip instance; recommend models for task/hardware needs and configure endpoints/keys securely. Paperclip lifecycle management remains Later.
- [ ] Manage agent tasks/sessions, model connections, status, performance, errors, and scoped tool permissions with approval, audit, and stop controls; do not grant blanket access by default.
- [ ] Improve the existing Automatic Tool Builder's create/test/save flow and permission display; distinguish user-created and installed Tools, support Agent Skills/`SKILL.md`, and share Harness tools with external coding agents instead of duplicating their workflows.

### W3 — Improvement Wave: Reliable Installation and Agent Use

First slice: [Module Lifecycle](MODULE_LIFECYCLE.md) covers the existing Speech Recognition and Hermes Agent modules; the broader security and licensing work below remains open.

- [ ] Complete module enable/disable, update, repair, and removal with shared-component ownership, disk accounting, compatibility checks, rollback, and practical offline install/removal. Prevent removal of components still in use.
- [ ] Separate deletion of jobs, conversations, models, and physical files; preserve datasets, models, projects, and valuable outputs during module removal and app uninstall, with explicit deletion choices.
- [ ] Extend the existing Security Model with trust levels for MCP, agent runtimes, trainers, and Tools; explain file/folder, network, shell, service, account, and external-data permissions at grant/revocation. Use OS sandboxing where available and document Windows/macOS/Linux isolation limits.
- [ ] Make states, dependencies, progress, errors, and recovery consistent; check disk space and process cleanup so modules do not freeze the app. Review third-party licensing, redistribution, commercial use, embedding, hosting, and attribution before shipping integrations.

### W4 — Choose a Model and Use Relevant Tools

Discovery and setup slice: [Discover and Model Setup](MODEL_SETUP.md) connects the existing Registry, Modules, MCP settings, Playground, and Library; dedicated Connector catalogs and provider model connections remain outside this slice.

- [ ] Show the selected model's chat, vision, image, STT, TTS, video, embedding, reranking, and tool-calling capabilities; clearly label experimental or incompatible model/runtime combinations.
- [ ] Route requests to relevant Tools/MCP capabilities through on-demand discovery and schema loading instead of attaching every schema to every request; expose routing errors and permission needs.
- [ ] Provide distinct Discover categories for Skills, MCP servers, Connectors, Tools, and bundled Modules; keep module/backend installation metadata separate from the official Skills/MCP/Tools Registry.
- [ ] Clarify first-model setup, resource selection, model switching, and execution location; distinguish models, providers, agents, workflows, and devices throughout the flow.

### W5 — Manage an Existing Harness Server

First slice: [browser administration](CLI.md#browser-management) adds a static Svelte management shell, one-time administrator setup, password/cookie sessions, CSRF checks, backoff, session/token revocation, and user-supplied TLS on the existing Control API listener. Multiple users, ACME, service operations/packaging, and the SDK publication audit remain open.

- [ ] Build browser management on shipped `harness serve`, Control API, Gateway, Harness Link, and authorization capabilities; review existing boundaries before extending them.
- [ ] Deliver browser sessions, HTTPS, users, and scoped tokens with clear connection approval, revocation, and errors for remote administration.
- [ ] Add server updates, logs, backup/restore, and service lifecycle flows with Linux service packages and Docker images.
- [ ] Audit Control API and TypeScript/Python SDK coverage; verify package-registry publication status and distinguish published packages from source-only clients in setup guidance.

### W6 — Improvement Wave: End-to-End Release Readiness

- [ ] Verify discover → install → connect model → run agent/tool → recover → remove flows across supported platforms, including permission revocation and user-file preservation.
- [ ] Measure UI responsiveness, disk use, and process/memory cleanup during concurrent module/server use; fix regressions before expanding scope.
- [ ] Validate lazy tool routing, capability labels, safe endpoint/key handling, and compatibility across local and linked models.
- [ ] Refine onboarding and reusable setup through first-model, first-agent, and remote-server scenarios; document delivered limits, dependency versions, and recovery steps in both languages.

## Later

### Training

- [ ] Start with one existing backend and a small reproducible training PoC. Compare Unsloth, LLaMA-Factory, MLX-LM, and Axolotl on actual model/GPU/OS support; assess LoRA/QLoRA, SFT, DPO, multimodal, full fine-tuning, and pretraining before choosing the initial method.
- [ ] Offer reusable Training Jobs with goal/model/dataset/device selection, drafts/back/resume, dataset cleaning/format/split/license checks, estimates, trial runs, queues, metrics, evaluation, and checkpoints; pause/resume only where supported. Recommend compatible methods from architecture, weight format, drivers, compute, VRAM/RAM, CPU, disk, library versions, and measured performance; explain unsupported combinations. Add backend overrides when more backends become available.
- [ ] Manage LoRA export, merge, conversion, quantization, and Library import; run on a user's connected GPU server and monitor from desktop/mobile without requiring Harness Cloud GPUs.

### Agents and Orchestration

- [ ] Extend orchestration with tested model/context/tool-calling requirements, justified model/provider switching, BYOK, concurrency, multiple devices/models, retries, timeouts, cost/token tracking, intermediate outputs, approvals, and recovery. Compare DeepSeek Harness patterns and Core/module/adapter ownership.
- [ ] Integrate Paperclip installation or an existing instance through supported APIs for teams, roles, agents, tasks, runs, budgets, model protocols, health, and Projects links; preserve upstream workflows and add Harness screens only where useful.

### Automations

- [ ] Connect to the user's own n8n instance; show workflows, triggers, models/providers, runs, history, errors, and editor links. Supply Gateway models and scoped file/tool access; do not embed, distribute, install, or operate an automation engine.
- [ ] Let Playground draft, create, edit, and trigger workflows through permitted n8n APIs/MCP with approval and result inspection; validate real API capabilities and applicable integration/commercial terms.

### REA

- [ ] Add an optional Playground analysis workspace using REA's existing CLI/MCP engines for supported EXE/DLL, Electron/JavaScript, .NET, APK, web, and binary targets; select target, analysis type, model, and engine without building a decompiler or full IDE.
- [ ] Detect Node.js and installed engines; install only required components, explain Ghidra/Hopper/IDA requirements and paid licenses, and separate inspection, execution, debugger, ADB, network, and device permissions for authorized private analysis.
- [ ] Show progress, evidence, history, findings, pseudocode, symbols, call graphs, comparisons, and reports where supported; expose official REA MCP/Skills to OpenCode, Codex, and other clients using Harness models, with coherent Analysis/Coding navigation.

### Coding

- [ ] Explore a project-scoped workspace with local/remote models for code generation, file edits, tools, tests, debugging, and refactoring; learn from OpenCode, Codex, Claude Code, Continue, Cursor, and Zed while retaining client integrations and avoiding a full IDE.
- [ ] Show workspace permissions, changed files, commands, task progress, and reversible patches/checkpoints; support mobile task monitoring and focused change review.

### Context and RAG

- [ ] Retrieve context from documents, projects, agent tasks, tool outputs, and past decisions with a reliable small embedding component, preferably 200–300 MB or less; manage model changes, index compatibility, updates, portability, and multilingual quality.
- [ ] Link text, audio/video transcripts, timestamps, images, OCR/vision, metadata, and sources through separate extraction/retrieval stages as needed; measure indexing time, CPU/RAM/disk, latency, and battery cost without keeping the GPU continuously busy.

### Playground

- [ ] Extend chat with task-specific Create/Studio workspaces for image/audio/video, voice conversion, documents, coding, analysis, and agents; add a ComfyUI bridge, preserve existing multimodal features, and avoid duplicate Core/module screens. Compare integrated experiences such as Atomic Chat.
- [ ] Match each task's API, streaming, files, artifacts, and compute needs; support long video jobs with progress, cancellation, output formats, and file previews.

### Projects and Artifacts

- [ ] Group conversations, files, tools, agents, training jobs, outputs, and permissions in lightweight Projects/Workspaces; share artifacts with source, metadata, permissions, and lifecycle across modules without a heavy database or required cloud.
- [ ] Support drafts, back/forward steps, and later resumption for Agent Tasks and Analysis Sessions. Continue jobs across devices using history, task state, file references, and checkpoints; explain model/provider compatibility limits without requiring live RAM migration.

### StrukLabs Account and Web

- [ ] Design shared identity across StrukLabs products for profiles, devices, sync, and remote services; compare OIDC/SSO/OAuth and managed/serverless identity options on security, maintenance, and cost.
- [ ] Provide website product, account, release, documentation, subscription, and cloud-service management as needed; maintain the official Registry through its repository, without community submissions, moderation, or a Registry admin panel.

### Mobile

- [ ] Build a mobile-designed full client with navigation, approvals, streaming, file viewing, and job management; compare Svelte/Capacitor, Kotlin/Swift, and alternatives on code sharing, native APIs, background work, notifications, store policies, and maintenance.
- [ ] Use home/server models or standalone BYOK/custom endpoints from a phone; manage models, modules, agents, training/REA status, settings, and performance with scoped roles. Show provider versus GPU-device connections, execution location, and agent model/device/capabilities clearly.

### Remote and Devices

- [ ] Extend existing connections with secure pairing, device verification, scoped tokens, approval, audit, revocation, and lost-phone recovery; enable phone chat and remote OpenCode access to a selected Gateway.
- [ ] Compare direct LAN, internet P2P, and relay fallback using WebRTC, QUIC, or outbound WebSocket against NAT/firewalls, mobile reliability, bandwidth, and cost; keep inference/files on selected devices where practical and transfer large GGUF files directly when possible.
- [ ] Show device roles, models, runtimes, permissions, disk, workload, and availability; recommend another device when local hardware is insufficient and install only role-appropriate components. Use StrukLabs for identity/discovery/signaling instead of mandatory inference transit.

### BYOK

- [ ] Unify desktop/server/mobile provider setup, model discovery, capabilities, quotas/errors, and cost for OpenAI, Gemini, Anthropic, OpenRouter, and custom endpoints; verify Chat Completions, Responses, Anthropic Messages, Ollama, streaming, tool, and multimodal protocol differences.
- [ ] Share connection tests, error explanations, and secret handling while allowing keys to remain only on the user's chosen phone or desktop.

### Performance

- [ ] Improve task/hardware recommendations using measured loading, benchmarks, and tool-calling reliability; separate hardware fit, task fit, quality, and speed across inference, training, agents, images, and audio.
- [ ] Coordinate competing training/image/inference jobs with memory estimates, priorities, queues, GPU reservations, cancellation, and safe fallback; track regressions.
- [ ] Evaluate faster-whisper after an NVIDIA benchmark and VieNeu when SEA-G2P is practical; decide whether OpenVINO and MLC add useful coverage.

### Security

- [ ] Expand workload permissions for intensive GPU/CPU use, microphone, and camera; assess malicious packages, prompt injection, dependency updates, supply-chain exposure, credential leaks, and reversible agent changes.
- [ ] Offer anonymous, consent-based crash/environment/log reports with explicit defaults and retention; exclude conversation, model, and file contents from telemetry.

### Open Source

- [ ] Keep one monorepo, including future web/account/mobile work; establish real module boundaries and targeted builds/tests first. Later support multiple backends/components and module-provided integrations through versioned contracts and compatible backend updates.
- [ ] Clarify MIT core, SDK distribution, third-party contributions, and paid-service boundaries; prefer sustainable upstream contributions/adapters over forks and document update/support responsibilities. Keep the Registry official-only, without package signing or community publishing.
- [ ] Add separate GitHub search/URL discovery with README, releases, license, platform fit, purpose, popularity, and maintenance metadata; evaluate official API limits, OAuth, indexing/cache, and cost. Distinguish discovery from installability and official catalog inclusion.
- [ ] Validate discovered `SKILL.md` references/scripts, security, version pins, and installation scope; explore official MCP Registry metadata/protocol validation and explain local stdio versus remote HTTP. Recommend an official package before drafting a missing Skill/MCP/Tool/Connector; add Gmail, Excel, Drive, and calendar Connectors with OAuth, consent, privacy, and revocation.

### CI and SDK

- [ ] Use targeted CI for changed Rust crates, Svelte modules, Registry manifests, and SDKs; add integration tests at critical boundaries and full matrices at scheduled/release milestones. Define versioned APIs/SDKs for module and third-party developers.
- [ ] Turn dogfooding of Harness development, tools/Skills, agent tasks, small fine-tuning, Registry installation, and mobile management into reproducible QA; move small reliable cases into CI.

### Commercial

- [ ] Evaluate optional paid online, storage/sync, relay, and team services; budget identity/metadata/Registry/signaling separately from transfer/storage/push, avoiding mandatory central inference or large-file transit.
- [ ] Compare self-hosted and managed web infrastructure on reliability, maintenance, transparent operating costs, and portability without provider lock-in.

## Not Planned

- Pulling from the Ollama registry (`hf.co/...` references are supported).
- Requiring Python for the core app or duplicating native model coverage with Python engines; optional engines remain add-ons.
