import type { ChatMessage } from './types';
import markers from '../../../../shared/text-tool-call-markers.json' with { type: 'json' };

/** Hold only a possible marker suffix until a complete call opening appears. */
export class TextCallStreamBuffer {
  private pending = '';
  private buffering = false;
  private atStart = true;
  private visibleIntro = '';

  push(chunk: string): string {
    this.pending += chunk;
    if (this.buffering) return '';
    const starts = [...markers.strong, markers.fence];
    const possibleJson = (text: string) => text.startsWith('{') ||
      (text.startsWith('[') && (!text.slice(1).trimStart() || /^[{[]/.test(text.slice(1).trimStart())));
    let boundary = this.pending.length;
    for (const marker of starts) {
      const index = this.pending.indexOf(marker);
      if (index >= 0) boundary = Math.min(boundary, index);
    }
    for (let index = 0; index < this.pending.length; index++) {
      if (!markers.initialJson.includes(this.pending[index])) continue;
      const initial = (this.visibleIntro + this.pending.slice(0, index)).trimStart();
      const prefix = markers.weakPrefixes.find((p) => initial.startsWith(p));
      const intro = (prefix ? initial.slice(prefix.length) : initial).trim();
      if (possibleJson(this.pending.slice(index)) && !intro.startsWith('>') && !/[\r\n]/.test(intro) && [...intro].length <= markers.introMaxChars) {
        boundary = Math.min(boundary, index);
        break;
      }
    }
    if (this.atStart) {
      const initial = this.pending.trimStart();
      const prefix = markers.weakPrefixes.find((p) => initial.startsWith(p));
      const candidate = (prefix ? initial.slice(prefix.length) : initial).trimStart();
      if ((markers.initialJson.includes(candidate[0]) && possibleJson(candidate)) || starts.some((m) => candidate.startsWith(m))) boundary = 0;
      else if (!candidate || starts.some((m) => m.startsWith(candidate))) return '';
    }
    if (boundary < this.pending.length) {
      this.buffering = this.pending.slice(boundary).includes('{') || starts.some((m) => this.pending.slice(boundary).includes(m));
    } else {
      for (const marker of starts) {
        for (let length = 1; length < marker.length; length++) {
          if (this.pending.endsWith(marker.slice(0, length))) boundary = Math.min(boundary, this.pending.length - length);
        }
      }
    }
    const visible = this.pending.slice(0, boundary);
    this.pending = this.pending.slice(boundary);
    if (visible.trim()) this.atStart = false;
    this.visibleIntro += visible;
    return visible;
  }

  drain(): string {
    const pending = this.pending;
    this.pending = '';
    return pending;
  }
}

export interface TextCallRecovery {
  content: string;
  tool_calls: { id: string; name: string; arguments: string }[];
}

export type TextCallParser = (content: string, names: string[]) => Promise<TextCallRecovery>;

/** The parser lives in harness-core; both Playground views use this adapter after a full turn. */
export async function salvageToolCalls(
  msg: ChatMessage,
  names: string[],
  signal: AbortSignal,
  parse: TextCallParser,
  content = msg.content
): Promise<void> {
  if (!names.length || msg.toolCalls?.length || signal.aborted || msg.stopped || msg.error ||
      msg.finishReason === 'length' || msg.finishReason === 'content_filter' || !content.trim()) {
    msg.content = content;
    return;
  }
  let recovery: TextCallRecovery;
  try {
    recovery = await parse(content, names);
  } catch (error) {
    msg.content = content;
    throw error;
  }
  // Stop can arrive during IPC. Never turn a cancelled response into a runnable call.
  if (signal.aborted || !recovery.tool_calls.length) {
    msg.content = content;
    return;
  }
  msg.content = recovery.content;
  msg.toolCalls = recovery.tool_calls.map((call) => ({ ...call, status: 'pending', recoveredFromText: true }));
  msg.finishReason = 'tool_calls';
}
