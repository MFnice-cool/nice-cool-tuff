/** Theme catalog (recreated for TuffBox CSS vars). */
import { invoke } from "@tauri-apps/api/core";

export type ThemeId =
  | "tuffbox"
  | "tuffbox-light"
  | "carbon"
  | "inferno"
  | "aether"
  | "frost"
  | "pixelato"
  | "win95"
  | "solar"
  | "fern"
  | "blaze"
  | "dusk"
  | "glacier"
  | "minecraft"
  | "overworld"
  | "nether"
  | "deepdark"
  | "amethyst";

export interface ThemeMeta {
  id: ThemeId;
  label: string;
  /** Swatch colors for the card preview (bg deep → mid → accent). */
  shades: [string, string, string];
  /** Picker badge: Light | Sharp (hard edges) | Minimal (macOS soft). */
  badge?: "Light" | "Sharp" | "Minimal";
}

export const THEMES: ThemeMeta[] = [
  { id: "tuffbox", label: "TuffBox", shades: ["#0b0b0d", "#18181b", "#1bd96a"] },
  { id: "tuffbox-light", label: "TuffBox Light", shades: ["#f3f5f1", "#e2e8df", "#0a9a44"], badge: "Light" },
  { id: "carbon", label: "Carbon", shades: ["#15181e", "#272a35", "#3e85d0"] },
  { id: "inferno", label: "Inferno", shades: ["#120a0a", "#261616", "#ff5722"] },
  { id: "aether", label: "Aether", shades: ["#1e0d3c", "#31155f", "#c084fc"], badge: "Sharp" },
  { id: "frost", label: "Frost", shades: ["#072a44", "#104a74", "#22d3ee"], badge: "Sharp" },
  { id: "pixelato", label: "Pixelato", shades: ["#101010", "#303030", "#4caf50"], badge: "Sharp" },
  { id: "win95", label: "Win95", shades: ["#a5a5a5", "#c0c0c0", "#000080"], badge: "Sharp" },
  { id: "solar", label: "Solar", shades: ["#140e02", "#ffc500", "#ff9500"], badge: "Minimal" },
  { id: "fern", label: "Fern", shades: ["#001408", "#00c85a", "#00e0a8"], badge: "Minimal" },
  { id: "blaze", label: "Blaze", shades: ["#140000", "#ff2d2d", "#ff7a45"], badge: "Minimal" },
  { id: "dusk", label: "Dusk", shades: ["#180310", "#ff6b9c", "#c084fc"], badge: "Minimal" },
  { id: "glacier", label: "Glacier", shades: ["#000c18", "#a8e6ff", "#5b9fff"], badge: "Minimal" },
  { id: "minecraft", label: "Minecraft", shades: ["#212121", "#313131", "#3fad32"] },
  { id: "overworld", label: "Overworld", shades: ["#0d100c", "#1a2416", "#52d465"] },
  { id: "nether", label: "Nether", shades: ["#140608", "#241012", "#ff6a2b"] },
  { id: "deepdark", label: "Deep Dark", shades: ["#05090e", "#0a141c", "#2fd6c8"] },
  { id: "amethyst", label: "Amethyst", shades: ["#0f0a18", "#1c1230", "#b07cff"] },
];

const STORAGE_KEY = "tuffbox-theme";

/** Glass/transparency appearance mode: `data-glass="on|off"` on <html>.
 *  When enabled, also asks the Rust core for a real OS window effect
 *  (Acrylic/Mica/Tabbed on Windows, vibrancy on macOS). If the OS refuses
 *  (unsupported platform/DE), we keep CSS-only translucency: surfaces stay
 *  see-through over the textured shell background. */
export async function applyGlassEffects(enabled: unknown) {
  const on = enabled === true;
  if (typeof document === "undefined") return on;
  document.documentElement.setAttribute("data-glass", on ? "on" : "off");
  try {
    localStorage.setItem("tuffbox-glass", on ? "1" : "0");
  } catch {
    /* ignore */
  }
  // OS-level window glass (better than backdrop-filter alone). Real command
  // invocations go through @tauri-apps/api/core — the global __TAURI__.invoke
  // does NOT exist in Tauri 2 (commands live under __TAURI__.core.invoke), so
  // reading w.__TAURI__.invoke always returned undefined and glass appeared
  // dead even though the OS effect applies fine.
  const invokeAvailable = typeof invoke === "function";
  if (on && invokeAvailable) {
    try {
      const effect = ((await invoke("set_window_glass", { on: true })) as string) || "";
      // OS effect active → let the desktop blur show through the shell.
      if (effect && effect !== "off") {
        document.documentElement.classList.add("glass-os");
        try {
          localStorage.setItem("tuffbox-glass-os", effect);
        } catch {
          /* ignore */
        }
        return on;
      }
    } catch {
      /* fall through to CSS-only glass */
    }
  } else if (!on && invokeAvailable) {
    try {
      await invoke("set_window_glass", { on: false });
    } catch {
      /* ignore */
    }
  }
  // No OS effect (or glass off): make sure the shell background is opaque.
  document.documentElement.classList.remove("glass-os");
  try {
    localStorage.removeItem("tuffbox-glass-os");
  } catch {
    /* ignore */
  }
  return on;
}

/** Read the stored glass preference (default: off). */
export function readStoredGlass(): boolean {
  try {
    return localStorage.getItem("tuffbox-glass") === "1";
  } catch {
    return false;
  }
}

export function readStoredTheme(): ThemeId {
  const raw = localStorage.getItem(STORAGE_KEY) || "tuffbox";
  if (THEMES.some((t) => t.id === raw)) return raw as ThemeId;
  // Migrate legacy dark/light toggle
  if (raw === "dark") return "tuffbox";
  if (raw === "light") return "tuffbox-light";
  return "tuffbox";
}

export function applyTheme(id: ThemeId, persist = false) {
  document.documentElement.setAttribute("data-theme", id);
  if (persist) {
    localStorage.setItem(STORAGE_KEY, id);
  }
}

export function previewTheme(id: ThemeId) {
  applyTheme(id, false);
}

export function commitTheme(id: ThemeId) {
  applyTheme(id, true);
}

export function restoreCommittedTheme() {
  applyTheme(readStoredTheme(), false);
}
