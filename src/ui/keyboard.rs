//! On-screen keyboard for typing with a gamepad. The physical keyboard works at
//! the same time: typed characters go straight in, Backspace deletes, Enter
//! accepts and Esc cancels.

use macroquad::prelude::*;

use super::engine::input::Action;
use super::engine::theme;
use super::{centered, panel, stroke_rect, Hint, Ui};

const ROWS: [&str; 4] = [
    "1234567890-=",
    "qwertyuiop[]",
    "asdfghjkl;'\\",
    "zxcvbnm,./`$",
];
const SHIFTED: [&str; 4] = [
    "!@#$%^&*()_+",
    "QWERTYUIOP{}",
    "ASDFGHJKL:\"|",
    "ZXCVBNM<>?~%",
];
const SPECIALS: [&str; 6] = ["SHIFT", "SPACE", "DEL", "CLEAR", "CANCEL", "OK"];

const KEY_H: f32 = 24.0;
const GAP: f32 = 3.0;

pub enum Outcome {
    Typing,
    Done(String),
    Cancelled,
}

pub struct Keyboard {
    title: String,
    value: String,
    secret: bool,
    row: usize,
    col: usize,
    shift: bool,
}

fn row_len(row: usize) -> usize {
    if row < ROWS.len() {
        ROWS[row].chars().count()
    } else {
        SPECIALS.len()
    }
}

impl Keyboard {
    pub fn new(title: &str, value: &str, secret: bool) -> Keyboard {
        Keyboard {
            title: title.to_string(),
            value: value.to_string(),
            secret,
            row: 1,
            col: 0,
            shift: false,
        }
    }

    fn char_at(&self, row: usize, col: usize) -> Option<char> {
        let rows = if self.shift { SHIFTED } else { ROWS };
        rows.get(row)?.chars().nth(col)
    }

    /// Move to another row, keeping the horizontal position proportional.
    fn move_row(&mut self, row: usize) {
        let ratio = (self.col as f32 + 0.5) / row_len(self.row) as f32;
        self.row = row;
        self.col = ((ratio * row_len(row) as f32) as usize).min(row_len(row) - 1);
    }

    /// Press the key at (row, col).
    fn press(&mut self, row: usize, col: usize) -> Outcome {
        if let Some(c) = self.char_at(row, col) {
            self.value.push(c);
            return Outcome::Typing;
        }
        match SPECIALS[col] {
            "SHIFT" => self.shift = !self.shift,
            "SPACE" => self.value.push(' '),
            "DEL" => {
                self.value.pop();
            }
            "CLEAR" => self.value.clear(),
            "CANCEL" => return Outcome::Cancelled,
            _ => return Outcome::Done(self.value.clone()),
        }
        Outcome::Typing
    }

    pub fn update(&mut self, ui: &mut Ui, area: Rect) -> Outcome {
        ui.input.text_mode = true;
        self.value.extend(ui.input.typed());
        if is_key_pressed(KeyCode::Backspace) || ui.action(Action::Back) {
            self.value.pop();
        }
        if is_key_pressed(KeyCode::Enter)
            || is_key_pressed(KeyCode::KpEnter)
            || ui.action(Action::Menu)
        {
            return Outcome::Done(self.value.clone());
        }
        if is_key_pressed(KeyCode::Escape) {
            return Outcome::Cancelled;
        }
        if ui.action(Action::Alt) {
            self.shift = !self.shift;
        }
        if ui.action(Action::Edit) {
            self.value.push(' ');
        }

        let last_row = ROWS.len();
        if ui.action(Action::Up) {
            self.move_row(if self.row == 0 {
                last_row
            } else {
                self.row - 1
            });
        }
        if ui.action(Action::Down) {
            self.move_row(if self.row == last_row {
                0
            } else {
                self.row + 1
            });
        }
        if ui.action(Action::Left) {
            self.col = (self.col + row_len(self.row) - 1) % row_len(self.row);
        }
        if ui.action(Action::Right) {
            self.col = (self.col + 1) % row_len(self.row);
        }
        if ui.action(Action::Confirm) {
            return self.press(self.row, self.col);
        }
        if let Some(p) = ui.click() {
            if let Some(&(row, col, _)) =
                self.key_rects(area).iter().find(|(_, _, r)| r.contains(p))
            {
                (self.row, self.col) = (row, col);
                return self.press(row, col);
            }
        }
        Outcome::Typing
    }

    fn frame(&self, area: Rect) -> Rect {
        let rows = ROWS.len() as f32 + 1.0;
        centered(area, 460.0, 76.0 + rows * (KEY_H + GAP))
    }

    fn key_rects(&self, area: Rect) -> Vec<(usize, usize, Rect)> {
        let frame = self.frame(area);
        let inner_w = frame.w - 24.0;
        let top = frame.y + 70.0;
        (0..=ROWS.len())
            .flat_map(|row| {
                let n = row_len(row);
                let w = ((inner_w - GAP * (n as f32 - 1.0)) / n as f32).floor();
                let used = w * n as f32 + GAP * (n as f32 - 1.0);
                let x0 = (frame.x + (frame.w - used) / 2.0).round();
                (0..n).map(move |col| {
                    (
                        row,
                        col,
                        Rect::new(
                            x0 + col as f32 * (w + GAP),
                            top + row as f32 * (KEY_H + GAP),
                            w,
                            KEY_H,
                        ),
                    )
                })
            })
            .collect()
    }

    pub fn draw(&self, ui: &Ui, area: Rect) {
        let frame = self.frame(area);
        panel(frame, theme::ACCENT);
        ui.text(
            &ui.truncate(&self.title, frame.w - 24.0),
            frame.x + 12.0,
            frame.y + 22.0,
            theme::AMBER,
        );

        let field = Rect::new(frame.x + 12.0, frame.y + 32.0, frame.w - 24.0, 26.0);
        stroke_rect(field, 1.0, theme::BORDER);
        let shown = if self.secret {
            "*".repeat(self.value.chars().count())
        } else {
            self.value.clone()
        };
        let caret = if (get_time() * 2.0) as i64 % 2 == 0 {
            "_"
        } else {
            " "
        };
        let line = ui.truncate_start(&format!("{shown}{caret}"), field.w - 12.0);
        ui.text(&line, field.x + 6.0, field.y + 19.0, theme::TEXT);

        for (row, col, r) in self.key_rects(area) {
            let focused = (row, col) == (self.row, self.col);
            let label = match self.char_at(row, col) {
                Some(c) => c.to_string(),
                None => SPECIALS[col].to_string(),
            };
            let active = label == "SHIFT" && self.shift;
            if focused {
                draw_rectangle(r.x, r.y, r.w, r.h, theme::PANEL);
            }
            let border = if focused {
                theme::ACCENT
            } else if active {
                theme::AMBER
            } else {
                theme::BORDER
            };
            stroke_rect(r, if focused { 2.0 } else { 1.0 }, border);
            let color = if focused { theme::TEXT } else { theme::MUTED };
            let label = ui.truncate(&label, r.w - 4.0);
            ui.text(
                &label,
                (r.x + (r.w - ui.measure(&label)) / 2.0).round(),
                r.y + 18.0,
                color,
            );
        }
    }

    pub fn hints(&self, ui: &Ui) -> Vec<Hint> {
        if !ui.input.gamepad_last() {
            return vec![
                Hint::new("ENTER", "OK", theme::ACCENT),
                Hint::new("ESC", "Cancel", theme::AMBER),
                Hint::new("BKSP", "Delete", theme::BORDER),
            ];
        }
        vec![
            Hint::new("A", "Type", theme::ACCENT),
            Hint::new("B", "Delete", theme::AMBER),
            Hint::new("X", "Space", theme::BORDER),
            Hint::new("Y", "Shift", theme::BORDER),
            Hint::new("START", "OK", theme::ACCENT),
        ]
    }
}
