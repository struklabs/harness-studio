// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
// @ts-expect-error type error without @types/node package
import { readFileSync, realpathSync } from "node:fs";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;
const appVersion = JSON.parse(readFileSync("package.json", "utf8")).version;

// `harness serve` compares this marker with its own version and refuses a stale UI bundle.
/** @returns {import("vite").Plugin} */
const uiBuildMarker = () => ({
  name: "harness-ui-build-marker",
  apply: "build",
  generateBundle() {
    this.emitFile({
      type: "asset",
      fileName: "harness-ui.json",
      source: JSON.stringify({ version: appVersion }) + "\n",
    });
  },
});

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [
    tailwindcss(),
    uiBuildMarker(),
    sveltekit({
      preprocess: vitePreprocess(),
      adapter: adapter({ fallback: "index.html" }),
    }),
  ],
  // Keep optimised chunks local (also when node_modules is a symlink or junction) and
  // allow the resolved dependency directory (including local font files) to be served.
  cacheDir: ".svelte-kit/vite-cache",

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    fs: { allow: [realpathSync("."), realpathSync("node_modules")] },
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`, the Cargo workspace (`crates`, `target`)
      // and other non-frontend folders. `target` alone holds tens of thousands of files and
      // exhausts Linux's inotify limit (ENOSPC), which stops `tauri dev`.
      ignored: ["**/src-tauri/**", "**/target/**", "**/crates/**", "**/docs/**", "**/test/**", "**/scratch/**"],
    },
  },
}));
