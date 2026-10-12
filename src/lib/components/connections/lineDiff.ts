// Line diff for the "review before writing" dialog. Config files are small (a few hundred
// lines), so a plain LCS table is fast enough and keeps us free of a diff dependency.

export type DiffLine =
  | { kind: 'same' | 'add' | 'del'; text: string }
  /** A run of unchanged lines folded away. */
  | { kind: 'skip'; count: number };

/** Unified line diff of `before` → `after`, unchanged runs folded to `context` lines. */
export function lineDiff(before: string | null, after: string, context = 3): DiffLine[] {
  const a = before === null ? [] : splitLines(before);
  const b = splitLines(after);
  const n = a.length;
  const m = b.length;
  // lcs[i][j] = LCS length of a[i..] and b[j..]
  const lcs: Uint32Array[] = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
    }
  }
  const full: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      full.push({ kind: 'same', text: a[i] });
      i++;
      j++;
    } else if (lcs[i + 1][j] >= lcs[i][j + 1]) {
      full.push({ kind: 'del', text: a[i++] });
    } else {
      full.push({ kind: 'add', text: b[j++] });
    }
  }
  while (i < n) full.push({ kind: 'del', text: a[i++] });
  while (j < m) full.push({ kind: 'add', text: b[j++] });
  return fold(full, context);
}

export function diffStats(lines: DiffLine[]): { added: number; removed: number } {
  let added = 0;
  let removed = 0;
  for (const l of lines) {
    if (l.kind === 'add') added++;
    else if (l.kind === 'del') removed++;
  }
  return { added, removed };
}

function splitLines(text: string): string[] {
  const lines = text.replace(/\r\n/g, '\n').split('\n');
  if (lines.length > 1 && lines[lines.length - 1] === '') lines.pop();
  return lines;
}

/** Keeps `context` unchanged lines around each change and folds the rest. */
function fold(lines: DiffLine[], context: number): DiffLine[] {
  const keep = new Array<boolean>(lines.length).fill(false);
  lines.forEach((l, idx) => {
    if (l.kind === 'same') return;
    for (let k = Math.max(0, idx - context); k <= Math.min(lines.length - 1, idx + context); k++) keep[k] = true;
  });
  const out: DiffLine[] = [];
  let skipped = 0;
  lines.forEach((l, idx) => {
    if (keep[idx]) {
      if (skipped) out.push({ kind: 'skip', count: skipped });
      skipped = 0;
      out.push(l);
    } else {
      skipped++;
    }
  });
  if (skipped) out.push({ kind: 'skip', count: skipped });
  return out;
}
