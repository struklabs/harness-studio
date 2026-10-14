import { t } from '#lib/i18n.svelte.js';
import type { IntegrationId } from './integrations';

export interface SnippetContext {
  baseUrl: string;
  /** The real API key, or null when the server has none. */
  apiKey: string | null;
  modelId: string;
  contextSize: number;
}

export const PLACEHOLDER_KEY = 'harness-local';

/** `http://host:port` (no `/v1`): Anthropic-style clients add the path themselves. */
export function hostOf(baseUrl: string): string {
  return baseUrl.replace(/\/+$/, '').replace(/\/v1$/, '');
}

function out(ctx: number): number {
  return Math.min(8192, Math.max(1024, Math.floor((ctx || 32768) / 4)));
}

/** Copy-ready text for a tool or SDK. Reads t() so call it from a $derived. */
export function buildSnippet(id: IntegrationId, c: SnippetContext, fallback: string | null = null): string {
  const { baseUrl, modelId } = c;
  const host = hostOf(baseUrl);
  const clientKey = c.apiKey ?? PLACEHOLDER_KEY;
  const ctx = c.contextSize || 32768;
  switch (id) {
    case 'claude-code':
      return `// ~/.claude/settings.json (or: harness launch claude --model ${modelId})
{
  "env": {
    "ANTHROPIC_BASE_URL": "${host}",
    "ANTHROPIC_AUTH_TOKEN": "${clientKey}",
    "ANTHROPIC_MODEL": "${modelId}",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "${modelId}",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "${modelId}",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "${modelId}",
    "CLAUDE_CODE_SUBAGENT_MODEL": "${modelId}",
    "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "${ctx}",
    "CLAUDE_CODE_MAX_OUTPUT_TOKENS": "${out(ctx)}",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
    "CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS": "1"
  }
}`;
    case 'codex':
      return `# ~/.codex/config.toml
[model_providers.harness]
name = "Harness Studio"
base_url = "${baseUrl}"
wire_api = "responses"${c.apiKey ? `\nhttp_headers = { Authorization = "Bearer ${c.apiKey}" }` : ''}

# ~/.codex/harness.config.toml  ->  codex --profile harness
model_provider = "harness"
model = "${modelId}"
model_context_window = ${ctx}
web_search = "disabled"`;
    case 'opendesign':
      return t('conn.snippet_opendesign', { baseUrl, clientKey, modelId });
    case 'opencode':
    case 'kilo':
      return `{
  "$schema": "${id === 'kilo' ? 'https://app.kilo.ai/config.json' : 'https://opencode.ai/config.json'}",
  "provider": {
    "harness": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "Harness Studio",
      "options": { "baseURL": "${baseUrl}", "apiKey": "${clientKey}" },
      "models": {
        "${modelId}": { "tool_call": true, "limit": { "context": ${ctx}, "output": ${out(ctx)} } }
      }
    }
  },
  "model": "harness/${modelId}"
}`;
    case 'continue':
      return `models:
  - title: Harness Studio (llama.cpp)
    provider: openai
    model: ${modelId}
    apiBase: ${baseUrl}
    apiKey: ${clientKey}`;
    case 'vscode-copilot':
      return `// chatLanguageModels.json (Chat: Manage Language Models)
[
  {
    "name": "Harness Studio",
    "vendor": "customendpoint",
    "apiType": "chat-completions",${c.apiKey ? '\n    "apiKey": "${input:harnessStudioApiKey}",' : ''}
    "models": [
      {
        "id": "${modelId}",
        "name": "${modelId} (Harness Studio)",
        "url": "${baseUrl}/chat/completions",
        "toolCalling": true,
        "vision": false,
        "maxInputTokens": ${ctx - out(ctx)},
        "maxOutputTokens": ${out(ctx)}
      }
    ]
  }
]`;
    case 'zed':
      return `// Zed settings.json
"language_models": {
  "openai_compatible": {
    "Harness Studio": {
      "api_url": "${baseUrl}",
      "available_models": [
        {
          "name": "${modelId}",
          "display_name": "${modelId} (Harness Studio)",
          "max_tokens": ${ctx},
          "max_output_tokens": ${out(ctx)},
          "capabilities": { "tools": true, "images": false, "parallel_tool_calls": false, "prompt_cache_key": false }
        }
      ]
    }
  }
}`;
    case 'goose':
      return `// ~/.config/goose/custom_providers/harness.json (Windows: %APPDATA%\\Block\\goose\\config\\custom_providers)
{
  "name": "harness",
  "engine": "openai",
  "display_name": "Harness Studio",
  "base_url": "${baseUrl}/chat/completions",
  "models": [{ "name": "${modelId}", "context_limit": ${ctx} }],
  "supports_streaming": true,
  "requires_auth": ${c.apiKey ? 'true,\n  "api_key_env": "HARNESS_API_KEY"' : 'false'}
}
# or for one session: harness launch goose --model ${modelId}`;
    case 'librechat':
      return `# librechat.yaml (Docker: use host.docker.internal instead of 127.0.0.1)
endpoints:
  custom:
    - name: "Harness Studio"
      apiKey: "${clientKey}"
      baseURL: "${baseUrl}"
      models:
        default: ["${modelId}"]
        fetch: true
      titleConvo: true
      titleModel: "${modelId}"
      modelDisplayLabel: "Harness Studio"`;
    case 'aider':
      return `${t('conn.snippet_aider_comment')}
harness launch aider --model ${modelId}
# ${t('intg.snippet_aider_manual')}
aider --openai-api-base ${baseUrl} --openai-api-key ${clientKey} --model openai/${modelId}`;
    case 'curl':
      return `curl -X POST "${baseUrl}/chat/completions" \\
  -H "Content-Type: application/json" \\${c.apiKey ? `\n  -H "Authorization: Bearer ${c.apiKey}" \\` : ''}
  -d '{
    "model": "${modelId}",
    "messages": [{"role": "user", "content": "Merhaba, kendini kısaca tanıtır mısın?"}],
    "max_tokens": 256
  }'`;
    case 'openai-python':
      return `from openai import OpenAI

client = OpenAI(base_url="${baseUrl}", api_key="${clientKey}")

response = client.chat.completions.create(
    model="${modelId}",
    messages=[{"role": "user", "content": "llama.cpp ile yerel çıkarımın avantajları nelerdir?"}],
)
print(response.choices[0].message.content)`;
    case 'openai-js':
      return `import OpenAI from "openai";

const client = new OpenAI({ baseURL: "${baseUrl}", apiKey: "${clientKey}" });

const response = await client.chat.completions.create({
  model: "${modelId}",
  messages: [{ role: "user", content: "Hello!" }],
});
console.log(response.choices[0].message.content);`;
    case 'anthropic-python':
      return `from anthropic import Anthropic

# base_url without /v1: the SDK adds /v1/messages itself.
client = Anthropic(base_url="${host}", auth_token="${clientKey}")

message = client.messages.create(
    model="${modelId}",
    max_tokens=512,
    messages=[{"role": "user", "content": "Hello!"}],
)
print(message.content[0].text)`;
    case 'langchain':
      return `from langchain_openai import ChatOpenAI

llm = ChatOpenAI(base_url="${baseUrl}", api_key="${clientKey}", model="${modelId}")
print(llm.invoke("Hello!").content)

# Anthropic-style (base URL without /v1):
# from langchain_anthropic import ChatAnthropic
# llm = ChatAnthropic(base_url="${host}", api_key="${clientKey}", model="${modelId}")`;
    case 'llamaindex':
      return `from llama_index.llms.openai_like import OpenAILike

llm = OpenAILike(
    model="${modelId}",
    api_base="${baseUrl}",
    api_key="${clientKey}",
    is_chat_model=True,
    is_function_calling_model=True,
    context_window=${ctx},
)
print(llm.complete("Hello!"))`;
    case 'vercel-ai':
      return `import { createOpenAICompatible } from "@ai-sdk/openai-compatible";
import { generateText } from "ai";

const harness = createOpenAICompatible({
  name: "harness",
  baseURL: "${baseUrl}",
  apiKey: "${clientKey}",
});

const { text } = await generateText({ model: harness("${modelId}"), prompt: "Hello!" });
console.log(text);`;
    default:
      return fallback ?? `${baseUrl}
${modelId}`;
  }
}
