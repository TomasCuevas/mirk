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
    // Two projects, because the suite tests two kinds of thing.
    //
    // Most files test pure string/state decisions and need no DOM at all. The click tests need a
    // browser: they mount a component and press a button, which the server-side render used elsewhere
    // cannot do — and under `environment: node` Svelte resolves to its server build, so `mount` is not
    // even available. Keeping them apart means neither borrows a capability it does not need (L64).
    projects: [
      {
        extends: true,
        test: {
          name: "node",
          environment: "node",
          include: ["src/**/*.test.ts"],
          exclude: ["src/**/*.dom.test.ts"],
        },
      },
      {
        extends: true,
        resolve: { conditions: ["browser"] },
        test: {
          name: "dom",
          environment: "happy-dom",
          include: ["src/**/*.dom.test.ts"],
        },
      },
    ],
  },
});
