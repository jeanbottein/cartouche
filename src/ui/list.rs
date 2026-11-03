//! A vertical list with a cursor, scrolled in whole rows (so rows never get
//! cut at the edges), driven by pad/keys, the mouse wheel and clicks.

use macroquad::prelude::*;

use super::engine::input::Action;
use super::Ui;

#[derive(Default)]
pub struct List {
    pub cursor: usize,
    /// First visible row.
    first: usize,
}

const WHEEL_ROWS: usize = 3;

fn rows_fit(area: Rect, step: f32) -> usize {
    ((area.h / step).floor() as usize).max(1)
}

impl List {
    /// Move the cursor to the nearest selectable row at or after `index`.
    pub fn select(&mut self, index: usize, len: usize, selectable: impl Fn(usize) -> bool) {
        let forward = (index..len).find(|&i| selectable(i));
        let backward = || (0..index.min(len)).rev().find(|&i| selectable(i));
        self.cursor = forward.or_else(backward).unwrap_or(0);
    }

    /// Navigate; true when the focused row was activated (Confirm, or a click on it).
    pub fn update(
        &mut self,
        ui: &Ui,
        len: usize,
        area: Rect,
        step: f32,
        selectable: impl Fn(usize) -> bool,
    ) -> bool {
        if len == 0 {
            return false;
        }
        let mut moved = false;
        if ui.action(Action::Down) {
            if let Some(i) = (self.cursor + 1..len).find(|&i| selectable(i)) {
                self.cursor = i;
            }
            moved = true;
        }
        if ui.action(Action::Up) {
            if let Some(i) = (0..self.cursor).rev().find(|&i| selectable(i)) {
                self.cursor = i;
            }
            moved = true;
        }

        let visible = rows_fit(area, step);
        let max_first = len.saturating_sub(visible);
        let wheel = mouse_wheel().1;
        if wheel > 0.0 {
            self.first = self.first.saturating_sub(WHEEL_ROWS);
        } else if wheel < 0.0 {
            self.first = (self.first + WHEEL_ROWS).min(max_first);
        }

        let mut activated = ui.action(Action::Confirm);
        if let Some(p) = ui.click().filter(|p| area.contains(*p)) {
            let index = self.first + ((p.y - area.y) / step) as usize;
            if index < len && selectable(index) {
                activated |= index == self.cursor;
                self.cursor = index;
            }
        }

        if moved {
            self.reveal(visible);
        }
        self.first = self.first.min(max_first);
        activated
    }

    /// Scroll so the cursor is on screen.
    fn reveal(&mut self, visible: usize) {
        if self.cursor < self.first {
            // Show a section header sitting just above the cursor too.
            self.first = self.cursor.saturating_sub(1);
        } else if self.cursor >= self.first + visible {
            self.first = self.cursor + 1 - visible;
        }
    }

    /// Visible rows with their rects.
    pub fn visible(&self, len: usize, area: Rect, step: f32) -> Vec<(usize, Rect)> {
        let count = rows_fit(area, step);
        (self.first..len.min(self.first + count))
            .enumerate()
            .map(|(slot, i)| {
                (
                    i,
                    Rect::new(area.x, area.y + slot as f32 * step, area.w, step),
                )
            })
            .collect()
    }
}
