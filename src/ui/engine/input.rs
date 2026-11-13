//! Unified input layer.
//!
//! Collapses keyboard, gamepad (via `gilrs`, behind the `gamepad` feature) and —
//! for pointing — mouse into a single [`Action`] vocabulary so the rest of the app
//! never touches raw devices. Directional actions auto-repeat (initial delay then
//! steady rate); buttons fire on the press edge only. Tunables match the former
//! Dear PyGui controller layer.

use std::collections::hash_map::Entry;
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
    TabPrev,
    TabNext,
    /// Secondary action: edit / type a value (pad X, keyboard X).
    Edit,
    /// Tertiary action: e.g. shift on the on-screen keyboard (pad Y).
    Alt,
}

const ALL_ACTIONS: [Action; 11] = [
    Action::Up,
    Action::Down,
    Action::Left,
    Action::Right,
    Action::Confirm,
    Action::Back,
    Action::Menu,
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

/// Every other key: off while typing, so it reaches the text.
const COMMAND_KEYS: [(KeyCode, Action); 12] = [
    (KeyCode::W, Action::Up),
    (KeyCode::S, Action::Down),
    (KeyCode::A, Action::Left),
    (KeyCode::D, Action::Right),
    (KeyCode::Enter, Action::Confirm),
    (KeyCode::Space, Action::Confirm),
    (KeyCode::Escape, Action::Back),
    (KeyCode::Backspace, Action::Back),
    (KeyCode::Q, Action::TabPrev),
    (KeyCode::E, Action::TabNext),
    (KeyCode::X, Action::Edit),
    (KeyCode::H, Action::Alt),
];

#[cfg(feature = "gamepad")]
const PAD_BUTTONS: [(Button, Action); 11] = [
    (Button::DPadUp, Action::Up),
    (Button::DPadDown, Action::Down),
    (Button::DPadLeft, Action::Left),
    (Button::DPadRight, Action::Right),
    (Button::South, Action::Confirm),
    (Button::East, Action::Back),
    (Button::Start, Action::Menu),
    (Button::LeftTrigger, Action::TabPrev),
    (Button::RightTrigger, Action::TabNext),
    (Button::West, Action::Edit),
    (Button::North, Action::Alt),
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
                if let Entry::Vacant(press_edge) = self.down_since.entry(action) {
                    press_edge.insert(now);
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

    /// Keys held now stop repeating until released and pressed again. For
    /// window changes: a fullscreen transition can swallow the key release
    /// (macOS), and a "held" arrow would keep toggling the setting.
    pub fn hold_until_released(&mut self) {
        self.next_repeat.clear();
        self.armed.clear();
    }

    /// Did `a` fire this frame (edge or repeat)?
    pub fn action(&self, a: Action) -> bool {
        self.fired.contains(&a)
    }

    /// Fire `a` this frame, as if its key or button was pressed (a click on
    /// a footer button).
    pub fn press(&mut self, a: Action) {
        self.fired.insert(a);
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
        let commands: &[(KeyCode, Action)] = if self.text_mode { &[] } else { &COMMAND_KEYS };
        ARROW_KEYS
            .iter()
            .chain(commands)
            .filter(|(key, _)| is_key_down(*key))
            .map(|&(_, action)| action)
            .chain(self.pad_down())
            .collect()
    }

    /// Actions held on any connected gamepad.
    #[cfg(feature = "gamepad")]
    fn pad_down(&self) -> Vec<Action> {
        let Some(gilrs) = self.gilrs.as_ref() else {
            return Vec::new();
        };
        gilrs
            .gamepads()
            .flat_map(|(_, pad)| {
                let (x, y) = (pad.value(Axis::LeftStickX), pad.value(Axis::LeftStickY));
                let stick = [
                    (y > DEADZONE, Action::Up),
                    (y < -DEADZONE, Action::Down),
                    (x > DEADZONE, Action::Right),
                    (x < -DEADZONE, Action::Left),
                ];
                PAD_BUTTONS
                    .map(|(button, action)| (pad.is_pressed(button), action))
                    .into_iter()
                    .chain(stick)
                    .filter(|(held, _)| *held)
                    .map(|(_, action)| action)
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    #[cfg(not(feature = "gamepad"))]
    fn pad_down(&self) -> Vec<Action> {
        Vec::new()
    }
}
