import { Marked } from 'marked';
import DOMPurify from 'dompurify';

// Model output: GitHub-flavoured markdown, single newlines kept as line breaks (models use them
// that way). Fenced code blocks are split out by MessageContent before this runs.
const marked = new Marked({ gfm: true, breaks: true, async: false });

const FORBID_TAGS = [
  'style',
  'form',
  'input',
  'button',
  'textarea',
  'select',
  'iframe',
  'video',
  'audio',
  'source',
  'picture',
  'img',
];
const FORBID_ATTR = ['style', 'id', 'name', 'srcset', 'class'];

/**
 * Renders chat markdown to sanitized HTML.
 * - DOMPurify with the HTML profile only (no SVG/MathML, scripts, handlers, styles, ids, classes).
 * - Images are dropped (the CSP blocks remote images; alt text is kept by marked as text).
 * - Only http(s)/mailto links keep an href; the caller opens them in the external browser.
 */
export function renderChatMarkdown(src: string): string {
  const raw = marked.parse(src, { async: false });
  const fragment = DOMPurify.sanitize(raw, {
    RETURN_DOM_FRAGMENT: true,
    USE_PROFILES: { html: true },
    FORBID_TAGS,
    FORBID_ATTR,
  });
  for (const a of Array.from(fragment.querySelectorAll('a'))) {
    const href = a.getAttribute('href') ?? '';
    a.removeAttribute('target');
    if (/^(https?:|mailto:)/i.test(href)) {
      a.setAttribute('rel', 'noopener noreferrer');
      a.setAttribute('title', href);
    } else {
      a.removeAttribute('href');
    }
  }
  const container = fragment.ownerDocument.createElement('div');
  container.appendChild(fragment);
  return container.innerHTML;
}
