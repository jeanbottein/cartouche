//! A modal choice: a message and a few options.

use macroquad::prelude::*;

use super::engine::input::Action;
use super::engine::theme;
use super::list::List;
use super::{centered, panel, Hint, Ui};

const ROW: f32 = 24.0;

pub enum Outcome {
    Open,
    Chosen(usize),
    Cancelled,
}

pub struct Dialog {
    title: String,
    message: String,
    options: Vec<(String, Color)>,
    list: List,
}

impl Dialog {
    pub fn new(title: &str, message: &str, options: &[(&str, Color)]) -> Dialog {
        Dialog {
            title: title.to_string(),
            message: message.to_string(),
            options: options
                .iter()
                .map(|(label, color)| (label.to_string(), *color))
                .collect(),
            list: List::default(),
        }
    }

    fn layout(&self, ui: &Ui, area: Rect) -> (Rect, Vec<String>, Rect) {
        let width = 380.0f32.min(area.w - 20.0);
        let lines = ui.wrap(&self.message, width - 24.0);
        let height =
            40.0 + lines.len() as f32 * 20.0 + 10.0 + self.options.len() as f32 * ROW + 10.0;
        let frame = centered(area, width, height);
        let options_top = frame.y + 40.0 + lines.len() as f32 * 20.0 + 8.0;
        let list = Rect::new(
            frame.x + 12.0,
            options_top,
            frame.w - 24.0,
            self.options.len() as f32 * ROW,
        );
        (frame, lines, list)
    }

    pub fn update(&mut self, ui: &Ui, area: Rect) -> Outcome {
        if ui.action(Action::Back) {
            return Outcome::Cancelled;
        }
        let (_, _, list) = self.layout(ui, area);
        if self
            .list
            .update(ui, self.options.len(), list, ROW, |_| true)
        {
            return Outcome::Chosen(self.list.cursor);
        }
        Outcome::Open
    }

    pub fn draw(&self, ui: &Ui, area: Rect) {
        let (frame, lines, list) = self.layout(ui, area);
        panel(frame, theme::AMBER);
        ui.text(
            &ui.truncate(&self.title, frame.w - 24.0),
            frame.x + 12.0,
            frame.y + 24.0,
            theme::AMBER,
        );
        for (i, line) in lines.iter().enumerate() {
            ui.text(
                line,
                frame.x + 12.0,
                frame.y + 46.0 + i as f32 * 20.0,
                theme::TEXT,
            );
        }
        for (i, r) in self.list.visible(self.options.len(), list, ROW) {
            let (label, color) = &self.options[i];
            let focused = i == self.list.cursor;
            if focused {
                draw_rectangle(r.x, r.y, r.w, r.h - 2.0, theme::PANEL);
                draw_rectangle(r.x, r.y, 3.0, r.h - 2.0, *color);
            }
            ui.text(
                label,
                r.x + 10.0,
                r.y + 17.0,
                if focused { theme::TEXT } else { *color },
            );
        }
    }

    pub fn hints(&self, ui: &Ui) -> Vec<Hint> {
        vec![
            ui.hint(Action::Confirm, "Choose", theme::ACCENT),
            ui.hint(Action::Back, "Cancel", theme::AMBER),
        ]
    }
}
