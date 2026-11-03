//! Unified input layer (ENGINE-3).
//!
//! Collapses keyboard, gamepad (via `gilrs`, behind the `gamepad` feature) and —
//! for pointing — mouse into a single [`Action`] vocabulary so the rest of the app
//! never touches raw devices. Directional actions auto-repeat (initial delay then
//! steady rate); buttons fire on the press edge only. Tunables match the former
//! Dear PyGui controller layer.

use std::collections::{HashMap, HashSet};

#[cfg(feature = "gamepad")]
use gilrs::{Axis, Button, Gilrs};
use macroquad::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
    Menu,
    ToggleAspect,
    TabPrev,
    TabNext,
    /// Secondary action: edit / type a value (pad X, keyboard X).
    Edit,
    /// Tertiary action: e.g. shift on the on-screen keyboard (pad Y).
    Alt,
}

const ALL_ACTIONS: [Action; 12] = [
    Action::Up,
    Action::Down,
    Action::Left,
    Action::Right,
    Action::Confirm,
    Action::Back,
    Action::Menu,
    Action::ToggleAspect,
    Action::TabPrev,
    Action::TabNext,
    Action::Edit,
    Action::Alt,
];

const ARROW_KEYS: [(KeyCode, Action); 4] = [
    (KeyCode::Up, Action::Up),
    (KeyCode::Down, Action::Down),
    (KeyCode::Left, Action::Left),
    (KeyCode::Right, Action::Right),
];

const KEYS: [(KeyCode, Action); 17] = [
    (KeyCode::Up, Action::Up),
    (KeyCode::Down, Action::Down),
    (KeyCode::Left, Action::Left),
    (KeyCode::Right, Action::Right),
    (KeyCode::W, Action::Up),
    (KeyCode::S, Action::Down),
    (KeyCode::A, Action::Left),
    (KeyCode::D, Action::Right),
    (KeyCode::Enter, Action::Confirm),
    (KeyCode::Space, Action::Confirm),
    (KeyCode::Escape, Action::Back),
    (KeyCode::Backspace, Action::Back),
    (KeyCode::Tab, Action::ToggleAspect),
    (KeyCode::Q, Action::TabPrev),
    (KeyCode::E, Action::TabNext),
    (KeyCode::X, Action::Edit),
    (KeyCode::H, Action::Alt),
];

#[cfg(feature = "gamepad")]
const DEADZONE: f32 = 0.4;
const REPEAT_INITIAL: f64 = 0.35;
const REPEAT_RATE: f64 = 0.12;

fn is_repeatable(a: Action) -> bool {
    matches!(a, Action::Up | Action::Down | Action::Left | Action::Right)
}

pub struct Input {
    #[cfg(feature = "gamepad")]
    gilrs: Option<Gilrs>,
    down_since: HashMap<Action, f64>,
    next_repeat: HashMap<Action, f64>,
    fired: HashSet<Action>,
    /// Actions whose current press started while input was live (not suspended),
    /// so a hold carried over from a game can't count as a long-press here.
    armed: HashSet<Action>,
    /// Whether the gamepad (vs. keyboard) was used last — drives the button hints.
    gamepad_last: bool,
    /// While typing into a text field only the arrow keys map to actions, so
    /// letters, Enter, Space and Backspace reach the text instead.
    pub text_mode: bool,
    was_text_mode: bool,
    /// Characters typed this frame (for text fields).
    typed: Vec<char>,
}

impl Input {
    pub fn new() -> Self {
        Self {
            // No gamepad subsystem available -> keyboard/mouse still work.
            #[cfg(feature = "gamepad")]
            gilrs: Gilrs::new().ok(),
            down_since: HashMap::new(),
            next_repeat: HashMap::new(),
            fired: HashSet::new(),
            armed: HashSet::new(),
            gamepad_last: false,
            text_mode: false,
            was_text_mode: false,
            typed: Vec::new(),
        }
    }

    /// Poll devices and compute which actions fired this frame. Call once per frame.
    pub fn update(&mut self) {
        #[cfg(feature = "gamepad")]
        if let Some(g) = self.gilrs.as_mut() {
            // Pump events to refresh gamepad state.
            while g.next_event().is_some() {
                self.gamepad_last = true;
            }
        }
        if get_last_key_pressed().is_some() {
            self.gamepad_last = false;
        }

        self.typed.clear();
        while let Some(c) = get_char_pressed() {
            if !c.is_control() {
                self.typed.push(c);
            }
        }

        let now = get_time();
        let down = self.collect_down();
        self.fired.clear();
        if self.was_text_mode && !self.text_mode {
            // Keys still held from typing (Enter, Esc, ...) must not fire now.
            for &action in &down {
                self.down_since.entry(action).or_insert(now);
            }
        }
        self.was_text_mode = self.text_mode;

        for action in ALL_ACTIONS {
            if down.contains(&action) {
                if !self.down_since.contains_key(&action) {
                    // press edge
                    self.down_since.insert(action, now);
                    self.fired.insert(action);
                    self.armed.insert(action);
                    if is_repeatable(action) {
                        self.next_repeat.insert(action, now + REPEAT_INITIAL);
                    }
                } else if is_repeatable(action) {
                    if let Some(&fire_at) = self.next_repeat.get(&action) {
                        if now >= fire_at {
                            self.fired.insert(action);
                            self.next_repeat.insert(action, now + REPEAT_RATE);
                        }
                    }
                }
            } else {
                self.down_since.remove(&action);
                self.next_repeat.remove(&action);
                self.armed.remove(&action);
            }
        }
    }

    /// Keep tracking devices but fire nothing — used while a game is running, so
    /// controller input meant for the game doesn't drive the UI. Buttons still
    /// held when input resumes don't fire until released and pressed again.
    pub fn update_suspended(&mut self) {
        self.update();
        self.fired.clear();
        self.armed.clear();
    }

    /// Did `a` fire this frame (edge or repeat)?
    pub fn action(&self, a: Action) -> bool {
        self.fired.contains(&a)
    }

    /// Seconds `a` has been held, if its press started while input was live.
    pub fn held_for(&self, a: Action) -> Option<f64> {
        if !self.armed.contains(&a) {
            return None;
        }
        self.down_since.get(&a).map(|t| get_time() - t)
    }

    /// Characters typed on the physical keyboard this frame.
    pub fn typed(&self) -> &[char] {
        &self.typed
    }

    /// True when the gamepad was the last device used (for button hints).
    pub fn gamepad_last(&self) -> bool {
        self.gamepad_last
    }

    fn collect_down(&self) -> HashSet<Action> {
        let mut s = HashSet::new();

        // --- Keyboard ---
        let keys: &[(KeyCode, Action)] = if self.text_mode { &ARROW_KEYS } else { &KEYS };
        for &(key, action) in keys {
            if is_key_down(key) {
                s.insert(action);
            }
        }

        // --- Gamepad (any connected pad) ---
        #[cfg(feature = "gamepad")]
        if let Some(g) = self.gilrs.as_ref() {
            for (_id, pad) in g.gamepads() {
                if pad.is_pressed(Button::DPadUp) {
                    s.insert(Action::Up);
                }
                if pad.is_pressed(Button::DPadDown) {
                    s.insert(Action::Down);
                }
                if pad.is_pressed(Button::DPadLeft) {
                    s.insert(Action::Left);
                }
                if pad.is_pressed(Button::DPadRight) {
                    s.insert(Action::Right);
                }

                let ly = pad.value(Axis::LeftStickY);
                if ly > DEADZONE {
                    s.insert(Action::Up);
                }
                if ly < -DEADZONE {
                    s.insert(Action::Down);
                }
                let lx = pad.value(Axis::LeftStickX);
                if lx > DEADZONE {
                    s.insert(Action::Right);
                }
                if lx < -DEADZONE {
                    s.insert(Action::Left);
                }

                if pad.is_pressed(Button::South) {
                    s.insert(Action::Confirm);
                }
                if pad.is_pressed(Button::East) {
                    s.insert(Action::Back);
                }
                if pad.is_pressed(Button::Start) {
                    s.insert(Action::Menu);
                }
                if pad.is_pressed(Button::Select) {
                    s.insert(Action::ToggleAspect);
                }
                if pad.is_pressed(Button::LeftTrigger) {
                    s.insert(Action::TabPrev);
                }
                if pad.is_pressed(Button::RightTrigger) {
                    s.insert(Action::TabNext);
                }
                if pad.is_pressed(Button::West) {
                    s.insert(Action::Edit);
                }
                if pad.is_pressed(Button::North) {
                    s.insert(Action::Alt);
                }
            }
        }

        s
    }
}
