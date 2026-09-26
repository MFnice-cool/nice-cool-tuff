//! OS-level window glass (window-vibrancy): Acrylic on Windows 10,
//! Mica on Windows 11, sidebar vibrancy on macOS. Driven by the user's
//! "Glass transparency" appearance toggle — the frontend invokes
//! `set_window_glass(true|false)` after `applyGlassEffects`.
//!
//! Real translucency needs the WebView body itself to be see-through, so the
//! frontend also sets `html.glass-os` when the OS effect is active: the CSS
//! then drops the opaque shell background and lets the blurred desktop show
//! through. Better than Shard, which only fakes glass with backdrop-filter
//! over an opaque window.

use tauri::Manager;

/// Apply or remove the OS window effect. Returns a short label of what was
/// applied (for the settings hint), or Err when the platform/window refuses.
#[tauri::command(rename_all = "camelCase")]
pub fn set_window_glass(app: tauri::AppHandle, on: bool) -> Result<String, String> {
    let Some(win) = app.get_webview_window("main") else {
        return Err("main window not found".into());
    };
    if !on {
        // Best-effort clear of every effect; a clear failing on one path must
        // not abort the others (e.g. Win10 blur window + acrylic clear Err).
        let _ = window_vibrancy::clear_acrylic(&win);
        let _ = window_vibrancy::clear_blur(&win);
        let _ = window_vibrancy::clear_mica(&win);
        return Ok("off".into());
    }

    #[cfg(target_os = "windows")]
    {
        // Acrylic first: real blur of the desktop — the most visibly
        // "glassy" effect. Tabbed/Mica are opaque-ish washes on Win11 and
        // read as "no transparency" to the user, so they are fallbacks.
        // Acrylic: RGBWA tint (RGBA). A dark slate matches the default
        // TuffBox palette; light themes still read fine because the WebView
        // paints its own tinted surfaces on top.
        if window_vibrancy::apply_acrylic(
            &win,
            Some((16, 20, 27, 125)), // dark slate, ~50% opacity
        )
        .is_ok()
        {
            return Ok("acrylic".into());
        }
        // Windows 10 (build < 22000): Tabbed/Mica are unavailable, and SWCA
        // Acrylic visibly glitches behind WebView2 (loses blur on blur-
        // behind, flickers on focus loss). ACCENT_ENABLE_BLURBEHIND is the
        // stable Aero-style glass on Win10 — less frosted than Acrylic but
        // reliably transparent.
        if window_vibrancy::apply_blur(&win, Some((16, 20, 27, 90))).is_ok() {
            return Ok("blur".into());
        }
        if window_vibrancy::apply_tabbed(&win, None).is_ok() {
            return Ok("tabbed".into());
        }
        if window_vibrancy::apply_mica(&win, None).is_ok() {
            return Ok("mica".into());
        }
        Err("no windows glass effect available (Acrylic/Blur/Mica/Tabbed all failed)".into())
    }

    #[cfg(target_os = "macos")]
    {
        window_vibrancy::apply_vibrancy(
            &win,
            window_vibrancy::NSVisualEffectMaterial::HudWindow,
            Some(window_vibrancy::NSVisualEffectState::Active),
            Some(12.0), // window corner radius, matches default Tauri macOS chrome
        )
        .map(|_| "vibrancy".into())
        .map_err(|e| format!("vibrancy failed: {e}"))
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = win;
        Err("window glass is not supported on this platform".into())
    }
}
