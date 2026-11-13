//! The game info block of the side panels (Library and Network): the cover at
//! the left, with the short facts beside it (year and players, compatibility
//! and platforms, a status line, the progress of a running transfer); the
//! title and the description below, across the whole panel.

use macroquad::prelude::*;

use super::engine::theme;
use super::jobs::Job;
use super::{style, Ui};
use crate::core::models::Target;
use crate::core::scanner::{self, Compatibility};

const LINE: f32 = 20.0;
const SMALL_LINE: f32 = 17.0;
const TITLE_LINE: f32 = 25.0;
const TITLE_LINES: usize = 2;
const GAP: f32 = 12.0;
/// Most of a side panel's height the cover takes.
const COVER_SHARE: f32 = 0.62;
/// Most of a side panel's width the cover takes: the facts beside it are short.
const COVER_WIDTH_SHARE: f32 = 0.55;
/// Width / height of a SteamGridDB cover (600×900).
const COVER_ASPECT: f32 = 2.0 / 3.0;

pub struct Info<'a> {
    pub title: &'a str,
    pub year: Option<u32>,
    pub players: &'a str,
    pub targets: &'a [Target],
    /// Where the game is and how big, e.g. `Installed · 1.2 GB`.
    pub status: String,
    pub description: &'a str,
    /// The cover is on its way.
    pub cover_loading: bool,
}

/// The parts of a side panel.
#[derive(Debug, PartialEq)]
pub struct Panel {
    pub cover: Rect,
    /// The short facts, beside the cover.
    pub facts: Rect,
    /// Title and description, under the cover.
    pub body: Rect,
}

pub fn split_panel(panel: Rect) -> Panel {
    let cover_w = (panel.h * COVER_SHARE * COVER_ASPECT)
        .min(panel.w * COVER_WIDTH_SHARE)
        .floor();
    let cover = Rect::new(panel.x, panel.y, cover_w, (cover_w / COVER_ASPECT).round());
    let facts_x = cover.right() + GAP;
    let body_y = cover.bottom() + GAP;
    Panel {
        cover,
        facts: Rect::new(facts_x, panel.y, panel.right() - facts_x, cover.h),
        body: Rect::new(panel.x, body_y, panel.w, panel.bottom() - body_y),
    }
}

fn compatibility_color(compatibility: Compatibility) -> Color {
    match compatibility {
        Compatibility::Native => theme::accent(),
        Compatibility::Proton => theme::info(),
        Compatibility::Unsupported => theme::danger(),
        Compatibility::Unknown => theme::muted(),
    }
}

fn players_label(players: &str) -> Option<String> {
    match players.trim() {
        "" => None,
        "1" => Some("1 player".into()),
        n => Some(format!("{n} players")),
    }
}

/// Verb, numbers and bar of a running job; returns the height used.
fn draw_job(ui: &Ui, area: Rect, y: f32, job: &Job) -> f32 {
    let label = if job.cancelling() {
        "Cancelling...".to_string()
    } else {
        format!("{} · {}", job.kind.verb(), job.summary())
    };
    ui.text(&ui.truncate(&label, area.w), area.x, y, theme::secondary());
    style::bar(
        Rect::new(area.x, y + 6.0, area.w, 6.0),
        job.fraction(),
        theme::secondary(),
    );
    LINE + 12.0
}

pub fn draw(ui: &Ui, panel: &Panel, info: &Info, job: Option<&Job>) {
    style::well(panel.cover);
    if info.cover_loading {
        let label = "loading...";
        let x = panel.cover.center().x - ui.measure_small(label) / 2.0;
        ui.small_text(label, x, panel.cover.center().y, theme::muted());
    }
    let facts_end = draw_facts(ui, panel.facts, info, job);
    let top = facts_end.max(panel.body.y);
    let body = Rect::new(panel.body.x, top, panel.body.w, panel.body.bottom() - top);
    let title_end = draw_title(ui, body, info.title);
    let rest = Rect::new(body.x, title_end, body.w, body.bottom() - title_end);
    draw_description(ui, rest, info.description);
}

/// The title from the top of `area`; returns where it ends.
fn draw_title(ui: &Ui, area: Rect, title: &str) -> f32 {
    let lines = ui.wrap_title(title, area.w, TITLE_LINES);
    for (i, line) in lines.iter().enumerate() {
        ui.title(
            line,
            area.x,
            area.y + 20.0 + i as f32 * TITLE_LINE,
            theme::text(),
        );
    }
    area.y + lines.len() as f32 * TITLE_LINE + 6.0
}

/// Year and players, compatibility, platforms and status; returns where
/// they end.
fn draw_facts(ui: &Ui, area: Rect, info: &Info, job: Option<&Job>) -> f32 {
    let mut y = area.y + 12.0;
    let facts: Vec<String> = info
        .year
        .map(|y| y.to_string())
        .into_iter()
        .chain(players_label(info.players))
        .collect();
    if !facts.is_empty() {
        let line = ui.truncate_small(&facts.join(" · "), area.w);
        ui.small_text(&line, area.x, y, theme::muted());
        y += SMALL_LINE;
    }

    let compatibility = scanner::compatibility(info.targets);
    let label = compatibility.label();
    let label_w = style::chip(
        ui,
        label,
        area.x,
        y + 2.0,
        compatibility_color(compatibility),
    ) + 6.0;
    let platforms = scanner::platforms(info.targets).join(" · ");
    let (x, w) = if label_w + ui.measure_small(&platforms) <= area.w {
        (area.x + label_w, area.w - label_w)
    } else {
        y += SMALL_LINE + 4.0;
        (area.x, area.w)
    };
    ui.small_text(
        &ui.truncate_small(&platforms, w),
        x,
        y + 2.0,
        theme::muted(),
    );
    y += SMALL_LINE + 4.0;

    ui.small_text(
        &ui.truncate_small(&info.status, area.w),
        area.x,
        y,
        theme::text(),
    );
    y += SMALL_LINE;

    if let Some(job) = job {
        y += draw_job(ui, area, y, job);
    }
    y
}

fn draw_description(ui: &Ui, area: Rect, description: &str) {
    let first = area.y + 14.0;
    let room = ((area.bottom() - first) / LINE + 1.0).floor().max(0.0) as usize;
    let lines = ui.wrap(description.trim(), area.w);
    let shown = lines.len().min(room);
    for (i, line) in lines.iter().take(shown).enumerate() {
        let text = if i + 1 == shown && shown < lines.len() {
            ui.truncate(&format!("{line}…"), area.w)
        } else {
            line.clone()
        };
        ui.text(&text, area.x, first + i as f32 * LINE, theme::muted());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cover_sits_at_the_left_with_the_facts_beside_it() {
        let panel = split_panel(Rect::new(400.0, 50.0, 360.0, 340.0));
        assert_eq!(panel.cover, Rect::new(400.0, 50.0, 140.0, 210.0));
        assert_eq!(panel.facts, Rect::new(552.0, 50.0, 208.0, 210.0));
        assert_eq!(panel.body, Rect::new(400.0, 272.0, 360.0, 118.0));
    }

    #[test]
    fn a_narrow_panel_keeps_room_beside_the_cover() {
        let panel = split_panel(Rect::new(0.0, 0.0, 200.0, 400.0));
        assert_eq!(panel.cover.w, 110.0);
        assert_eq!(panel.cover.h, 165.0);
    }
}
