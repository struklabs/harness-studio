/** Reject answer bodies rather than turning their first line into a title. */
export function cleanGeneratedTitle(raw: string): string {
  const withoutThinking = raw.replace(/<think>[\s\S]*?<\/think>/gi, '');
  if (/<\/?think>/i.test(withoutThinking)) return '';
  let text = withoutThinking.trim().replace(/^(title|başlık)\s*:\s*/i, '');
  if (!text || text.split(/\n/).filter(line => line.trim()).length > 1) return '';
  if (/\d{1,2}:\d{2}/.test(text) || /[.!?]\s+\S/.test(text)) return '';
  text = text.replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
    .replace(/[*_`#~]/g, '').replace(/^["'“”‘’\s]+|["'“”‘’\s]+$/g, '').trim();
  if (!text || text.length > 60 || text.split(/\s+/).length > 6 || /[.!?]$/.test(text)) return '';
  return text;
}
