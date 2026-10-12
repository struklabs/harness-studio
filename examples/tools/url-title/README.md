# url_title

Returns the `<title>` of a web page.

- **Input:** `{ "url": "https://en.wikipedia.org/wiki/Rust_(programming_language)" }`
- **Output:** the title text.
- **Permissions:** network access to `example.com` and `*.wikipedia.org` only; no file access; no shell.

Install it from Settings > Tools > Installed tools (choose this folder), or copy the folder to
`<Harness data dir>/tools/url_title/`. To let it reach other sites, add their host names to
`permissions.network` in `manifest.json` before installing.
