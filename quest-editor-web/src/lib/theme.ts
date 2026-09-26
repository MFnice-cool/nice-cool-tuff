/**
 * Theme management for quest editor.
 * Supports dark and light themes with CSS custom properties.
 */

export type Theme = "dark" | "light";

export interface ThemeState {
  current: Theme;
  storageKey: string;
}

export function createThemeState(): ThemeState {
  return {
    current: "dark",
    storageKey: "tuffbox.quest_editor.theme",
  };
}

export function loadTheme(): Theme {
  try {
    const saved = localStorage.getItem("tuffbox.quest_editor.theme");
    if (saved === "light" || saved === "dark") return saved;
  } catch { /* ignore */ }
  return "dark";
}

export function saveTheme(theme: Theme): void {
  try {
    localStorage.setItem("tuffbox.quest_editor.theme", theme);
  } catch { /* ignore */ }
}

export function toggleTheme(current: Theme): Theme {
  return current === "dark" ? "light" : "dark";
}

export function getThemeVars(theme: Theme): Record<string, string> {
  if (theme === "light") {
    return {
      "--bg-primary": "#ffffff",
      "--bg-secondary": "#f5f5f5",
      "--bg-tertiary": "#e8e8e8",
      "--bg-canvas": "#f0f0f0",
      "--text-primary": "#1a1a1e",
      "--text-secondary": "#4a4a52",
      "--text-muted": "#7a7a82",
      "--border": "#d0d0d8",
      "--accent": "#2d8a7a",
      "--accent-hover": "#3db8a8",
      "--warning": "#d97706",
      "--danger": "#dc2626",
      "--success": "#16a34a",
      "--node-border": "#1a1a1e",
      "--node-bg": "#ffffff",
      "--node-selected": "#2d8a7a",
      "--grid-line": "rgba(0,0,0,0.06)",
    };
  }
  return {
    "--bg-primary": "#1a1a1e",
    "--bg-secondary": "#212126",
    "--bg-tertiary": "#2b2b30",
    "--bg-canvas": "#2b2b30",
    "--text-primary": "#e8e8e8",
    "--text-secondary": "#9a9aa0",
    "--text-muted": "#6a6a72",
    "--border": "#3a3a42",
    "--accent": "#3db8a8",
    "--accent-hover": "#55c95a",
    "--warning": "#fbbf24",
    "--danger": "#f87171",
    "--success": "#55c95a",
    "--node-border": "#ffffff",
    "--node-bg": "#18181c",
    "--node-selected": "#55c95a",
    "--grid-line": "rgba(255,255,255,0.03)",
  };
}
