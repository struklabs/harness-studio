export interface ParsedSearchQuery {
  cleanedQuery: string;
  detectedQuant: string | null;
}

const QUANT_REGEX = /\b(Q[0-9]_[A-Za-z0-9_]+|IQ[0-9]_[A-Za-z0-9_]+|TQ[0-9]_[0-9]+|MXFP4(?:_MOE)?|BF16|FP16|F16|F32|Q[0-9]_K|Q[0-9]_0|Q[0-9]_1)\b/i;

export function parseSearchQuery(raw: string): ParsedSearchQuery {
  let text = raw.trim();
  if (/^[\w.-]+\/[\w.-]+$/.test(text)) return { cleanedQuery: text, detectedQuant: null };
  let detectedQuant: string | null = null;

  // 1. Check inside parentheses: e.g. (Q4_K_M) or [Q4_K_M]
  const parenMatch = text.match(/[\(\[\{]([^\)\]\}]+)[\)\]\}]/);
  if (parenMatch) {
    const inside = parenMatch[1].trim();
    const qMatch = inside.match(QUANT_REGEX);
    if (qMatch) {
      detectedQuant = qMatch[1].toUpperCase();
      text = text.replace(parenMatch[0], ' ');
    }
  }

  // 2. Check standalone quant in text: e.g. Q4_K_M
  if (!detectedQuant) {
    const qMatch = text.match(QUANT_REGEX);
    if (qMatch) {
      detectedQuant = qMatch[1].toUpperCase();
      text = text.replace(qMatch[0], ' ');
    }
  }

  // 3. Remove .gguf extension or redundant standalone 'gguf' (since filter=gguf is always sent to HF API)
  text = text.replace(/\.gguf\b/gi, ' ').replace(/\bgguf\b/gi, ' ');

  // 4. Collapse spaces and normalize
  const cleanedQuery = text.replace(/\s+/g, ' ').trim();

  return {
    cleanedQuery,
    detectedQuant,
  };
}
