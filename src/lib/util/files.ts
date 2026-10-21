// File dialogs. Only paths the user picks in a native dialog become
// writable/readable (the dialog plugin adds them to the fs scope).

import { open, save } from "@tauri-apps/plugin-dialog";
import { readTextFile, writeTextFile } from "@tauri-apps/plugin-fs";

type Filter = { name: string; extensions: string[] };

export async function saveTextFile(defaultPath: string, content: string, filters: Filter[]): Promise<string | null> {
  const path = await save({ defaultPath, filters });
  if (!path) return null;
  await writeTextFile(path, content);
  return path;
}

export async function openTextFile(filters: Filter[]): Promise<{ path: string; content: string } | null> {
  const path = await open({ multiple: false, directory: false, filters });
  if (!path || Array.isArray(path)) return null;
  return { path, content: await readTextFile(path) };
}

export async function pickFile(filters: Filter[], create = false): Promise<string | null> {
  if (create) return (await save({ filters })) ?? null;
  const path = await open({ multiple: false, directory: false, filters });
  return typeof path === "string" ? path : null;
}

export function safeFileName(name: string): string {
  return name.replace(/[^\w.-]+/g, "_").slice(0, 80) || "export";
}
