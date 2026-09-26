/** Standalone "Browse content" window entry — a separate OS window so the
 * user can page through mods while the main window stays on any IDE tab.
 * Deliberately minimal: theme + styles + the browser, nothing else. */
import { mount } from "svelte";
import ModsBrowserWindow from "./components/ModsBrowserWindow.svelte";
import "./styles/fonts.css";
import "./styles.css";
import "./styles/themes.css";
import "./styles/textures.css";
import { applyTheme, readStoredTheme } from "./lib/themes";

applyTheme(readStoredTheme(), false);

const app = mount(ModsBrowserWindow, {
  target: document.getElementById("app")!,
});

export default app;
