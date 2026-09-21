import { defineConfig } from "vite";
import preact from "@preact/preset-vite";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [preact(), tailwindcss()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    rollupOptions: { input: { settings: "index.html", overlay: "overlay.html" } },
  },
});
