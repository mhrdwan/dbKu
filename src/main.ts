import { isTauri } from "@tauri-apps/api/core";
import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";

// In a plain browser (`pnpm dev` without Tauri) answer IPC with demo data,
// so the UI can be worked on without the Rust backend.
if (import.meta.env.DEV && !isTauri()) {
  const { installMockBackend } = await import("./dev/mockBackend");
  installMockBackend();
}

const app = mount(App, { target: document.getElementById("app")! });

export default app;
