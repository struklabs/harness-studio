// Mermaid diagrams in chat answers. The library (~several hundred KB, more per diagram type) is
// loaded with a dynamic import the first time a finished ```mermaid block is shown, so it never
// lands in the initial bundle. Output is re-sanitized with DOMPurify (SVG profile only, no
// foreignObject/HTML labels, no external references) before it reaches {@html}.
import DOMPurify from 'dompurify';
import { currentScheme } from '$lib/theme';

type MermaidApi = (typeof import('mermaid'))['default'];

let loader: Promise<MermaidApi> | null = null;

function loadMermaid(): Promise<MermaidApi> {
  loader ??= import('mermaid')
    .then((m) => m.default)
    .catch((e: unknown) => {
      // A failed chunk load may succeed later (e.g. after an update); don't cache the failure.
      loader = null;
      throw e;
    });
  return loader;
}

/** Normalizes any CSS colour (hex, rgb, oklch, …) to `#rrggbb`, which Mermaid's colour math accepts. */
let probe: CanvasRenderingContext2D | null = null;
function toHex(css: string, fallback: string): string {
  const value = css.trim();
  if (!value) return fallback;
  if (/^#[0-9a-f]{6}$/i.test(value)) return value;
  probe ??= document.createElement('canvas').getContext('2d', { willReadFrequently: true });
  if (!probe) return fallback;
  probe.clearRect(0, 0, 1, 1);
  probe.fillStyle = fallback;
  probe.fillStyle = value;
  probe.fillRect(0, 0, 1, 1);
  const [r, g, b] = probe.getImageData(0, 0, 1, 1).data;
  return `#${[r, g, b].map((n) => n.toString(16).padStart(2, '0')).join('')}`;
}

/** Fill behind an exported PNG: the card colour the diagram is drawn on, in the active theme. */
export function diagramBackground(): string {
  const dark = currentScheme() === 'dark';
  return toHex(getComputedStyle(document.documentElement).getPropertyValue('--card'), dark ? '#15181d' : '#f6f7f9');
}

/** Mermaid `base` theme variables from the app's semantic tokens (whatever theme is active). */
function themeVariables(): Record<string, string | boolean> {
  const cs = getComputedStyle(document.documentElement);
  const token = (name: string, fallback: string) => toHex(cs.getPropertyValue(`--${name}`), fallback);
  const dark = currentScheme() === 'dark';
  const fg = token('foreground', dark ? '#e6e8eb' : '#16191e');
  const bg = token('card', dark ? '#15181d' : '#f6f7f9');
  const muted = token('muted', dark ? '#1b1f25' : '#e3e6ea');
  const mutedFg = token('muted-foreground', dark ? '#8a919c' : '#5b636e');
  const border = token('border', dark ? '#2a2f37' : '#d3d8de');
  const primary = token('primary', dark ? '#e27c3a' : '#934716');
  const secondary = token('secondary', muted);
  const accent = token('accent', muted);
  const charts = [1, 2, 3, 4, 5].map((i) => token(`chart-${i}`, primary));
  const vars: Record<string, string | boolean> = {
    darkMode: dark,
    background: bg,
    fontFamily: cs.getPropertyValue('--font-sans').trim() || 'sans-serif',
    fontSize: '14px',
    textColor: fg,
    primaryColor: secondary,
    primaryTextColor: fg,
    primaryBorderColor: primary,
    secondaryColor: accent,
    secondaryTextColor: fg,
    secondaryBorderColor: border,
    tertiaryColor: muted,
    tertiaryTextColor: fg,
    tertiaryBorderColor: border,
    mainBkg: secondary,
    nodeBorder: primary,
    nodeTextColor: fg,
    lineColor: mutedFg,
    clusterBkg: bg,
    clusterBorder: border,
    titleColor: fg,
    edgeLabelBackground: bg,
    noteBkgColor: muted,
    noteTextColor: fg,
    noteBorderColor: border,
    actorBkg: secondary,
    actorBorder: primary,
    actorTextColor: fg,
    actorLineColor: mutedFg,
    signalColor: fg,
    signalTextColor: fg,
    labelBoxBkgColor: secondary,
    labelBoxBorderColor: border,
    labelTextColor: fg,
    loopTextColor: fg,
    activationBkgColor: muted,
    activationBorderColor: border,
    sequenceNumberColor: bg,
    pieTitleTextColor: fg,
    pieSectionTextColor: bg,
    pieLegendTextColor: fg,
    pieStrokeColor: bg,
    pieOuterStrokeColor: border,
    git0: charts[0],
    git1: charts[1],
    git2: charts[2],
    git3: charts[3],
    git4: charts[4],
  };
  for (let i = 0; i < 12; i++) {
    vars[`pie${i + 1}`] = charts[i % charts.length];
    vars[`cScale${i}`] = i % 2 ? secondary : accent;
  }
  return vars;
}

/** Keys a `%%{init}%%` directive inside the diagram may not change (Mermaid's defaults + ours). */
const SECURE_KEYS = [
  'secure',
  'securityLevel',
  'startOnLoad',
  'maxTextSize',
  'maxEdges',
  'suppressErrorRendering',
  'htmlLabels',
  'theme',
  'themeVariables',
  'themeCSS',
  'darkMode',
  'fontFamily',
  'altFontFamily',
  'deterministicIds',
];

/** Drops `@import` and every `url(...)` that is not a same-document `#fragment`. */
function cleanCss(css: string): string {
  return css.replace(/@import[^;]*;?/gi, '').replace(/url\(\s*(['"]?)(?!#)[^)]*\)/gi, 'none');
}

function sanitizeSvg(svg: string): string {
  const fragment = DOMPurify.sanitize(svg, {
    RETURN_DOM_FRAGMENT: true,
    USE_PROFILES: { svg: true, svgFilters: true },
    ADD_TAGS: ['style'],
    FORBID_TAGS: ['foreignObject', 'script', 'image', 'a', 'animate', 'set', 'animateTransform', 'animateMotion'],
  });
  const root = fragment.firstElementChild;
  if (!root || root.tagName.toLowerCase() !== 'svg') throw new Error('mermaid: no svg output');
  for (const el of Array.from(fragment.querySelectorAll('*'))) {
    for (const attr of ['href', 'xlink:href']) {
      const v = el.getAttribute(attr);
      if (v !== null && !v.startsWith('#')) el.removeAttribute(attr);
    }
    const style = el.getAttribute('style');
    if (style) el.setAttribute('style', cleanCss(style));
    if (el.tagName.toLowerCase() === 'style') el.textContent = cleanCss(el.textContent ?? '');
  }
  root.setAttribute('role', 'img');
  const holder = document.createElement('div');
  holder.appendChild(fragment);
  return holder.innerHTML;
}

export type MermaidResult = { ok: true; svg: string } | { ok: false; error: string };

let seq = 0;
/** Mermaid's config is global; renders run one at a time so each uses its own theme snapshot. */
let chain: Promise<unknown> = Promise.resolve();

/** Renders a finished diagram with the current app theme. Never throws. */
export function renderMermaid(code: string): Promise<MermaidResult> {
  const run = async (): Promise<MermaidResult> => {
    const id = `hs-mermaid-${++seq}`;
    try {
      const mermaid = await loadMermaid();
      mermaid.initialize({
        startOnLoad: false,
        securityLevel: 'strict',
        htmlLabels: false,
        suppressErrorRendering: true,
        deterministicIds: false,
        theme: 'base',
        themeVariables: themeVariables(),
        secure: SECURE_KEYS,
        maxTextSize: 50_000,
        maxEdges: 500,
      });
      const { svg } = await mermaid.render(id, code);
      return { ok: true, svg: sanitizeSvg(svg) };
    } catch (e) {
      return { ok: false, error: errorLine(e) };
    } finally {
      // Mermaid leaves its measuring container behind when rendering fails.
      document.getElementById(`d${id}`)?.remove();
      document.getElementById(id)?.remove();
    }
  };
  const next = chain.then(run, run);
  chain = next;
  return next;
}

/**
 * Short form of a Mermaid error. Parser messages are "Parse error on line N:", the source line,
 * a `----^` marker and "Expecting …"; keep the first and the last line.
 */
function errorLine(e: unknown): string {
  const raw = e instanceof Error ? e.message : typeof e === 'string' ? e : String(e);
  const lines = raw
    .split('\n')
    .map((l) => l.trim())
    .filter((l) => l && !/^-*\^$/.test(l));
  const first = lines[0] ?? raw;
  const last = lines.length > 1 ? lines[lines.length - 1] : '';
  const text = last && /^Parse error/i.test(first) ? `${first} ${last}` : first;
  return text.slice(0, 240);
}
