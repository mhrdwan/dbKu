import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Set by `tauri dev` when developing on a device on the network.
const host = (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env.TAURI_DEV_HOST;

// https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2022",
    chunkSizeWarningLimit: 800,
  },
  test: {
    include: ["src/**/*.test.ts"],
  },
});
