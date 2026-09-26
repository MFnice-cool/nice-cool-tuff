/**
 * Opens the content browser (mods / resource packs / shaders / datapacks)
 * as a separate OS window so the main window stays usable (IDE, Graph, …).
 *
 * Strategy: reuse one window per app run (label "mods-browser"); a second
 * request focuses it and re-navigates via an event. If window creation or
 * messaging fails for any reason (permissions, runtime), callers fall back
 * to the previous in-app behavior so nothing blocks.
 */

export type BrowserContentType = "mod" | "resourcepack" | "shader" | "datapack";

const WINDOW_LABEL = "mods-browser";
const NAV_EVENT = "mods-browser:navigate";

/** Module-local "already opened" flag — avoids permission-gated window
 * enumeration just to decide between focus and create. */
let knownOpen = false;

export function buildBrowserUrl(projectPath: string, type?: BrowserContentType): string {
  const params = new URLSearchParams();
  params.set("path", projectPath);
  if (type) params.set("type", type);
  return `mods-browser.html?${params.toString()}`;
}

/**
 * Opens (or focuses) the standalone browser window.
 * Returns true when the separate window handles it; false → run fallback.
 */
export async function openModsBrowserWindow(
  projectPath: string,
  type?: BrowserContentType,
): Promise<boolean> {
  try {
    const { emitTo } = await import("@tauri-apps/api/event");
    const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
    if (knownOpen) {
      // Reuse the open window: focus it and switch its target pack/type.
      await emitTo(WINDOW_LABEL, NAV_EVENT, { path: projectPath, type: type ?? null });
      return true;
    }
    const win = new WebviewWindow(WINDOW_LABEL, {
      url: buildBrowserUrl(projectPath, type),
      title: "TuffBox — Browse content",
      width: 1180,
      height: 820,
      minWidth: 720,
      minHeight: 480,
      center: true,
      resizable: true,
      decorations: true,
    });
    await new Promise<void>((resolve, reject) => {
      win.once("tauri://created", () => resolve());
      win.once("tauri://error", (e) => reject(e.payload ?? new Error("window creation failed")));
    });
    knownOpen = true;
    win.once("tauri://destroyed", () => {
      knownOpen = false;
    });
    return true;
  } catch (e) {
    console.warn("[tuffbox] standalone content browser unavailable:", e);
    knownOpen = false;
    return false;
  }
}
