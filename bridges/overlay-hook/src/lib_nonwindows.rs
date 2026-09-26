//! Non-Windows stub — overlay GL hook is Windows-only.

#![cfg(not(windows))]

#[no_mangle]
pub extern "C" fn tuffbox_overlay_hook_unsupported() {}
