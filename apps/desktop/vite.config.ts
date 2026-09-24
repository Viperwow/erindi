import { defineConfig } from "vite";
import preact from "@preact/preset-vite";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [preact(), tailwindcss()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    // The CSP allows images from 'self' only, so small images must not be inlined as data: URIs.
    assetsInlineLimit: 0,
    rollupOptions: { input: { settings: "index.html", overlay: "overlay.html" } },
  },
});
