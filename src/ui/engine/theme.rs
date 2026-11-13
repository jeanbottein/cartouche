//! Themes: palette, typeface and shape style. Every theme dresses the same
//! layout; only colors, fonts, corners and glows change.
//!
//! The current theme is process-wide (screens read colors while drawing, far
//! from any shared state), picked from `THEME` in `config.txt`.

use std::sync::atomic::{AtomicUsize, Ordering};

use macroquad::prelude::Color;

use super::text::Typeface;

pub struct Theme {
    /// Value in `config.txt`.
    pub id: &'static str,
    /// Shown in Settings.
    pub name: &'static str,
    pub typeface: Typeface,
    /// Corner radius of cards and buttons, in design pixels; 0 is square.
    pub radius: f32,
    /// Soft halo around the focused card.
    pub glow: bool,
    /// Outline width of the focused card and of panels.
    pub focus_width: f32,
    /// Outline unfocused cards too (otherwise only their fill shows).
    pub card_outline: bool,
    /// Background gradient, top to bottom.
    pub bg_top: Color,
    pub bg_bottom: Color,
    /// Cards and panels.
    pub surface: Color,
    /// The focused card or row.
    pub surface_hi: Color,
    pub border: Color,
    /// Behind panels.
    pub shadow: Color,
    pub text: Color,
    pub muted: Color,
    pub disabled: Color,
    /// Focus, confirm, success.
    pub accent: Color,
    /// Titles, warnings, going back.
    pub secondary: Color,
    /// Information, Steam, folders.
    pub info: Color,
    /// Errors and destructive actions.
    pub danger: Color,
}

const fn rgb(hex: u32) -> Color {
    Color::new(
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
        1.0,
    )
}

/// The first one is the default.
pub static THEMES: [Theme; 2] = [
    Theme {
        id: "midnight",
        name: "Midnight",
        typeface: Typeface::Barlow,
        radius: 6.0,
        glow: true,
        focus_width: 1.5,
        card_outline: false,
        bg_top: rgb(0x141b2e),
        bg_bottom: rgb(0x070a12),
        surface: rgb(0x151c2d),
        surface_hi: rgb(0x212b45),
        border: rgb(0x273250),
        shadow: Color::new(0.0, 0.0, 0.0, 0.55),
        text: rgb(0xe9edf6),
        muted: rgb(0x8b94ad),
        disabled: rgb(0x4b5370),
        accent: rgb(0x4fe0b0),
        secondary: rgb(0xf4b860),
        info: rgb(0x6ea8ff),
        danger: rgb(0xff6b6b),
    },
    // The original look: pure black, the pixel font, square frames.
    Theme {
        id: "retro",
        name: "Retro",
        typeface: Typeface::Pixel,
        radius: 0.0,
        glow: false,
        focus_width: 2.0,
        card_outline: true,
        bg_top: rgb(0x000000),
        bg_bottom: rgb(0x000000),
        surface: rgb(0x000000),
        surface_hi: rgb(0x0e0b08),
        border: rgb(0x41301e),
        shadow: Color::new(0.0, 0.0, 0.0, 0.0),
        text: rgb(0xe6ebf2),
        muted: rgb(0x6e7682),
        disabled: rgb(0x3d424a),
        accent: rgb(0x26a86c),
        secondary: rgb(0xa05210),
        info: rgb(0x569cd6),
        danger: rgb(0xd9554d),
    },
];

static CURRENT: AtomicUsize = AtomicUsize::new(0);

/// The theme with this id (any case); unset or unknown means the default.
pub fn find(id: Option<&str>) -> &'static Theme {
    id.and_then(|id| THEMES.iter().find(|t| t.id.eq_ignore_ascii_case(id.trim())))
        .unwrap_or(&THEMES[0])
}

/// Switch every screen to `theme`.
pub fn set(theme: &'static Theme) {
    let index = THEMES.iter().position(|t| t.id == theme.id).unwrap_or(0);
    CURRENT.store(index, Ordering::Relaxed);
}

pub fn current() -> &'static Theme {
    &THEMES[CURRENT.load(Ordering::Relaxed)]
}

pub fn bg() -> Color {
    current().bg_bottom
}
pub fn surface_hi() -> Color {
    current().surface_hi
}
pub fn border() -> Color {
    current().border
}
pub fn text() -> Color {
    current().text
}
pub fn muted() -> Color {
    current().muted
}
pub fn disabled() -> Color {
    current().disabled
}
pub fn accent() -> Color {
    current().accent
}
pub fn secondary() -> Color {
    current().secondary
}
pub fn info() -> Color {
    current().info
}
pub fn danger() -> Color {
    current().danger
}

/// A color by its role, looked up in the current theme when drawn: text kept
/// on screen across a theme change follows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Muted,
    Accent,
    Secondary,
    Danger,
}

impl Tone {
    fn of(self, theme: &Theme) -> Color {
        match self {
            Tone::Muted => theme.muted,
            Tone::Accent => theme.accent,
            Tone::Secondary => theme.secondary,
            Tone::Danger => theme.danger,
        }
    }

    pub fn color(self) -> Color {
        self.of(current())
    }
}

/// `color` at `alpha` opacity.
pub fn fade(color: Color, alpha: f32) -> Color {
    Color { a: alpha, ..color }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_reads_any_case_and_defaults_to_the_first() {
        assert_eq!(find(Some("RETRO")).id, "retro");
        assert_eq!(find(Some("nope")).id, THEMES[0].id);
        assert_eq!(find(None).id, THEMES[0].id);
    }

    #[test]
    fn set_switches_the_current_theme() {
        set(find(Some("retro")));
        assert_eq!(current().id, "retro");
        set(&THEMES[0]);
        assert_eq!(current().id, THEMES[0].id);
    }

    #[test]
    fn tones_take_their_color_from_the_theme() {
        let retro = find(Some("retro"));
        assert_eq!(Tone::Muted.of(retro), retro.muted);
        assert_eq!(Tone::Danger.of(&THEMES[0]), THEMES[0].danger);
    }

    #[test]
    fn theme_ids_are_unique() {
        let mut ids: Vec<&str> = THEMES.iter().map(|t| t.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), THEMES.len());
    }
}
