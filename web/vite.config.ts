import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vitest/config";

// Run from the repository root (`bun run build`), because `root` is resolved relative to the
// working directory. The output has to land where `crates/oct-desktop/tauri.conf.json` says the
// frontend dist is, since Tauri embeds it at compile time.
export default defineConfig({
  root: "web",
  plugins: [svelte()],
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2022",
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  test: {
    // The UI decisions under test are pure string/state functions, so no DOM is required.
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
