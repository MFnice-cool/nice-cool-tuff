//! Shared input state: mouse wheel accumulator + low-level hooks helpers.

use std::sync::atomic::{AtomicI32, AtomicBool, Ordering};

/// Accumulated wheel ticks (positive = scroll up / content moves down).
static WHEEL: AtomicI32 = AtomicI32::new(0);

/// Set by the mouse LL-hook; consumed once per frame by the UI.
pub fn push_wheel(delta: i32) {
    // Normalize Windows ±120 units into roughly ±1 step, keep magnitude.
    let steps = if delta == 0 {
        0
    } else {
        // Preserve sign; magnitude at least 1.
        let s = delta / 120;
        if s == 0 {
            delta.signum()
        } else {
            s
        }
    };
    WHEEL.fetch_add(steps, Ordering::SeqCst);
}

/// Drain wheel delta for this frame (in "notches", positive = up).
pub fn take_wheel() -> f32 {
    WHEEL.swap(0, Ordering::SeqCst) as f32
}

static OVERLAY_WANTS_INPUT: AtomicBool = AtomicBool::new(false);

pub fn set_overlay_open(open: bool) {
    OVERLAY_WANTS_INPUT.store(open, Ordering::SeqCst);
}

pub fn overlay_open() -> bool {
    OVERLAY_WANTS_INPUT.load(Ordering::SeqCst)
}
