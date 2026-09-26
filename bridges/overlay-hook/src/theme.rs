//! Minecraft-inspired UI palette (dirt / stone / grass / gold).
//! Square corners, bevelled buttons — not Discord, not the vanilla bitmap font.

#![allow(dead_code)]

/// Dim the world behind the shell (deep dirt-black).
pub const BACKDROP: (f32, f32, f32, f32) = (0.05, 0.04, 0.03, 0.72);

/// Left rail — packed dirt.
pub const RAIL_BG: (f32, f32, f32, f32) = (0.42, 0.30, 0.19, 1.0);
pub const RAIL_HOVER: (f32, f32, f32, f32) = (0.52, 0.38, 0.24, 1.0);
pub const RAIL_ACTIVE: (f32, f32, f32, f32) = (0.35, 0.25, 0.15, 1.0);

/// Main content — stone-slab grey-brown.
pub const CONTENT_BG: (f32, f32, f32, f32) = (0.55, 0.55, 0.55, 1.0);
pub const PANEL_BG: (f32, f32, f32, f32) = (0.45, 0.45, 0.45, 1.0);
pub const TOPBAR_BG: (f32, f32, f32, f32) = (0.32, 0.24, 0.15, 1.0);
pub const STATUS_BG: (f32, f32, f32, f32) = (0.22, 0.16, 0.10, 1.0);

/// Grass / emerald accent (active tabs, primary buttons).
pub const ACCENT: (f32, f32, f32, f32) = (0.35, 0.62, 0.22, 1.0);
pub const ACCENT_HOVER: (f32, f32, f32, f32) = (0.45, 0.75, 0.28, 1.0);
pub const ACCENT_DARK: (f32, f32, f32, f32) = (0.22, 0.42, 0.12, 1.0);

/// Stone button face.
pub const BTN: (f32, f32, f32, f32) = (0.55, 0.55, 0.55, 1.0);
pub const BTN_HOVER: (f32, f32, f32, f32) = (0.62, 0.70, 0.45, 1.0);
pub const BTN_LIGHT: (f32, f32, f32, f32) = (0.78, 0.78, 0.78, 1.0);
pub const BTN_DARK: (f32, f32, f32, f32) = (0.28, 0.28, 0.28, 1.0);

/// Bevel / border edges (classic MC 2px raised frame).
pub const BORDER_LIGHT: (f32, f32, f32, f32) = (1.0, 1.0, 1.0, 0.35);
pub const BORDER_DARK: (f32, f32, f32, f32) = (0.0, 0.0, 0.0, 0.55);
pub const BORDER_OUTER: (f32, f32, f32, f32) = (0.12, 0.08, 0.05, 1.0);

pub const TEXT: (f32, f32, f32, f32) = (0.95, 0.95, 0.95, 1.0);
pub const TEXT_SHADOW: (f32, f32, f32, f32) = (0.12, 0.10, 0.08, 0.85);
pub const TEXT_DIM: (f32, f32, f32, f32) = (0.78, 0.75, 0.68, 1.0);
pub const TEXT_MUTED: (f32, f32, f32, f32) = (0.55, 0.52, 0.45, 1.0);
pub const TEXT_GOLD: (f32, f32, f32, f32) = (1.0, 0.84, 0.25, 1.0);

pub const SUCCESS: (f32, f32, f32, f32) = (0.33, 0.78, 0.28, 1.0);
pub const DANGER: (f32, f32, f32, f32) = (0.78, 0.18, 0.14, 1.0);
pub const WARNING: (f32, f32, f32, f32) = (0.95, 0.72, 0.18, 1.0);

pub const INPUT_BG: (f32, f32, f32, f32) = (0.0, 0.0, 0.0, 0.55);
pub const DIVIDER: (f32, f32, f32, f32) = (0.25, 0.18, 0.10, 1.0);
pub const ROW_HOVER: (f32, f32, f32, f32) = (0.62, 0.70, 0.40, 0.35);
pub const ROW_SELECTED: (f32, f32, f32, f32) = (0.35, 0.55, 0.20, 0.55);

/// Gold highlight strip (enchanted-book vibe).
pub const GOLD: (f32, f32, f32, f32) = (0.90, 0.72, 0.18, 1.0);

pub const RAIL_W: f32 = 148.0;
pub const TOPBAR_H: f32 = 44.0;
pub const STATUS_H: f32 = 28.0;

/// Bevel thickness in pixels.
pub const BEVEL: f32 = 2.0;
