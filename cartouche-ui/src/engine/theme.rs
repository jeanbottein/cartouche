//! Retro palette. Background is **pure black**; accents reuse the brown/amber/
//! green scheme of the legacy Dear PyGui theme so the look stays consistent.

use macroquad::prelude::Color;

/// Pure black background (ENGINE-6 default).
pub const BG: Color = Color::new(0.0, 0.0, 0.0, 1.0);

/// Panel/frame fill — barely-lifted-from-black so cards read against BG.
pub const PANEL: Color = Color::new(0.055, 0.043, 0.031, 1.0); // ~ (14,11,8)

/// Primary text.
pub const TEXT: Color = Color::new(0.902, 0.922, 0.949, 1.0); // (230,235,242)
/// Secondary/muted text.
pub const MUTED: Color = Color::new(0.431, 0.463, 0.510, 1.0); // (110,118,130)

/// Green accent (focus/cursor, confirm).
pub const ACCENT: Color = Color::new(0.149, 0.659, 0.424, 1.0); // (38,168,108)
/// Amber/gold accent (titles, selection).
pub const AMBER: Color = Color::new(0.627, 0.322, 0.063, 1.0); // (160,82,16)
/// Blue accent (info/tabs).
pub const BLUE: Color = Color::new(0.337, 0.612, 0.839, 1.0); // (86,156,214)

/// Subtle border for panels.
pub const BORDER: Color = Color::new(0.255, 0.188, 0.118, 1.0); // (65,48,30)
