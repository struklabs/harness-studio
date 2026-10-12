// Incremental Server-Sent Events parser for OpenAI-compatible streaming responses.
// Pure (no Svelte / DOM dependencies) so it can be exercised from a plain Node script.

export interface SseEvent {
  /** `event:` field value; "message" by default. llama.cpp's non-standard `error:` lines use "error". */
  event: string;
  /** Joined `data:` payload (multiple data lines are joined with "\n"). */
  data: string;
}

/**
 * Feed decoded text chunks with `push()`; complete events are returned as soon as their
 * terminating blank line arrives. Lines split across chunks are buffered until complete.
 * Call `flush()` once the stream ends to emit a trailing event lacking its blank line.
 */
export class SseParser {
  private buffer = '';
  private dataLines: string[] = [];
  private eventName = '';

  push(chunk: string): SseEvent[] {
    this.buffer += chunk;
    const out: SseEvent[] = [];
    let nl: number;
    while ((nl = this.buffer.indexOf('\n')) !== -1) {
      let line = this.buffer.slice(0, nl);
      this.buffer = this.buffer.slice(nl + 1);
      if (line.endsWith('\r')) line = line.slice(0, -1);
      this.handleLine(line, out);
    }
    return out;
  }

  flush(): SseEvent[] {
    const out: SseEvent[] = [];
    if (this.buffer) {
      const line = this.buffer.endsWith('\r') ? this.buffer.slice(0, -1) : this.buffer;
      this.buffer = '';
      this.handleLine(line, out);
    }
    this.dispatch(out);
    return out;
  }

  private handleLine(line: string, out: SseEvent[]) {
    if (line === '') {
      this.dispatch(out);
      return;
    }
    if (line.startsWith(':')) return; // comment / keep-alive
    const colon = line.indexOf(':');
    const field = colon === -1 ? line : line.slice(0, colon);
    let value = colon === -1 ? '' : line.slice(colon + 1);
    if (value.startsWith(' ')) value = value.slice(1);

    if (field === 'data') this.dataLines.push(value);
    else if (field === 'event') this.eventName = value;
    else if (field === 'error') out.push({ event: 'error', data: value });
  }

  private dispatch(out: SseEvent[]) {
    if (this.dataLines.length > 0) {
      out.push({ event: this.eventName || 'message', data: this.dataLines.join('\n') });
    }
    this.dataLines = [];
    this.eventName = '';
  }
}

/** Extracts a readable message from an error response body (OpenAI-style JSON or plain text). */
export function describeErrorBody(body: string): string {
  const trimmed = body.trim();
  if (!trimmed) return '';
  try {
    const parsed = JSON.parse(trimmed);
    const msg = parsed?.error?.message ?? parsed?.message ?? (typeof parsed?.error === 'string' ? parsed.error : null);
    if (typeof msg === 'string' && msg) return msg;
  } catch {
    // not JSON
  }
  return trimmed.length > 800 ? `${trimmed.slice(0, 800)}…` : trimmed;
}
