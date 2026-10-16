// Legacy tab names resolve to the first related child page.
export const CONNECTION_PAGE_IDS = ['harness-link', 'endpoints', 'ollama', 'ports', 'config', 'guide', 'sdk', 'harness-mcp', 'servers', 'import', 'status', 'workbench', 'health'];
/** @param {string | null | undefined} value */
export function resolveConnectionPage(value) {
  const id = value === 'clients' ? 'config' : value === 'mcp' ? 'servers' : value === 'test' ? 'workbench' : value;
  if (!id) return null;
  return CONNECTION_PAGE_IDS.includes(id) ? id : null;
}
/** @param {string} value */
export function connectionPageFromUrl(value) {
  const url = new URL(value);
  const hash = url.hash.slice(1).replace(/^\/?connections[/:]?/, '');
  const params = new URLSearchParams(hash);
  return resolveConnectionPage(url.searchParams.get('connection') ?? url.searchParams.get('section') ?? url.searchParams.get('tab'))
    ?? resolveConnectionPage(params.get('connection') ?? params.get('section') ?? params.get('tab') ?? hash);
}
