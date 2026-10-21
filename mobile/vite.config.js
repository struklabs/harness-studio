// The mobile client's UI: a SvelteKit SPA (adapter-static, SPA fallback) built into
// `mobile/build` for `mobile/src-tauri`. Shares the desktop's node_modules, theme tokens, UI
// components and translations (`#lib/*` resolves to the desktop's `src/lib`).
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
// @ts-expect-error type error without @types/node package
import { realpathSync } from "node:fs";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

export default defineConfig(() => ({
  plugins: [
    tailwindcss(),
    sveltekit({
      preprocess: vitePreprocess(),
      adapter: adapter({ fallback: "index.html" }),
    }),
  ],
  // The desktop's shared code (UI components, translations): `#lib/*` is its `src/lib`, as the
  // root package.json's `imports` maps it for the desktop's own files.
  resolve: { alias: [{ find: /^#lib\//, replacement: `${realpathSync("../src/lib")}/` }] },
  cacheDir: ".svelte-kit/vite-cache",
  clearScreen: false,
  server: {
    fs: { allow: [realpathSync(".."), realpathSync("../node_modules")] },
    port: 1430,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host ? { protocol: "ws", host, port: 1431 } : undefined,
    watch: { ignored: ["**/src-tauri/**", "**/target/**"] },
  },
}));
