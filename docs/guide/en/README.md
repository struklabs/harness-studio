# Harness Studio User Guide

Türkçe: [../tr/README.md](../tr/README.md)

**What you do here:** find the right chapter and follow the five steps that get you from a fresh install to your first local chat.

Harness Studio is an open-source project by struklabs. It lets you download AI models to your own computer, run them, and connect them to the tools you already use. You do not need a terminal for your first chat. The engine that runs the model is llama.cpp, and the chat screen is the Playground.

Names in **bold** are labels you will see in the app. Model Hub and engine downloads need an internet connection. Once a model and an engine are on your computer, chatting happens entirely on your machine.

## Contents

1. [Installation and First Launch](01-installation.md)
2. [Model Hub and My Models](02-model-hub-and-library.md)
3. [Dashboard and Servers](03-dashboard-and-servers.md)
4. [Playground](04-playground.md)
5. [Connections and the Gateway](05-connections-and-gateway.md)
6. [Tools](06-tools.md)
7. [Performance and Analytics](07-performance-and-analytics.md)
8. [Settings](08-settings.md)
9. [CLI, Control API, and MCP](09-cli-api-mcp.md)
10. [Troubleshooting and FAQ](10-troubleshooting.md)
11. [Setup Scenarios: First Model, First Agent, Remote Server](11-setup-scenarios.md)
12. [API Stability and Versioning](12-api-stability.md)
13. [Training: Fine-Tune a Model on Your Own Data](13-training.md)
14. [Open Source, Contributions, and Community Content](14-open-source-and-community.md)
15. [Security Protections](security.md)
16. [Agent Office](14-agent-office.md)

For public skills and MCP repositories, see [GitHub Discovery](github-discovery.md).

## Get Started in Five Steps

Five minutes is a goal for getting to know the steps. Engine and model downloads take as long as their size and your connection require.

1. **Download and install.** Get the Windows installer (`.exe`, NSIS) from the project's [GitHub Releases](https://github.com/struklabs/harness-studio/releases) page and run it. The installer is not code-signed yet, so Windows SmartScreen may warn you the first time; the steps are in [Installation](01-installation.md). Signed, Apple-notarized DMGs are available for Apple Silicon and Intel (macOS 13.3+); Linux `.deb` and AppImage packages are previews.
2. **Finish the Setup window.** On **Welcome**, pick your interface level (**Simple** is recommended for a first run). On **Choose**, keep the preselected engine and first model, or tick more, then press **Install**. **Done** takes you to the **Get Started** checklist.
3. **Download a model.** In **Find a Model** (**Model Hub** in Advanced), search for a chat model and open **Files and quants**. Read the **Hardware fit** column and press **Download** on one file. Wait for **Completed** in the **Download Queue**.
4. **Start the server.** In **Dashboard**, pick the model you downloaded (and a **Model definition**, if you have one). Check the memory estimate and press **Start Server**. Wait for **Server active**.
5. **Chat.** In **Chat and Media** (**Playground** in Advanced), open **Chat**, check the model, type "Write me a three-item plan for today." into **Your message**, and press **Send**.

The **Get Started** list on the **Dashboard** repeats these steps and adds the first-agent and remote-server setups; see [Setup Scenarios](11-setup-scenarios.md).

If the model does not load, go to the [memory recovery steps](03-dashboard-and-servers.md#when-memory-runs-out-oom) and the [troubleshooting chapter](10-troubleshooting.md).

## Key Concepts

| Term | Meaning |
| --- | --- |
| Model | The file that carries what an AI has learned. Most models here are GGUF files, but Harness Studio also runs other formats (ONNX, Whisper, diffusion models, and so on) through optional engines. |
| Quant | A way of storing a model's numbers more compactly so the file takes less space. A smaller quant (Q4) uses less memory at a small cost in quality. |
| Runtime (engine) | The program that actually runs the model. For chat it is llama.cpp. Harness Studio downloads and updates it for you. Optional engines add speech, image generation, ONNX, and more. |
| Server | A running process that has a model loaded and waits for requests. You start it from the Dashboard. |
| Gateway | An optional layer that serves several models from one address and loads them on demand. It is off by default. |
| Context | How much text (in tokens) the model can keep in mind at once. Larger context needs more memory. |
| Alias | The model name other programs use instead of a file name. You set it in a model definition. |
| Interface level | **Simple** shows the core workflow; **Advanced** shows every technical setting. Switching never changes a saved value. |

## Simple and Advanced Interface Levels

Harness Studio has one interface with two levels of detail. **Simple** shows what you need to find a model, download it, chat, and connect an app. **Advanced** adds the technical pages: the **Servers** page, **Create**, **Performance**, **Analytics**, most Dashboard and Playground settings pages, and most Settings sections.

You can switch at any time with the **Simple / Advanced** switch under the main navigation, with **Interface Level** in **Settings > Appearance & Startup**, or from the command palette (**Switch to Advanced Level**). Switching only changes what is visible; it does not change or reset any value. If a link, an error message, or the command palette opens an Advanced page while you are in Simple, the page opens normally with a short notice at the top ("This page is at the Advanced level.").

This guide marks Advanced-level pages where it matters. If you cannot find a page mentioned here, switch to **Advanced**.

Simple navigation names are **Find a Model** (Advanced: **Model Hub**), **Chat and Media** (**Playground**), and **Connect an App** (**Connections**). Dashboard offers **Overview**, **Choose a Model**, and **Conversation Length**; only context size is exposed on the last page. Use **Advanced Settings** to reach technical controls. In a narrow window the floating section menu stays open after a selection; close it with **Collapse** or Escape. This differs from the main navigation drawer, which closes after navigation.

## How This Guide Writes Paths

Bold text is a button, field, or page name in the app. Settings paths follow the sidebar order. For example, **Settings > Storage > Hugging Face Account** means: open **Settings**, then choose **Hugging Face Account** under the **Storage** heading in the list on the left.

The guide uses the English interface. The app also runs in Turkish; change the language in **Settings > Appearance & Startup**.

## More Detailed References

[Features](../../en/FEATURES.md), [CLI](../../en/CLI.md), [MCP](../../en/MCP.md), [Integrations](../../en/INTEGRATIONS.md), [Security and Privacy](../../en/SECURITY_MODEL.md), and [Tools](../../en/TOOLS.md).
