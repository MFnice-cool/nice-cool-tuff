import { mount } from "svelte";
import App from "./App.svelte";
import "./styles/fonts.css";
import "./styles.css";
import "./styles/themes.css";
import "./styles/textures.css";
import { applyTheme, readStoredTheme } from "./lib/themes";
import { installActionFeedback } from "./lib/actionFeedback";
import { isTauri } from "@tauri-apps/api/core";
import { installBrowserMockIfNeeded } from "./lib/browserMock";

// Browser preview (e2b.app, vite dev without Tauri) — install full IPC mock
// synchronously before any component touches `invoke`/`listen`.
// installBrowserMockIfNeeded is async but its synchronous prelude (mockIPC)
// runs immediately; we don't need to await the promise for dev preview.
void installBrowserMockIfNeeded();

applyTheme(readStoredTheme(), false);
installActionFeedback();

// Unified logging (tauri-plugin-tracing): bridge JS console output into the
// Rust tracing subscriber, which writes the rotating logs/tuffbox.*.log file
// alongside Rust spans. Browser preview (no Tauri) skips the interception.
if (isTauri()) {
  void import("@fltsci/tauri-plugin-tracing")
    .then(({ interceptConsole }) =>
      interceptConsole({ preserveOriginal: true }),
    )
    .catch(() => {
      /* logging bridge is best-effort; never block startup */
    });
}

window.addEventListener("error", (event) => {
  console.error("[tuffbox] uncaught error:", event.error ?? event.message);
});

window.addEventListener("unhandledrejection", (event) => {
  console.error("[tuffbox] unhandled rejection:", event.reason);
});

const app = mount(App, {
  target: document.getElementById("app")!,
});

export default app;
