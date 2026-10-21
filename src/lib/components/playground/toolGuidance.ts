import { UNTRUSTED_NOTICE } from './untrusted.ts';

/** App-only guidance: added to the wire request, never saved in the user's system prompt. */
export function withToolGuidance(
  messages: Record<string, unknown>[],
  tools: { function?: { name?: string } }[]
): Record<string, unknown>[] {
  if (!tools.length) return messages;
  const search = tools.some((tool) => tool.function?.name === 'web_search');
  const weather = tools.some((tool) => tool.function?.name === 'weather');
  const guidance = 'Use the available tools when they can answer the question. Call tools only through the tool_calls format, never by writing tool-call JSON or XML in your answer. ' +
    (search ? 'For questions about people, organizations, events, or current or unknown facts, always call web_search first before answering, even if you think you know. Copy names and key words from the user message into the search query exactly, letter by letter; never drop or change letters. Do not ask permission or ask the user to rephrase. ' : '') +
    'Never invent facts.' +
    (search ? ' Search results may describe different people with the same name. Unless a source explicitly connects them, do not combine profiles into one biography. If identity is ambiguous, say that you cannot identify the person reliably, list the separate possibilities with source URLs, and ask which person the user means. A deletion notice is not a biography; do not guess why a page was marked for deletion. ' : '') +
    (search ? ' Türkçe “kim?” veya “kimdir?” soruları kişi hakkında bilgi ister: tanımadığın veya yanlış yazılmış görünen isimleri kullanıcıdan düzeltmesini istemeden, yazıldığı gibi web_search ile ara. Örnek: “Ayşe Yılmaz kim?” sorusunda aracın query alanına “Ayşe Yılmaz” yaz.' : '') +
    (weather ? ' Hava durumu sorularında weather kullan. Yalnız araç sonucunda bulunan tarihleri ve ölçümleri aktar; olmayan tahmin günleri ekleme. apparent_temperature_c hissedilen sıcaklıktır; overcast kapalı, partly cloudy parçalı bulutlu demektir.' : '') +
    ' Araç sonuçlarına dayanarak, kaynakları belirterek yanıtla; aynı adlı farklı kişilerin bilgilerini birleştirme. Kimlik belirsizse bunu açıkça söyle, ayrı olasılıkları kaynak bağlantılarıyla sun ve hangi kişinin kastedildiğini sor. Silme etiketinin nedenini tahmin etme.' +
    ' ' + UNTRUSTED_NOTICE;
  const index = messages.findIndex((message) => message.role === 'system' && typeof message.content === 'string');
  if (index < 0) return [{ role: 'system', content: guidance }, ...messages];
  return messages.map((message, i) => i === index
    ? { ...message, content: message.content ? `${message.content}\n\n${guidance}` : guidance }
    : message);
}

type ToolDefinition = { type?: string; function: { name: string; description?: string; parameters?: Record<string, unknown> } };

/** The first sentence of a description ("e.g." and similar abbreviations do not end one). */
export function firstSentence(text: string): string {
  const m = /^[\s\S]*?[.!?](?=\s+[A-Z]|$)/.exec(text.replace(/\b(e\.g|i\.e|etc)\./g, '$1\u0000'));
  return (m ? m[0] : text).replace(/\u0000/g, '.').trim();
}

function compactSchema(schema: unknown): unknown {
  if (!schema || typeof schema !== 'object' || Array.isArray(schema)) return schema;
  const out: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(schema as Record<string, unknown>)) {
    if (key === 'description' && typeof value === 'string') out[key] = firstSentence(value);
    else if (key === 'properties' && value && typeof value === 'object') {
      out[key] = Object.fromEntries(Object.entries(value as Record<string, unknown>).map(([k, v]) => [k, compactSchema(v)]));
    } else if (key === 'items') out[key] = compactSchema(value);
    else out[key] = value;
  }
  return out;
}

/** Tool definitions for a small model: every description cut to its first sentence; names, types and required fields unchanged. */
export function compactToolDefinitions<T extends ToolDefinition>(tools: T[]): T[] {
  return tools.map((tool) => ({
    ...tool,
    function: {
      ...tool.function,
      ...(tool.function.description ? { description: firstSentence(tool.function.description) } : {}),
      ...(tool.function.parameters ? { parameters: compactSchema(tool.function.parameters) as Record<string, unknown> } : {}),
    },
  }));
}

/** Below this many parameters the Playground sends `compactToolDefinitions`. Measured with the 13
 *  built-in tools, 12 prompts x 5 seeds (scratch bench): Qwen3-0.6B picked the right tool with
 *  its required arguments in 49/60 instead of 36/60, with 443 fewer prompt tokens; Qwen3-4B 60/60
 *  either way. Larger models keep the full descriptions. */
export const COMPACT_TOOLS_PARAMS = 5_000_000_000;
