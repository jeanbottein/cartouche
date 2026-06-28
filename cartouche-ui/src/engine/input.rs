//! Unified input layer (ENGINE-3).
//!
//! Collapses keyboard, gamepad (via `gilrs`, behind the `gamepad` feature) and —
//! for pointing — mouse into a single [`Action`] vocabulary so the rest of the app
//! never touches raw devices. Directional actions auto-repeat (initial delay then
//! steady rate); buttons fire on the press edge only. Tunables mirror the legacy
//! `lib/gui/controller.py`.

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
}

const ALL_ACTIONS: [Action; 10] = [
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
        }
    }

    /// Poll devices and compute which actions fired this frame. Call once per frame.
    pub fn update(&mut self) {
        #[cfg(feature = "gamepad")]
        if let Some(g) = self.gilrs.as_mut() {
            while g.next_event().is_some() {} // pump events to refresh gamepad state
        }

        let now = get_time();
        let down = self.collect_down();
        self.fired.clear();

        for action in ALL_ACTIONS {
            if down.contains(&action) {
                if !self.down_since.contains_key(&action) {
                    // press edge
                    self.down_since.insert(action, now);
                    self.fired.insert(action);
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
            }
        }
    }

    /// Did `a` fire this frame (edge or repeat)?
    pub fn action(&self, a: Action) -> bool {
        self.fired.contains(&a)
    }

    fn collect_down(&self) -> HashSet<Action> {
        let mut s = HashSet::new();

        // --- Keyboard ---
        if is_key_down(KeyCode::Up) || is_key_down(KeyCode::W) {
            s.insert(Action::Up);
        }
        if is_key_down(KeyCode::Down) || is_key_down(KeyCode::S) {
            s.insert(Action::Down);
        }
        if is_key_down(KeyCode::Left) || is_key_down(KeyCode::A) {
            s.insert(Action::Left);
        }
        if is_key_down(KeyCode::Right) || is_key_down(KeyCode::D) {
            s.insert(Action::Right);
        }
        if is_key_down(KeyCode::Enter) || is_key_down(KeyCode::Space) {
            s.insert(Action::Confirm);
        }
        if is_key_down(KeyCode::Escape) || is_key_down(KeyCode::Backspace) {
            s.insert(Action::Back);
        }
        if is_key_down(KeyCode::Tab) {
            s.insert(Action::ToggleAspect);
        }
        if is_key_down(KeyCode::Q) {
            s.insert(Action::TabPrev);
        }
        if is_key_down(KeyCode::E) {
            s.insert(Action::TabNext);
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
            }
        }

        s
    }
}
