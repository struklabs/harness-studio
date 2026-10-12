// Pure helpers for the log console: level classification, query matching,
// filtering and safe highlight splitting (no {@html}: text is split into parts
// and the template wraps matching parts in <mark>).

export type LogLevel = 'error' | 'warn' | null;
export type LevelFilter = 'all' | 'warn' | 'error';

/**
 * Heuristic level markers (kept deliberately simple):
 * - llama.cpp's common logger prints a one-letter level surrounded by spaces
 *   (`... E load_model: ...`, `... W ...`), matched case-sensitively;
 * - free-text words `error` / `failed` / `warn(ing)` anywhere, case-insensitively;
 * - Harness Studio's own error prefix `[Harness Studio Error]`.
 * Errors win over warnings.
 */
const ERROR_LETTER = /(?:^|\s)E\s/;
const ERROR_WORDS = /error|failed|\[Harness Studio Error\]/i;
const WARN_LETTER = /(?:^|\s)W\s/;
const WARN_WORDS = /warn/i;

export function classifyLevel(text: string): LogLevel {
  if (ERROR_LETTER.test(text) || ERROR_WORDS.test(text)) return 'error';
  if (WARN_LETTER.test(text) || WARN_WORDS.test(text)) return 'warn';
  return null;
}

/** 'warn' keeps warnings and errors ("warnings and above"); 'error' keeps only errors. */
export function passesLevel(level: LogLevel, filter: LevelFilter): boolean {
  if (filter === 'all') return true;
  if (filter === 'error') return level === 'error';
  return level === 'error' || level === 'warn';
}

export interface MatchOptions {
  regex: boolean;
  caseSensitive: boolean;
}

export type Matcher = { re: RegExp | null; error: string | null };

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/** Builds a global RegExp for `query` (null when the query is empty or invalid). */
export function buildMatcher(query: string, opts: MatchOptions): Matcher {
  if (!query) return { re: null, error: null };
  const flags = opts.caseSensitive ? 'g' : 'gi';
  try {
    return { re: new RegExp(opts.regex ? query : escapeRegExp(query), flags), error: null };
  } catch (e) {
    return { re: null, error: e instanceof Error ? e.message : String(e) };
  }
}

export function lineMatches(text: string, re: RegExp): boolean {
  re.lastIndex = 0;
  const hit = re.test(text);
  re.lastIndex = 0;
  return hit;
}

/** One log line; `server` says which server wrote it (live view only). */
/** `at`: when the backend wrote the line (ms since the epoch); orders lines of several servers. */
export type LogEntry = { seq: number; text: string; server?: string; at?: number };

export type TextPart = { text: string; match: boolean };

/** Splits `text` into alternating plain/matching parts. Zero-length matches are skipped. */
export function splitHighlight(text: string, re: RegExp | null): TextPart[] {
  if (!re || !text) return [{ text, match: false }];
  const parts: TextPart[] = [];
  let last = 0;
  re.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    if (m[0].length === 0) {
      re.lastIndex++; // avoid an infinite loop on patterns like `a*`
      if (re.lastIndex > text.length) break;
      continue;
    }
    if (m.index > last) parts.push({ text: text.slice(last, m.index), match: false });
    parts.push({ text: m[0], match: true });
    last = m.index + m[0].length;
  }
  re.lastIndex = 0;
  if (last < text.length) parts.push({ text: text.slice(last), match: false });
  return parts.length ? parts : [{ text, match: false }];
}

export interface LogRow {
  seq: number;
  text: string;
  /** Server the line came from (live view). */
  server?: string;
  level: LogLevel;
  /** True when a query is active and this line matches it. */
  hit: boolean;
}

export interface FilterOptions {
  re: RegExp | null;
  level: LevelFilter;
  /** When true (and a query is active) only matching lines are kept; otherwise matches are just highlighted. */
  onlyMatching: boolean;
}

/** Applies the level filter and the query to `entries`. */
export function filterLines(entries: LogEntry[], opts: FilterOptions): LogRow[] {
  const out: LogRow[] = [];
  for (const e of entries) {
    const level = classifyLevel(e.text);
    if (!passesLevel(level, opts.level)) continue;
    const hit = opts.re ? lineMatches(e.text, opts.re) : false;
    if (opts.re && opts.onlyMatching && !hit) continue;
    out.push({ seq: e.seq, text: e.text, server: e.server, level, hit });
  }
  return out;
}

/** Splits a saved log file into numbered entries (1-based, like the backend search). */
export function linesFromText(text: string): { seq: number; text: string }[] {
  const lines = text.split(/\r?\n/);
  if (lines.length && lines[lines.length - 1] === '') lines.pop();
  return lines.map((t, i) => ({ seq: i + 1, text: t }));
}
