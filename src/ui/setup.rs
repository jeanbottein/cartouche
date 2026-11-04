//! First run in the UI: choose where the workspace goes and which folders to
//! create, then write `system/config.txt`.

use std::path::{Path, PathBuf};

use macroquad::prelude::*;

use super::engine::theme;
use super::form::{Event as FormEvent, Field, Form};
use super::{Hint, Ui};
use crate::core::app::{app_name, exe_dir, WORKSPACE_DIR};
use crate::core::{init, paths};

pub enum Outcome {
    Created(PathBuf),
    Quit,
}

pub struct Setup {
    form: Form,
    message: Option<String>,
    /// What creating a workspace in the chosen folder means (and its pitfalls).
    advice: Vec<String>,
}

const LINE: f32 = 20.0;

fn advice_for(workspace: &str) -> Vec<String> {
    init::folder_advice(&paths::abspath(&paths::expand(workspace)))
}

fn fields(workspace: &str, previous: Option<&Form>) -> Vec<Field> {
    let base = paths::expand(workspace);
    let keep =
        |id: &str, default: &str| previous.map_or(default.to_string(), |f| f.value(id).to_string());
    let help = format!("A {WORKSPACE_DIR}/ folder is created here, with your settings.");
    let mut out = vec![
        Field::section(&format!("WELCOME TO {}", app_name().to_uppercase())),
        Field::path(
            "workspace",
            "Workspace folder",
            workspace,
            true,
            Path::new("/"),
        )
        .with_help(&help),
        Field::section("FOLDERS  (relative to the workspace, or absolute)"),
    ];
    for feature in &init::FEATURES {
        out.push(
            Field::flag(
                &format!("{}.on", feature.key),
                feature.label,
                &keep(&format!("{}.on", feature.key), "True"),
                true,
            )
            .with_help(feature.description),
        );
        out.push(
            Field::path(
                &format!("{}.path", feature.key),
                &format!("  {} folder", feature.label),
                &keep(&format!("{}.path", feature.key), feature.default),
                true,
                &base,
            )
            .with_help(feature.description),
        );
    }
    out.extend([
        Field::section(""),
        Field::button("create", "Create workspace", "", theme::ACCENT),
        Field::button("quit", "Quit", "", theme::MUTED),
    ]);
    out
}

impl Setup {
    pub fn new() -> Setup {
        // Portable first: next to the app (the current folder means nothing
        // when launched from a file manager or Steam).
        let next_to_app = paths::lossy(&exe_dir());
        Setup {
            form: Form::new(fields(&next_to_app, None)),
            advice: advice_for(&next_to_app),
            message: None,
        }
    }

    fn create(&mut self) -> Option<Outcome> {
        let parent = paths::abspath(&paths::expand(self.form.value("workspace")));
        let chosen: Vec<(String, String)> = init::FEATURES
            .iter()
            .filter(|f| {
                self.form
                    .get(&format!("{}.on", f.key))
                    .is_some_and(|field| field.is_on())
            })
            .map(|f| {
                let value = self.form.value(&format!("{}.path", f.key)).trim();
                (
                    f.key.to_string(),
                    if value.is_empty() {
                        f.default.to_string()
                    } else {
                        value.to_string()
                    },
                )
            })
            .collect();
        match init::initialize(&parent, &chosen) {
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
            FormEvent::Changed(id) if id == "workspace" => {
                let workspace = self.form.value("workspace").to_string();
                let rebuilt = fields(&workspace, Some(&self.form));
                self.form.set_fields(rebuilt);
                self.advice = advice_for(&workspace);
                None
            }
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
