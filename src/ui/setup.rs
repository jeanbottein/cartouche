//! First run in the UI: no cartridge next to the app yet. Explain the
//! portable layout, then create it there (`system/`, `games/`, `mods/`,
//! `saves-copies/`, `saves-links/`). No choices: folders are fixed.

use std::path::{Path, PathBuf};

use macroquad::prelude::*;

use super::engine::theme;
use super::form::{Event as FormEvent, Field, Form};
use super::{Hint, Ui};
use crate::core::app::{label, DISPLAY_NAME, FOLDERS, WORKSPACE_DIR};
use crate::core::init;

pub enum Outcome {
    Created(PathBuf),
    Quit,
}

pub struct Setup {
    /// Where the cartridge goes: next to the app, or the folder given on the
    /// command line.
    dir: PathBuf,
    form: Form,
    message: Option<String>,
    /// What creating a cartridge here means (and its pitfalls).
    advice: Vec<String>,
}

const LINE: f32 = 20.0;

fn fields(dir: &Path) -> Vec<Field> {
    // The folder name, not the full path: it is the cartridge's label, and
    // fits on the help line.
    let help = format!(
        "Creates {WORKSPACE_DIR}/, {} in {}/",
        FOLDERS.join("/, ") + "/",
        label(dir)
    );
    vec![
        Field::section(&format!("WELCOME TO {DISPLAY_NAME}")),
        Field::button("create", "Create the cartridge here", "", theme::ACCENT).with_help(&help),
        Field::button("quit", "Quit", "", theme::MUTED),
    ]
}

impl Setup {
    pub fn new(dir: PathBuf) -> Setup {
        Setup {
            form: Form::new(fields(&dir)),
            advice: init::folder_advice(&dir),
            dir,
            message: None,
        }
    }

    fn create(&mut self) -> Option<Outcome> {
        match init::initialize(&self.dir) {
            Ok(config) => Some(Outcome::Created(config)),
            Err(e) => {
                self.message = Some(format!("Setup failed: {e}"));
                None
            }
        }
    }

    /// Wrapped advice lines, warnings flagged, for the panel under the form.
    fn advice_lines(&self, ui: &Ui, width: f32) -> Vec<(String, bool)> {
        self.advice
            .iter()
            .enumerate()
            .flat_map(|(i, text)| ui.wrap(text, width).into_iter().map(move |l| (l, i > 0)))
            .collect()
    }

    /// The form's rect: what remains above the advice and the message line.
    fn form_area(&self, ui: &Ui, area: Rect) -> Rect {
        let advice_h = self.advice_lines(ui, area.w).len() as f32 * LINE + 10.0;
        Rect::new(area.x, area.y, area.w, area.h - 22.0 - advice_h)
    }

    pub fn update(&mut self, ui: &mut Ui, area: Rect) -> Option<Outcome> {
        let form_area = self.form_area(ui, area);
        match self.form.update(ui, form_area)? {
            FormEvent::Pressed(id) if id == "create" => self.create(),
            FormEvent::Pressed(id) if id == "quit" => Some(Outcome::Quit),
            _ => None,
        }
    }

    pub fn draw(&self, ui: &Ui, area: Rect) {
        if let Some(message) = &self.message {
            ui.text(
                &ui.truncate(message, area.w),
                area.x,
                area.bottom() - 4.0,
                theme::RED,
            );
        }
        let form_area = self.form_area(ui, area);
        self.form.draw(ui, form_area);
        let mut y = form_area.bottom() + 24.0;
        for (line, warning) in self.advice_lines(ui, area.w) {
            let color = if warning { theme::AMBER } else { theme::MUTED };
            ui.text(&line, area.x, y, color);
            y += LINE;
        }
    }

    pub fn hints(&self, ui: &Ui) -> Vec<Hint> {
        self.form.hints(ui)
    }
}
