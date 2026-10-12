import { bestSettingsField, normalizeSearch } from '$lib/search';
import { inAllLanguages, t, type TKey } from '$lib/i18n.svelte';

/** Search only rendered field text; translated aliases keep cross-language queries working. */
export function revealSettingsField(root: HTMLElement, query: string, labels: string[], keys: TKey[]): () => void {
  const texts = Array.from(root.querySelectorAll<HTMLElement>('label, h2, h3, h4, p, span'))
    .filter((node) => node.getClientRects().length > 0 && !node.closest('header'));
  const fields = new Map<HTMLElement, string[]>();
  const translations = keys.map((key) => ({ text: normalizeSearch(t(key)), aliases: inAllLanguages(key) }));
  for (const node of texts) {
    // Leaf text avoids treating a theme grid or a whole group as a single field.
    if (node.querySelector('label, h2, h3, h4, p, span')) continue;
    const text = node.textContent?.trim();
    if (!text) continue;
    // The whole setting row when there is one (a label alone would frame only the title).
    const target = node.closest<HTMLElement>('[data-setting-field]') ?? node.closest<HTMLElement>('label') ?? node;
    const aliases = translations.filter((entry) => normalizeSearch(text).includes(entry.text))
      .flatMap((entry) => entry.aliases);
    fields.set(target, [...(fields.get(target) ?? []), text, ...aliases]);
  }
  const target = bestSettingsField(query, labels, Array.from(fields, ([value, texts]) => ({ value, texts })));
  if (!target) return () => {};

  const label = target instanceof HTMLLabelElement ? target : target.querySelector('label');
  const control = label?.control;
  const focusTarget = control instanceof HTMLElement && !control.matches(':disabled, [aria-disabled="true"]')
    ? control : label ?? target;
  const oldTabindex = focusTarget.getAttribute('tabindex');
  if (focusTarget === label || focusTarget === target) focusTarget.setAttribute('tabindex', '-1');
  focusTarget.focus({ preventScroll: true });
  target.scrollIntoView({ block: 'center', behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth' });
  target.setAttribute('data-setting-highlight', '');
  const cleanup = () => {
    target.removeAttribute('data-setting-highlight');
    if (oldTabindex === null) focusTarget.removeAttribute('tabindex');
    else focusTarget.setAttribute('tabindex', oldTabindex);
  };
  const timer = window.setTimeout(cleanup, 1500);
  return () => { window.clearTimeout(timer); cleanup(); };
}
