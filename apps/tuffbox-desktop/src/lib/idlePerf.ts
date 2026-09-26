/**
 * Idle-CPU policy for the desktop shell.
 *
 * A launcher sits in the background most of the time — it must be
 * near-silent there. The window being minimized or fully occluded makes
 * WebView2 report `document.visibilityState === "hidden"` (Chromium native
 * occlusion tracking), which drives the helpers below.
 */

/** Body class while the window is hidden (pauses every CSS animation). */
export const APP_HIDDEN_CLASS = "app-hidden";

/** Task-panel polling while background work is visible to the user. */
export const TASK_POLL_ACTIVE_MS = 2000;
/** Task-panel polling while no background work is shown. */
export const TASK_POLL_IDLE_MS = 15000;

/**
 * Background-task polling cadence: responsive while tasks are on screen,
 * relaxed when the panel has nothing to show. (The old fixed 800ms tick
 * kept the WebView2 process awake on an otherwise idle home page.)
 */
export function taskPollIntervalMs(activeTasks: number): number {
  return activeTasks > 0 ? TASK_POLL_ACTIVE_MS : TASK_POLL_IDLE_MS;
}

/**
 * Keep `body.app-hidden` in sync with the window's visibility. CSS pauses
 * all animations for that class (styles.css), so a backgrounded launcher
 * stops compositing frames entirely.
 *
 * Returns a cleanup that removes the listener and the class.
 */
export function installAppHiddenSync(): () => void {
  const sync = () => {
    document.body.classList.toggle(
      APP_HIDDEN_CLASS,
      document.visibilityState !== "visible",
    );
  };
  sync();
  document.addEventListener("visibilitychange", sync);
  return () => {
    document.removeEventListener("visibilitychange", sync);
    document.body.classList.remove(APP_HIDDEN_CLASS);
  };
}
