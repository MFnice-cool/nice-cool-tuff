import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";
import sveltePreprocess from "svelte-preprocess";
import path from "node:path";
import { fileURLToPath } from "node:url";

const host = process.env.TAURI_DEV_HOST;
const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

// https://v2.tauri.app/start/frontend/vite/
export default defineConfig(async () => ({
  plugins: [
    tailwindcss(),
    svelte({
      preprocess: sveltePreprocess({
        typescript: {
          // .svelte files under node_modules (e.g. @lucide icons) resolve the
          // nearest tsconfig from THEIR directory, not ours — losing
          // verbatimModuleSyntax, so markup-only imports like lucide's `Icon`
          // were elided at runtime ("Icon is not defined"). This transformer
          // is transpile-only, so a fixed option set is safe for every file.
          tsconfigFile: false,
          compilerOptions: {
            verbatimModuleSyntax: true,
            target: "esnext",
          },
        },
      }),
    }),
  ],
  resolve: {
    alias: {
      "@tuffbox/quest-lib": path.join(rootDir, "packages/quest-lib/src/index.ts"),
    },
  },
  clearScreen: false,
  optimizeDeps: {
    // Svelte libraries shipping .svelte runes sources break under esbuild
    // pre-bundling (@xyflow fails to compile; @lucide loses its internal
    // Icon component across chunks). Let vite-plugin-svelte transform them.
    exclude: ["@xyflow/svelte", "@lucide/svelte"],
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    // Browser preview (e2b.app sandbox proxy) hits the dev server through a
    // generated *.e2b.app host — allow it or Vite answers 403.
    allowedHosts: true,

    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    // Tauri uses Chromium on Windows and WebKit on macOS/Linux.
    target:
      process.env.TAURI_ENV_PLATFORM === "windows"
        ? "chrome105"
        : "safari13",
    minify: process.env.TAURI_ENV_DEBUG ? false : "esbuild",
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    // Two entries: the main IDE shell and the standalone "Browse content"
    // window (mods-browser.html) that opens beside the IDE.
    rollupOptions: {
      input: {
        main: path.resolve(path.dirname(fileURLToPath(import.meta.url)), "index.html"),
        modsBrowser: path.resolve(path.dirname(fileURLToPath(import.meta.url)), "mods-browser.html"),
      },
    },
  },
}));
