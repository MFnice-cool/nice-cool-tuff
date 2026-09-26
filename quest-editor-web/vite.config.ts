import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import path from "node:path";
import { fileURLToPath } from "node:url";

const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

export default defineConfig({
  plugins: [svelte()],
  resolve: {
    alias: {
      "@tuffbox/quest-lib": path.join(rootDir, "packages/quest-lib/src/index.ts"),
    },
  },
  server: { port: 5173, host: true },
  test: {
    include: ["src/**/*.test.ts", "../packages/quest-lib/src/**/*.test.ts"],
  },
});
