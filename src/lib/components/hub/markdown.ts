import { Marked } from 'marked';
import type { DOMPurify } from 'dompurify';

const marked = new Marked({ gfm: true, breaks: false, async: false });

/** Inline images we keep: only self-contained raster data URIs (the CSP blocks remote images anyway). */
const SAFE_DATA_IMAGE = /^data:image\/(png|jpe?g|gif|webp);base64,/i;

const FORBID_TAGS = ['style', 'form', 'input', 'button', 'textarea', 'select', 'iframe', 'video', 'audio', 'source', 'picture'];
const FORBID_ATTR = ['style', 'id', 'name', 'srcset'];

/**
 * Renders a model README (markdown) to sanitized HTML.
 * - Everything goes through DOMPurify (HTML profile only: no SVG/MathML, scripts, handlers, styles or ids).
 * - Images are replaced by their alt text unless they are an inline raster data URI.
 * - Relative links are resolved against the repo on huggingface.co; only http(s) links keep
 *   an href (clicks are intercepted by the caller and opened in the external browser).
 *
 * `purify` is injected so the function can be tested with a jsdom window.
 */
export function renderReadme(markdown: string, repoId: string, purify: DOMPurify): string {
  const raw = marked.parse(markdown, { async: false });
  const fragment = purify.sanitize(raw, {
    RETURN_DOM_FRAGMENT: true,
    // HTML only: model cards never need inline SVG/MathML, which are the usual mXSS vectors.
    USE_PROFILES: { html: true },
    FORBID_TAGS,
    FORBID_ATTR,
  });
  const doc = fragment.ownerDocument;

  for (const img of Array.from(fragment.querySelectorAll('img'))) {
    const src = img.getAttribute('src') ?? '';
    if (SAFE_DATA_IMAGE.test(src)) continue;
    const alt = (img.getAttribute('alt') ?? '').trim();
    if (alt) {
      const span = doc.createElement('span');
      span.className = 'md-img-alt';
      span.textContent = `[${alt}]`;
      img.replaceWith(span);
    } else {
      img.remove();
    }
  }

  const base = `https://huggingface.co/${repoId}/blob/main/`;
  for (const a of Array.from(fragment.querySelectorAll('a'))) {
    const href = a.getAttribute('href');
    a.removeAttribute('target');
    if (!href || href.startsWith('#')) {
      a.removeAttribute('href');
      continue;
    }
    let url: URL | null = null;
    try {
      url = new URL(href, base);
    } catch {
      url = null;
    }
    if (url && (url.protocol === 'https:' || url.protocol === 'http:')) {
      a.setAttribute('href', url.href);
      a.setAttribute('rel', 'noopener noreferrer');
      a.setAttribute('title', url.href);
    } else {
      a.removeAttribute('href');
    }
  }

  const container = doc.createElement('div');
  container.appendChild(fragment);
  return container.innerHTML;
}

/** Returns the external http(s) URL a click inside rendered README HTML should open, if any. */
export function externalLinkFromEvent(e: MouseEvent): string | null {
  const target = e.target as Element | null;
  const a = target?.closest?.('a');
  if (!a) return null;
  const href = a.getAttribute('href');
  if (!href || !/^https?:\/\//i.test(href)) return null;
  return href;
}
