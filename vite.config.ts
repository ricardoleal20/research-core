import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "tailwindcss";
import autoprefixer from "autoprefixer";

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
  // Force the local Tailwind+vendored-fonts chain in DEV too — config-file
  // autodiscovery of postcss.config.cjs was silently skipping the utility
  // layer under `vite serve`, leaving utility classes unstyled (icons fell
  // back to the huge default svg size in the lock tile, etc.).
  css: {
    postcss: {
      plugins: [tailwindcss(), autoprefixer()],
    },
  },
  // Svelte 5 (runes) surfaces are added view-by-view alongside the existing
  // vanilla TS views (AD-8); the plugin only compiles .svelte files.
  plugins: [svelte()],
});
