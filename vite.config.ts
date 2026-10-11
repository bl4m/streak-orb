import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    proxy: {
      "/preview/duolingo": {
        target: "https://www.duolingo.com",
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/preview\/duolingo/, ""),
      },
      "/preview/github-api": {
        target: "https://api.github.com",
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/preview\/github-api/, ""),
      },
      "/preview/github": {
        target: "https://github.com",
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/preview\/github/, ""),
      },
      "/preview/leetcode": {
        target: "https://leetcode.com",
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/preview\/leetcode/, ""),
      },
      "/preview/codeforces": {
        target: "https://codeforces.com",
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/preview\/codeforces/, ""),
      },
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "safari15",
    minify: "esbuild",
    sourcemap: false,
  },
});
