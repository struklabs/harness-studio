// Server-sent events from text chunks that split anywhere (mid-line, mid-event).

export interface SseEvent {
  event: string | null;
  data: string;
}

export class SseParser {
  private buffer = '';

  /** The events completed by `chunk`. */
  push(chunk: string): SseEvent[] {
    this.buffer += chunk.replace(/\r\n?/g, '\n');
    const out: SseEvent[] = [];
    let end: number;
    while ((end = this.buffer.indexOf('\n\n')) >= 0) {
      const block = this.buffer.slice(0, end);
      this.buffer = this.buffer.slice(end + 2);
      const parsed = parseBlock(block);
      if (parsed) out.push(parsed);
    }
    return out;
  }

  /** What is left when the stream ends without a final blank line. */
  flush(): SseEvent[] {
    const rest = this.buffer;
    this.buffer = '';
    const parsed = rest.trim() ? parseBlock(rest) : null;
    return parsed ? [parsed] : [];
  }
}

function parseBlock(block: string): SseEvent | null {
  let event: string | null = null;
  const data: string[] = [];
  for (const line of block.split('\n')) {
    if (!line || line.startsWith(':')) continue;
    const colon = line.indexOf(':');
    const field = colon < 0 ? line : line.slice(0, colon);
    let value = colon < 0 ? '' : line.slice(colon + 1);
    if (value.startsWith(' ')) value = value.slice(1);
    if (field === 'event') event = value;
    else if (field === 'data') data.push(value);
  }
  return data.length || event ? { event, data: data.join('\n') } : null;
}
