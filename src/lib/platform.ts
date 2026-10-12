// Host OS from the webview's user agent, for placeholders and hints that show a path the way
// the user's own system writes it.
const ua = typeof navigator !== 'undefined' ? `${navigator.userAgent} ${navigator.platform}` : '';

export const isWindows = /Win/i.test(ua);
export const isMac = /Mac/i.test(ua);

/** An example path for the current OS: `win` on Windows, `unix` (Linux and macOS) elsewhere. */
export const pathExample = (win: string, unix: string): string => (isWindows ? win : unix);
