//! Status: run pipeline groups on a background thread with per-phase progress,
//! a live log (every log line of the app) and cancel between phases.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use log::Level;
use macroquad::prelude::*;

use super::engine::canvas::Layout;
use super::engine::input::Action;
use super::engine::theme;
use super::list::List;
use super::{stroke_rect, Hint, Ui};
use crate::core::config::Config;
use crate::core::logging::Line;
use crate::core::pipeline::{self, Group, Phase, Pipeline};

const MAX_LOG_LINES: usize = 500;
const ROW: f32 = 22.0;
const LOG_LINE: f32 = 18.0;

const ACTIONS: [(&str, Option<Group>); 6] = [
    ("Run all", Some(Group::All)),
    ("Parse games", Some(Group::Parse)),
    ("Steam sync", Some(Group::Steam)),
    ("Back up saves", Some(Group::Backup)),
    ("Apply patches", Some(Group::Patch)),
    ("Cancel run", None),
];

#[derive(Clone, Copy, PartialEq)]
enum PhaseState {
    Pending,
    Running,
    Done,
}

enum Msg {
    Started(Phase),
    Finished(Phase),
    Done { completed: bool },
}

struct Run {
    cancel: Arc<AtomicBool>,
    rx: Receiver<Msg>,
    handle: JoinHandle<()>,
}

/// A pipeline run just ended (the library should be reloaded).
pub struct Finished;

pub struct Status {
    list: List,
    log: VecDeque<Line>,
    /// Log lines scrolled up from the bottom.
    log_offset: usize,
    run: Option<Run>,
    phases: Vec<(Phase, PhaseState)>,
    state: (String, Color),
    last_run: Option<Instant>,
}

impl Status {
    pub fn new() -> Status {
        Status {
            list: List::default(),
            log: VecDeque::new(),
            log_offset: 0,
            run: None,
            phases: Group::All
                .phases()
                .iter()
                .map(|&p| (p, PhaseState::Pending))
                .collect(),
            state: ("Ready.".into(), theme::MUTED),
            last_run: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.run.is_some()
    }

    pub fn push_log(&mut self, line: Line) {
        for text in line.text.lines() {
            self.log.push_back(Line {
                level: line.level,
                text: text.to_string(),
            });
        }
        while self.log.len() > MAX_LOG_LINES {
            self.log.pop_front();
        }
    }

    fn start(&mut self, group: Group, cfg: &Config) {
        if self.run.is_some() {
            return;
        }
        self.phases = group
            .phases()
            .iter()
            .map(|&p| (p, PhaseState::Pending))
            .collect();
        self.state = (format!("Running '{}'...", group.name()), theme::AMBER);
        self.log_offset = 0;
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let (flag, cfg) = (cancel.clone(), cfg.clone());
        let handle = std::thread::spawn(move || {
            let completed = Pipeline::new(&cfg).run_group(group, &flag, |event| {
                let _ = tx.send(match event {
                    pipeline::Event::Started(p) => Msg::Started(p),
                    pipeline::Event::Finished(p) => Msg::Finished(p),
                });
            });
            let _ = tx.send(Msg::Done { completed });
        });
        self.run = Some(Run { cancel, rx, handle });
    }

    fn set_phase(&mut self, phase: Phase, state: PhaseState) {
        if let Some(entry) = self.phases.iter_mut().find(|(p, _)| *p == phase) {
            entry.1 = state;
        }
    }

    /// Apply progress from the pipeline thread; `Some` once a run ended.
    pub fn poll(&mut self) -> Option<Finished> {
        let run = self.run.as_ref()?;
        let messages: Vec<Msg> = run.rx.try_iter().collect();
        let crashed = messages.is_empty() && run.handle.is_finished();
        let mut outcome = crashed.then_some(None);
        for msg in messages {
            match msg {
                Msg::Started(p) => self.set_phase(p, PhaseState::Running),
                Msg::Finished(p) => self.set_phase(p, PhaseState::Done),
                Msg::Done { completed } => outcome = Some(Some(completed)),
            }
        }
        let outcome = outcome?;
        self.run = None;
        self.last_run = Some(Instant::now());
        self.state = match outcome {
            Some(true) => ("Completed.".into(), theme::ACCENT),
            Some(false) => ("Cancelled.".into(), theme::AMBER),
            None => (
                "Error: the run stopped unexpectedly (see log).".into(),
                theme::RED,
            ),
        };
        Some(Finished)
    }

    /// (run actions and phases, log).
    fn layout(&self, ui: &Ui, area: Rect) -> (Rect, Rect) {
        let actions_h = ACTIONS.len() as f32 * ROW;
        let stacked_h = match ui.canvas.layout() {
            Layout::Wide => return Status::columns(area),
            Layout::Tall => 64.0 + actions_h + self.phases_height(),
            Layout::Narrow => 64.0 + actions_h,
        };
        let top = Rect::new(area.x, area.y, area.w, stacked_h);
        (
            top,
            Rect::new(
                area.x,
                top.bottom() + 6.0,
                area.w,
                area.bottom() - top.bottom() - 6.0,
            ),
        )
    }

    /// Actions and phases on the left, the log on the right (widescreen).
    fn columns(area: Rect) -> (Rect, Rect) {
        let left_w = (area.w * 0.4).floor();
        (
            Rect::new(area.x, area.y, left_w, area.h),
            Rect::new(
                area.x + left_w + 12.0,
                area.y,
                area.w - left_w - 12.0,
                area.h,
            ),
        )
    }

    /// Room for the progress bar and every phase under the actions.
    fn phases_height(&self) -> f32 {
        26.0 + self.phases.len() as f32 * LOG_LINE
    }

    fn actions_area(left: Rect) -> Rect {
        Rect::new(left.x, left.y + 56.0, left.w, ACTIONS.len() as f32 * ROW)
    }

    pub fn update(&mut self, ui: &Ui, area: Rect, cfg: Option<&Config>) {
        let (left, log) = self.layout(ui, area);
        if ui.action(Action::Left) {
            self.log_offset = (self.log_offset + 5).min(self.log.len().saturating_sub(1));
        }
        if ui.action(Action::Right) {
            self.log_offset = self.log_offset.saturating_sub(5);
        }
        let (mx, my) = mouse_position();
        let over_log = ui
            .canvas
            .screen_to_canvas(vec2(mx, my))
            .is_some_and(|p| log.contains(p));
        let wheel = mouse_wheel().1;
        if over_log && wheel != 0.0 {
            self.log_offset = if wheel > 0.0 {
                (self.log_offset + 3).min(self.log.len().saturating_sub(1))
            } else {
                self.log_offset.saturating_sub(3)
            };
            return;
        }
        if !self
            .list
            .update(ui, ACTIONS.len(), Status::actions_area(left), ROW, |_| true)
        {
            return;
        }
        match (ACTIONS[self.list.cursor].1, cfg) {
            (Some(group), Some(cfg)) => self.start(group, cfg),
            (Some(_), None) => self.state = ("No configuration loaded.".into(), theme::RED),
            (None, _) => {
                if let Some(run) = &self.run {
                    run.cancel.store(true, Ordering::Relaxed);
                    self.state = ("Cancelling after this phase...".into(), theme::AMBER);
                }
            }
        }
    }

    pub fn draw(&self, ui: &Ui, area: Rect, game_count: usize) {
        let (left, log) = self.layout(ui, area);
        let last_run = match self.last_run {
            Some(t) => format!("last run {} min ago", t.elapsed().as_secs() / 60),
            None => "not run yet".into(),
        };
        ui.text(
            &ui.truncate(&format!("{game_count} game(s), {last_run}"), left.w),
            left.x,
            left.y + 18.0,
            theme::MUTED,
        );
        ui.text(
            &ui.truncate(&self.state.0, left.w),
            left.x,
            left.y + 40.0,
            self.state.1,
        );

        for (i, r) in self
            .list
            .visible(ACTIONS.len(), Status::actions_area(left), ROW)
        {
            let (label, group) = ACTIONS[i];
            let focused = i == self.list.cursor;
            let disabled =
                (group.is_some() && self.run.is_some()) || (group.is_none() && self.run.is_none());
            if focused {
                draw_rectangle(r.x, r.y, r.w, r.h - 2.0, theme::PANEL);
                draw_rectangle(r.x, r.y, 3.0, r.h - 2.0, theme::ACCENT);
            }
            let color = if disabled {
                theme::BORDER
            } else if focused {
                theme::TEXT
            } else {
                theme::MUTED
            };
            ui.text(label, r.x + 10.0, r.y + 16.0, color);
        }

        let phases_top = Status::actions_area(left).bottom() + 10.0;
        if ui.canvas.layout() != Layout::Narrow {
            self.draw_phases(
                ui,
                Rect::new(left.x, phases_top, left.w, left.bottom() - phases_top),
            );
        }
        self.draw_log(ui, log);
    }

    fn draw_phases(&self, ui: &Ui, area: Rect) {
        let done = self
            .phases
            .iter()
            .filter(|(_, s)| *s == PhaseState::Done)
            .count();
        let bar = Rect::new(area.x, area.y, area.w, 8.0);
        stroke_rect(bar, 1.0, theme::BORDER);
        let fill = (bar.w - 2.0) * done as f32 / self.phases.len().max(1) as f32;
        draw_rectangle(bar.x + 1.0, bar.y + 1.0, fill, bar.h - 2.0, theme::ACCENT);
        let rows = ((area.h - 16.0) / LOG_LINE) as usize;
        for (i, (phase, state)) in self.phases.iter().take(rows).enumerate() {
            let (mark, color) = match state {
                PhaseState::Pending => ("  ", theme::BORDER),
                PhaseState::Running => ("> ", theme::AMBER),
                PhaseState::Done => ("* ", theme::ACCENT),
            };
            ui.text(
                &format!("{mark}{}", phase.label()),
                area.x,
                area.y + 28.0 + i as f32 * LOG_LINE,
                color,
            );
        }
    }

    fn draw_log(&self, ui: &Ui, area: Rect) {
        stroke_rect(area, 1.0, theme::BORDER);
        let inner_w = area.w - 12.0;
        let capacity = ((area.h - 8.0) / LOG_LINE) as usize;
        let color_of = |level: Level| match level {
            Level::Error => theme::RED,
            Level::Warn => theme::AMBER,
            _ => theme::TEXT,
        };
        // Newest at the bottom: wrap from the end until the panel is full.
        let mut rows: Vec<(String, Color)> = Vec::new();
        for line in self.log.iter().rev().skip(self.log_offset) {
            let wrapped = if line.text.trim().is_empty() {
                vec![String::new()]
            } else {
                ui.wrap(&line.text, inner_w)
            };
            for part in wrapped.into_iter().rev() {
                rows.push((ui.truncate(&part, inner_w), color_of(line.level)));
            }
            if rows.len() >= capacity {
                break;
            }
        }
        let shown = rows.len().min(capacity);
        for (slot, (text, color)) in rows.into_iter().take(capacity).rev().enumerate() {
            let y = area.bottom() - 6.0 - (shown - slot - 1) as f32 * LOG_LINE - 4.0;
            ui.text(&text, area.x + 6.0, y, color);
        }
        if self.log_offset > 0 {
            let note = format!("-{} lines", self.log_offset);
            ui.text(
                &note,
                area.right() - 6.0 - ui.measure(&note),
                area.y + 18.0,
                theme::BLUE,
            );
        }
    }

    pub fn hints(&self, ui: &Ui) -> Vec<Hint> {
        vec![
            ui.hint(Action::Confirm, "Select", theme::ACCENT),
            ui.hint(Action::Left, "Scroll log", theme::BORDER),
        ]
    }
}
