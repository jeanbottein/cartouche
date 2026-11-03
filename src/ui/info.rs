//! The game info block of the side panels (Library and Network): title, year
//! and players, compatibility and platforms, a status line, the progress of a
//! running transfer, and the description.

use macroquad::prelude::*;

use super::engine::theme;
use super::jobs::Job;
use super::{stroke_rect, Ui};
use crate::core::models::Target;
use crate::core::scanner::{self, Compatibility};

const LINE: f32 = 20.0;
/// Share of a side panel's height given to the cover.
const COVER_SHARE: f32 = 0.5;

pub struct Info<'a> {
    pub title: &'a str,
    pub year: Option<u32>,
    pub players: &'a str,
    pub targets: &'a [Target],
    /// Where the game is and how big, e.g. `Installed · 1.2 GB`.
    pub status: String,
    pub description: &'a str,
}

/// (cover, info) parts of a side panel.
pub fn split_panel(panel: Rect) -> (Rect, Rect) {
    let cover_h = (panel.h * COVER_SHARE).floor();
    (
        Rect::new(panel.x, panel.y, panel.w, cover_h),
        Rect::new(
            panel.x,
            panel.y + cover_h + 8.0,
            panel.w,
            panel.h - cover_h - 8.0,
        ),
    )
}

fn compatibility_color(compatibility: Compatibility) -> Color {
    match compatibility {
        Compatibility::Native => theme::ACCENT,
        Compatibility::Proton => theme::BLUE,
        Compatibility::Unsupported => theme::RED,
        Compatibility::Unknown => theme::MUTED,
    }
}

fn players_label(players: &str) -> Option<String> {
    match players.trim() {
        "" => None,
        "1" => Some("1 player".into()),
        n => Some(format!("{n} players")),
    }
}

/// A thin progress bar.
pub fn bar(area: Rect, fraction: f32, color: Color) {
    draw_rectangle(area.x, area.y, area.w, area.h, theme::PANEL);
    draw_rectangle(
        area.x,
        area.y,
        (area.w * fraction.clamp(0.0, 1.0)).round(),
        area.h,
        color,
    );
}

/// Verb, numbers and bar of a running job; returns the height used.
fn draw_job(ui: &Ui, area: Rect, y: f32, job: &Job) -> f32 {
    let label = if job.cancelling() {
        "Cancelling...".to_string()
    } else {
        format!("{} · {}", job.kind.verb(), job.summary())
    };
    ui.text(&ui.truncate(&label, area.w), area.x, y, theme::AMBER);
    let track = Rect::new(area.x, y + 6.0, area.w, 6.0);
    bar(track, job.fraction(), theme::AMBER);
    stroke_rect(track, 1.0, theme::BORDER);
    LINE + 12.0
}

pub fn draw(ui: &Ui, area: Rect, info: &Info, job: Option<&Job>) {
    let mut y = area.y + 24.0;
    ui.heading(
        &ui.truncate_heading(info.title, area.w),
        area.x,
        y,
        theme::TEXT,
    );
    y += LINE + 8.0;

    let facts: Vec<String> = info
        .year
        .map(|y| y.to_string())
        .into_iter()
        .chain(players_label(info.players))
        .collect();
    if !facts.is_empty() {
        ui.text(
            &ui.truncate(&facts.join(" · "), area.w),
            area.x,
            y,
            theme::MUTED,
        );
        y += LINE;
    }

    let compatibility = scanner::compatibility(info.targets);
    let label = compatibility.label();
    ui.text(label, area.x, y, compatibility_color(compatibility));
    let label_w = ui.measure(label) + 12.0;
    let platforms = scanner::platforms(info.targets).join(" · ");
    ui.text(
        &ui.truncate(&platforms, area.w - label_w),
        area.x + label_w,
        y,
        theme::MUTED,
    );
    y += LINE;

    ui.text(&ui.truncate(&info.status, area.w), area.x, y, theme::TEXT);
    y += LINE + 6.0;

    if let Some(job) = job {
        y += draw_job(ui, area, y, job);
    }

    let room = ((area.bottom() - y) / LINE).floor().max(0.0) as usize;
    let lines = ui.wrap(info.description.trim(), area.w);
    let shown = lines.len().min(room);
    for (i, line) in lines.iter().take(shown).enumerate() {
        let text = if i + 1 == shown && shown < lines.len() {
            ui.truncate(&format!("{line}…"), area.w)
        } else {
            line.clone()
        };
        ui.text(&text, area.x, y + i as f32 * LINE, theme::MUTED);
    }
}
