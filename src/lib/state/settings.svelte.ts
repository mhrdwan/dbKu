// Small per-machine preferences, kept in localStorage.

export type ThemeChoice = "system" | "light" | "dark";

interface Settings {
  theme: ThemeChoice;
  maxRows: number;
  pageSize: number;
  editorFontSize: number;
  sidebarWidth: number;
}

const DEFAULTS: Settings = { theme: "system", maxRows: 5000, pageSize: 200, editorFontSize: 13, sidebarWidth: 264 };
const STORAGE_KEY = "dbku.settings";

function read(): Settings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? { ...DEFAULTS, ...(JSON.parse(raw) as Partial<Settings>) } : DEFAULTS;
  } catch {
    return DEFAULTS;
  }
}

class SettingsStore {
  value = $state<Settings>(read());

  set<K extends keyof Settings>(k: K, v: Settings[K]) {
    this.value = { ...this.value, [k]: v };
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(this.value));
    } catch {
      // Storage full or unavailable: the setting still applies this session.
    }
    if (k === "theme") applyTheme();
  }
}

export const settings = new SettingsStore();

export function applyTheme() {
  const theme = settings.value.theme;
  if (theme === "system") document.documentElement.removeAttribute("data-theme");
  else document.documentElement.setAttribute("data-theme", theme);
}

export function isDarkNow(): boolean {
  const t = settings.value.theme;
  if (t !== "system") return t === "dark";
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}
