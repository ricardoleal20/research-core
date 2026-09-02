import { defineConfig } from "vite";

// Tauri expects a fixed dev server port and the same host.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
  },
  // Tauri injects env vars; build to dist/ which tauri.conf.json points at.
  build: {
    target: "esnext",
    outDir: "dist",
    emptyOutDir: true,
  },
});
