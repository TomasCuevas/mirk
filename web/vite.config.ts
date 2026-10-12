import { defineConfig } from "vite";

// Run from the repository root (`bun run build`), because `root` is resolved relative to the
// working directory. The output has to land where `crates/oct-desktop/tauri.conf.json` says the
// frontend dist is, since Tauri embeds it at compile time.
export default defineConfig({
  root: "web",
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
});
