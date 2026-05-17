import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";

// Mockingbird UI build config.
//
// Two HTML entry points:
//   - index.html      → main window (sidebar + Insights/History/Dictionary/Modes/Settings)
//   - recording.html  → recording overlay (frameless, non-activating)
//
// We deliberately keep the build tiny — no chart libs, no @tanstack/*,
// no router data layer; everything either ships in this repo or is
// pinned in package.json with --ignore-scripts enforced via .npmrc.
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": resolve(__dirname, "src"),
    },
  },
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        recording: resolve(__dirname, "recording.html"),
      },
      output: {
        // Stable chunk names so Tauri's CSP hash list (Phase 7) is
        // predictable; helps the asset reviewer too.
        entryFileNames: "assets/[name]-[hash].js",
        chunkFileNames: "assets/[name]-[hash].js",
        assetFileNames: "assets/[name]-[hash][extname]",
      },
    },
    // Source maps stay on in dev; off in release for smaller bundles.
    sourcemap: process.env.NODE_ENV !== "production",
    // 1 MB chunk warning is plenty for our scope.
    chunkSizeWarningLimit: 1024,
    target: "es2022",
  },
  server: {
    // Tauri talks to Vite at 5173 in dev.
    port: 5173,
    strictPort: true,
    // No HMR overlay — distracting when the recording overlay is on
    // top. We get errors in the devtools console anyway.
    hmr: { overlay: false },
  },
  // Test config (vitest reads it from here via mergeConfig).
  test: {
    environment: "jsdom",
    globals: false,
    setupFiles: ["./tests/setup-unit.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
