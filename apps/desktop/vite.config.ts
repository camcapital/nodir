import { defineConfig } from "vitest/config";

// Tauri expects a fixed port and its own console output, and only needs to
// target the webviews it ships with.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: ["es2022", "safari15"],
    outDir: "dist",
    emptyOutDir: true,
    // CodeMirror and its markdown language; it all loads from disk.
    chunkSizeWarningLimit: 1500,
  },
  test: {
    environment: "jsdom",
  },
});
