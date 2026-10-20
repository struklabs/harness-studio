// Title bar toolbar: which buttons exist, which the user hid, and which fit at the current width.

/** Button ids in display (and overflow-priority) order: the first ones stay inline longest. */
/** @typedef {'sidebar' | 'downloads' | 'server' | 'logs' | 'folder'} ToolbarId */

/** @type {{ id: ToolbarId, advanced: boolean }[]} */
export const TOOLBAR_ITEMS = [
  { id: 'sidebar', advanced: false },
  { id: 'downloads', advanced: false },
  { id: 'server', advanced: false },
  { id: 'logs', advanced: true },
  { id: 'folder', advanced: true },
];

/** Buttons the user may show: not hidden by them and allowed by the Simple/Advanced level. */
/**
 * @param {ToolbarId[]} hidden
 * @param {boolean} advanced
 * @returns {ToolbarId[]}
 */
export function shownItems(hidden, advanced) {
  return TOOLBAR_ITEMS.filter((item) => (advanced || !item.advanced) && !hidden.includes(item.id)).map((i) => i.id);
}

/** How many buttons fit inline next to the search box before the rest move into the overflow menu. */
/** @param {number} viewport */
export function inlineCount(viewport) {
  if (viewport < 560) return 1;
  if (viewport < 720) return 2;
  if (viewport < 900) return 3;
  return TOOLBAR_ITEMS.length;
}

/** Splits shown ids into those drawn inline and those listed in the overflow menu. */
/**
 * @param {ToolbarId[]} shown
 * @param {number} viewport
 * @returns {{ inline: ToolbarId[], overflow: ToolbarId[] }}
 */
export function splitItems(shown, viewport) {
  const n = inlineCount(viewport);
  return { inline: shown.slice(0, n), overflow: shown.slice(n) };
}

/** Parses the stored hidden list, dropping unknown ids. */
/**
 * @param {string | null | undefined} raw
 * @returns {ToolbarId[]}
 */
export function parseHidden(raw) {
  try {
    const v = JSON.parse(raw ?? '[]');
    return Array.isArray(v) ? v.filter((id) => TOOLBAR_ITEMS.some((i) => i.id === id)) : [];
  } catch {
    return [];
  }
}
