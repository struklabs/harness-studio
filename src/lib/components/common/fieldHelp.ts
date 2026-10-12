let sequence = 0;
/** Preserve existing validation ids and connect shared help to conditional/loop controls. */
export function describeFields(node: HTMLElement, descriptionId?: string) {
  const selector = 'input,textarea,select,[role="switch"],[role="checkbox"],[role="slider"],[role="combobox"],[role="radio"],[data-slot="toggle-group-item"]';
  let id = descriptionId;
  const attached = new Map<Element, string[]>();
  function clear() {
    for (const [el, ours] of attached) {
      const refs = (el.getAttribute('aria-describedby') ?? '').split(/\s+/).filter(r => r && !ours.includes(r));
      if (refs.length) el.setAttribute('aria-describedby', refs.join(' ')); else el.removeAttribute('aria-describedby');
    }
    attached.clear();
  }
  function connect() {
    clear();
    const ids = id ? [id] : [...node.querySelectorAll<HTMLElement>('[data-slot="field-description"],[data-field-help-text]')].filter(el => el.closest('[data-help-scope]') === node).map(el => {
      if (!el.id) el.id = `field-description-${++sequence}`;
      return el.id;
    });
    if (!ids.length) return;
    for (const el of node.querySelectorAll(selector)) {
      if (el.closest('[data-help-trigger]')) continue;
      if (el.closest('[data-help-scope]') !== node) continue;
      const refs = new Set((el.getAttribute('aria-describedby') ?? '').split(/\s+/).filter(Boolean));
      const ours = ids.filter(ref => !refs.has(ref));
      ids.forEach(ref => refs.add(ref));
      el.setAttribute('aria-describedby', [...refs].join(' ')); attached.set(el, ours);
    }
  }
  const observer = new MutationObserver(connect);
  observer.observe(node, { childList: true, subtree: true }); connect();
  return { update(next?: string) { clear(); id = next; connect(); }, destroy() { observer.disconnect(); clear(); } };
}
