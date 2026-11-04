//! A scrolling list of labelled fields (text, secret, on/off, choice, path,
//! buttons, section headers) that edits its own values through the on-screen
//! keyboard and the file browser, and reports changes and button presses.

use std::path::{Path, PathBuf};

use macroquad::prelude::*;

use super::browser::{self, Browser};
use super::engine::input::Action;
use super::engine::theme;
use super::keyboard::{self, Keyboard};
use super::list::List;
use super::{Hint, Ui};
use crate::core::paths;

const ROW: f32 = 24.0;
const HELP_H: f32 = 44.0;

#[derive(Clone)]
pub enum Kind {
    Section,
    Text,
    Secret,
    /// "True"/"False"; empty means `default`.
    Flag {
        default: bool,
    },
    Choice(&'static [&'static str]),
    /// Relative values are relative to `base`.
    Path {
        dirs_only: bool,
        base: PathBuf,
    },
    Button(Color),
}

#[derive(Clone)]
pub struct Field {
    pub id: String,
    pub label: String,
    pub value: String,
    pub kind: Kind,
    pub help: String,
}

impl Field {
    fn new(id: &str, label: &str, value: &str, kind: Kind) -> Field {
        Field {
            id: id.to_string(),
            label: label.to_string(),
            value: value.to_string(),
            kind,
            help: String::new(),
        }
    }

    pub fn section(label: &str) -> Field {
        Field::new("", label, "", Kind::Section)
    }

    pub fn text(id: &str, label: &str, value: &str) -> Field {
        Field::new(id, label, value, Kind::Text)
    }

    pub fn secret(id: &str, label: &str, value: &str) -> Field {
        Field::new(id, label, value, Kind::Secret)
    }

    pub fn flag(id: &str, label: &str, value: &str, default: bool) -> Field {
        Field::new(id, label, value, Kind::Flag { default })
    }

    pub fn choice(id: &str, label: &str, value: &str, options: &'static [&'static str]) -> Field {
        Field::new(id, label, value, Kind::Choice(options))
    }

    pub fn path(id: &str, label: &str, value: &str, dirs_only: bool, base: &Path) -> Field {
        Field::new(
            id,
            label,
            value,
            Kind::Path {
                dirs_only,
                base: base.to_path_buf(),
            },
        )
    }

    /// A button; `value` is shown on the right as a note.
    pub fn button(id: &str, label: &str, value: &str, color: Color) -> Field {
        Field::new(id, label, value, Kind::Button(color))
    }

    pub fn with_help(mut self, help: &str) -> Field {
        self.help = help.to_string();
        self
    }

    fn selectable(&self) -> bool {
        !matches!(self.kind, Kind::Section)
    }

    pub fn is_on(&self) -> bool {
        match self.kind {
            Kind::Flag { default } if self.value.is_empty() => default,
            _ => self.value.eq_ignore_ascii_case("true"),
        }
    }

    fn display_value(&self) -> String {
        match &self.kind {
            Kind::Flag { .. } => if self.is_on() { "ON" } else { "OFF" }.to_string(),
            Kind::Choice(_) => format!(
                "< {} >",
                if self.value.is_empty() {
                    "-"
                } else {
                    &self.value
                }
            ),
            Kind::Secret if !self.value.is_empty() => {
                "*".repeat(self.value.chars().count().min(16))
            }
            _ if self.value.is_empty() && !matches!(self.kind, Kind::Button(_) | Kind::Section) => {
                "(not set)".to_string()
            }
            _ => self.value.clone(),
        }
    }
}

pub enum Event {
    Changed(String),
    Pressed(String),
}

enum Modal {
    Keyboard(Keyboard, usize),
    Browser(Browser, usize),
}

#[derive(Default)]
pub struct Form {
    pub fields: Vec<Field>,
    list: List,
    modal: Option<Modal>,
}

/// The browser's pick, relative to `base` when inside it.
fn picked_value(picked: &Path, base: &Path) -> String {
    match picked.strip_prefix(base) {
        Ok(rel) if !rel.as_os_str().is_empty() => paths::lossy(rel),
        Ok(_) => ".".to_string(),
        Err(_) => paths::lossy(picked),
    }
}

impl Form {
    pub fn new(fields: Vec<Field>) -> Form {
        let mut form = Form {
            fields,
            ..Default::default()
        };
        form.select(0);
        form
    }

    /// Replace the fields, keeping the cursor near where it was.
    pub fn set_fields(&mut self, fields: Vec<Field>) {
        self.fields = fields;
        self.select(self.list.cursor);
    }

    pub fn select(&mut self, index: usize) {
        let fields = &self.fields;
        self.list
            .select(index, fields.len(), |i| fields[i].selectable());
    }

    pub fn select_id(&mut self, id: &str) {
        if let Some(i) = self.fields.iter().position(|f| f.id == id) {
            self.select(i);
        }
    }

    pub fn get(&self, id: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.id == id)
    }

    pub fn value(&self, id: &str) -> &str {
        self.get(id).map_or("", |f| f.value.as_str())
    }

    pub fn focused(&self) -> Option<&Field> {
        self.fields.get(self.list.cursor)
    }

    pub fn has_modal(&self) -> bool {
        self.modal.is_some()
    }

    fn list_area(area: Rect) -> Rect {
        Rect::new(area.x, area.y, area.w, area.h - HELP_H)
    }

    pub fn update(&mut self, ui: &mut Ui, area: Rect) -> Option<Event> {
        if let Some(index) = self.modal.as_ref().map(modal_index) {
            return self.update_modal(ui, area, index);
        }
        let fields = &self.fields;
        let list_area = Form::list_area(area);
        let activated = self
            .list
            .update(ui, fields.len(), list_area, ROW, |i| fields[i].selectable());
        let index = self.list.cursor;
        let field = self.fields.get_mut(index)?;
        let id = field.id.clone();
        let (left, right) = (ui.action(Action::Left), ui.action(Action::Right));
        match &field.kind {
            Kind::Section => None,
            Kind::Button(_) => activated.then_some(Event::Pressed(id)),
            Kind::Flag { .. } if activated || left || right => {
                field.value = if field.is_on() { "False" } else { "True" }.to_string();
                Some(Event::Changed(id))
            }
            Kind::Choice(options) if activated || left || right => {
                let current = options.iter().position(|o| *o == field.value);
                let next = match (current, left) {
                    (Some(i), true) => (i + options.len() - 1) % options.len(),
                    (Some(i), false) => (i + 1) % options.len(),
                    (None, _) => 0,
                };
                field.value = options[next].to_string();
                Some(Event::Changed(id))
            }
            Kind::Path { dirs_only, base } if activated => {
                let current = base.join(paths::expand(&field.value));
                let title = format!("Choose {}", field.label.trim());
                self.modal = Some(Modal::Browser(
                    Browser::new(&title, &current, *dirs_only),
                    index,
                ));
                None
            }
            Kind::Text | Kind::Secret | Kind::Path { .. }
                if activated || ui.action(Action::Edit) =>
            {
                let secret = matches!(field.kind, Kind::Secret);
                self.modal = Some(Modal::Keyboard(
                    Keyboard::new(&field.label, &field.value, secret),
                    index,
                ));
                None
            }
            _ => None,
        }
    }

    fn update_modal(&mut self, ui: &mut Ui, area: Rect, index: usize) -> Option<Event> {
        let value = match self.modal.as_mut()? {
            Modal::Keyboard(kb, _) => match kb.update(ui, area) {
                keyboard::Outcome::Typing => return None,
                keyboard::Outcome::Cancelled => None,
                keyboard::Outcome::Done(value) => Some(value.trim().to_string()),
            },
            Modal::Browser(browser, _) => match browser.update(ui, area) {
                browser::Outcome::Browsing => return None,
                browser::Outcome::Cancelled => None,
                browser::Outcome::Picked(path) => match &self.fields[index].kind {
                    Kind::Path { base, .. } => Some(picked_value(&path, base)),
                    _ => Some(paths::lossy(&path)),
                },
            },
        };
        self.modal = None;
        let field = self.fields.get_mut(index)?;
        match value {
            Some(value) if value != field.value => {
                field.value = value;
                Some(Event::Changed(field.id.clone()))
            }
            _ => None,
        }
    }

    pub fn draw(&self, ui: &Ui, area: Rect) {
        let list_area = Form::list_area(area);
        let label_w = (area.w * 0.42).floor();
        for (i, r) in self.list.visible(self.fields.len(), list_area, ROW) {
            self.draw_row(ui, &self.fields[i], r, label_w, i == self.list.cursor);
        }

        if let Some(field) = self.focused().filter(|f| !f.help.is_empty()) {
            let top = area.bottom() - HELP_H + 6.0;
            draw_rectangle(area.x, top - 4.0, area.w, 1.0, theme::BORDER);
            for (n, line) in ui
                .wrap(&field.help, area.w - 8.0)
                .iter()
                .take(2)
                .enumerate()
            {
                ui.text(
                    line,
                    area.x + 4.0,
                    top + 15.0 + n as f32 * 18.0,
                    theme::MUTED,
                );
            }
        }

        match &self.modal {
            Some(Modal::Keyboard(kb, _)) => kb.draw(ui, area),
            Some(Modal::Browser(browser, _)) => browser.draw(ui, area),
            None => {}
        }
    }

    fn draw_row(&self, ui: &Ui, field: &Field, r: Rect, label_w: f32, focused: bool) {
        let baseline = r.y + 17.0;
        if let Kind::Section = field.kind {
            ui.text(&ui.truncate(&field.label, r.w), r.x, baseline, theme::AMBER);
            draw_rectangle(r.x, r.y + r.h - 3.0, r.w, 1.0, theme::BORDER);
            return;
        }
        if focused {
            draw_rectangle(r.x, r.y, r.w, r.h - 2.0, theme::PANEL);
            draw_rectangle(r.x, r.y, 3.0, r.h - 2.0, theme::ACCENT);
        }
        let value = field.display_value();
        let value_color = match field.kind {
            Kind::Flag { .. } if field.is_on() => theme::ACCENT,
            _ if field.value.is_empty() => theme::MUTED,
            _ => {
                if focused {
                    theme::TEXT
                } else {
                    theme::BLUE
                }
            }
        };
        if let Kind::Button(color) = field.kind {
            let label = ui.truncate(&format!("> {}", field.label), r.w - 16.0);
            ui.text(
                &label,
                r.x + 8.0,
                baseline,
                if focused { theme::TEXT } else { color },
            );
            let note_x = r.x + 16.0 + ui.measure(&label);
            let note = ui.truncate_start(&value, r.right() - note_x - 6.0);
            ui.text(
                &note,
                r.right() - 6.0 - ui.measure(&note),
                baseline,
                theme::MUTED,
            );
            return;
        }
        let label_color = if focused { theme::TEXT } else { theme::MUTED };
        ui.text(
            &ui.truncate(&field.label, label_w - 14.0),
            r.x + 8.0,
            baseline,
            label_color,
        );
        let value_w = r.w - label_w - 6.0;
        let shown = match field.kind {
            Kind::Path { .. } => ui.truncate_start(&value, value_w),
            _ => ui.truncate(&value, value_w),
        };
        ui.text(&shown, r.x + label_w, baseline, value_color);
    }

    pub fn hints(&self, ui: &Ui) -> Vec<Hint> {
        match &self.modal {
            Some(Modal::Keyboard(kb, _)) => return kb.hints(ui),
            Some(Modal::Browser(browser, _)) => return browser.hints(ui),
            None => {}
        }
        let Some(field) = self.focused() else {
            return Vec::new();
        };
        match field.kind {
            Kind::Section => Vec::new(),
            Kind::Button(_) => vec![ui.hint(Action::Confirm, "Select", theme::ACCENT)],
            Kind::Flag { .. } => vec![ui.hint(Action::Confirm, "Toggle", theme::ACCENT)],
            Kind::Choice(_) => vec![ui.hint(Action::Left, "Change", theme::ACCENT)],
            Kind::Path { .. } => vec![
                ui.hint(Action::Confirm, "Browse", theme::ACCENT),
                ui.hint(Action::Edit, "Type", theme::BLUE),
            ],
            Kind::Text | Kind::Secret => vec![ui.hint(Action::Confirm, "Edit", theme::ACCENT)],
        }
    }
}

fn modal_index(modal: &Modal) -> usize {
    match modal {
        Modal::Keyboard(_, i) | Modal::Browser(_, i) => *i,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_are_relative_inside_the_base() {
        assert_eq!(
            picked_value(Path::new("/ws/games"), Path::new("/ws")),
            "games"
        );
        assert_eq!(picked_value(Path::new("/ws"), Path::new("/ws")), ".");
        assert_eq!(
            picked_value(Path::new("/other/x"), Path::new("/ws")),
            "/other/x"
        );
    }

    #[test]
    fn flags_fall_back_to_their_default() {
        assert!(Field::flag("k", "K", "", true).is_on());
        assert!(!Field::flag("k", "K", "False", true).is_on());
        assert!(Field::flag("k", "K", "TRUE", false).is_on());
    }
}
