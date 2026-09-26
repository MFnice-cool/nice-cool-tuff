/**
 * Copy text to the system clipboard.
 * `navigator.clipboard` throws NotAllowedError in Tauri when the webview
 * lost focus (e.g. after a save dialog or closing a menu) — fall back to
 * a focused textarea + execCommand.
 */
export async function copyText(text: string): Promise<void> {
  const value = String(text ?? "");
  try {
    window.focus();
    await navigator.clipboard.writeText(value);
    return;
  } catch {
    // fall through
  }

  const ta = document.createElement("textarea");
  ta.value = value;
  ta.setAttribute("readonly", "");
  ta.style.cssText =
    "position:fixed;left:0;top:0;width:1px;height:1px;padding:0;border:0;opacity:0;pointer-events:none;";
  document.body.appendChild(ta);
  try {
    window.focus();
    ta.focus();
    ta.select();
    ta.setSelectionRange(0, value.length);
    if (!document.execCommand("copy")) {
      throw new Error("Clipboard write failed");
    }
  } finally {
    document.body.removeChild(ta);
  }
}
