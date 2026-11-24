//! Buttons and actions.
//!
//! Each frame the physical state of every button is collected into a
//! [`RawPad`] from the keyboard, raylib's gamepads and, on Gizlet OS, the
//! handheld's own evdev devices. The button test draws that state as it is;
//! the pages get [`Action`]s, with auto-repeat for held directions and the
//! A/B swap applied to controller buttons (the keyboard's Enter and Escape
//! keep their meaning).

use raylib_sys::{self as ffi, GamepadAxis as Axis, GamepadButton as G, KeyboardKey as K};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    X,
    Y,
    L,
    R,
    Start,
    Select,
    VolumeUp,
    VolumeDown,
}

impl Action {
    pub const ALL: [Action; 14] = [
        Action::Up,
        Action::Down,
        Action::Left,
        Action::Right,
        Action::A,
        Action::B,
        Action::X,
        Action::Y,
        Action::L,
        Action::R,
        Action::Start,
        Action::Select,
        Action::VolumeUp,
        Action::VolumeDown,
    ];

    pub fn parse(name: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| format!("{a:?}").eq_ignore_ascii_case(name))
    }

    fn repeats(self) -> bool {
        matches!(
            self,
            Action::Up | Action::Down | Action::Left | Action::Right | Action::VolumeUp | Action::VolumeDown
        )
    }
}

/// A physical button. Face buttons are named by position, Nintendo style:
/// A right, B bottom, X top, Y left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    X,
    Y,
    L1,
    R1,
    L2,
    R2,
    Select,
    Start,
    Home,
    L3,
    R3,
    VolumeUp,
    VolumeDown,
    /// Only the handheld's own power key reports this.
    #[cfg_attr(not(all(target_os = "linux", feature = "gizlet-os")), allow(dead_code))]
    Power,
}

pub const BUTTONS: usize = 20;

/// The physical state of the controls this frame.
#[derive(Clone, Default)]
pub struct RawPad {
    /// The controller's name, when one is connected.
    pub name: Option<String>,
    pub buttons: [bool; BUTTONS],
    /// Left X/Y and right X/Y in -1..1, then the left and right triggers in 0..1.
    pub axes: [f32; 6],
}

impl RawPad {
    pub fn down(&self, b: Button) -> bool {
        self.buttons[b as usize]
    }

    pub fn set(&mut self, b: Button) {
        self.buttons[b as usize] = true;
    }

    fn merge(&mut self, other: &RawPad) {
        for (a, b) in self.buttons.iter_mut().zip(other.buttons) {
            *a |= b;
        }
        for (a, b) in self.axes.iter_mut().zip(other.axes) {
            if b.abs() > a.abs() {
                *a = b;
            }
        }
        if self.name.is_none() {
            self.name.clone_from(&other.name);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TextEvent {
    Char(char),
    Backspace,
    Submit,
    Cancel,
}

#[derive(Default)]
pub struct Input {
    pub pressed: Vec<Action>,
    pub held: Vec<Action>,
    pub text: Vec<TextEvent>,
}

impl Input {
    pub fn pressed(&self, a: Action) -> bool {
        self.pressed.contains(&a)
    }

    pub fn held(&self, a: Action) -> bool {
        self.held.contains(&a)
    }
}

const REPEAT_DELAY: f32 = 0.38;
const REPEAT_EVERY: f32 = 0.075;
const STICK: f32 = 0.55;

fn key(k: K) -> bool {
    unsafe { ffi::IsKeyDown(k as i32) }
}

fn key_pressed(k: K) -> bool {
    unsafe { ffi::IsKeyPressed(k as i32) || ffi::IsKeyPressedRepeat(k as i32) }
}

/// Keyboard equivalents of the buttons.
const KEYS: [(K, Button); 21] = [
    (K::KEY_UP, Button::Up),
    (K::KEY_DOWN, Button::Down),
    (K::KEY_LEFT, Button::Left),
    (K::KEY_RIGHT, Button::Right),
    (K::KEY_ENTER, Button::A),
    (K::KEY_SPACE, Button::A),
    (K::KEY_K, Button::A),
    (K::KEY_ESCAPE, Button::B),
    (K::KEY_BACKSPACE, Button::B),
    (K::KEY_J, Button::B),
    (K::KEY_I, Button::X),
    (K::KEY_TAB, Button::X),
    (K::KEY_U, Button::Y),
    (K::KEY_Q, Button::L1),
    (K::KEY_E, Button::R1),
    (K::KEY_ONE, Button::L2),
    (K::KEY_THREE, Button::R2),
    (K::KEY_EQUAL, Button::Start),
    (K::KEY_MINUS, Button::Select),
    (K::KEY_RIGHT_BRACKET, Button::VolumeUp),
    (K::KEY_LEFT_BRACKET, Button::VolumeDown),
];

/// Keys that type while the on-screen keyboard is open: all but the arrows,
/// which keep moving its focus. `=` types too rather than pressing Start, so
/// a password with `=` or `+` is not submitted half-typed; Enter submits.
fn text_key(k: K) -> bool {
    !matches!(k, K::KEY_UP | K::KEY_DOWN | K::KEY_LEFT | K::KEY_RIGHT)
}

/// raylib's gamepad buttons; raylib names face buttons by position too.
const PAD_BUTTONS: [(G, Button); 15] = [
    (G::GAMEPAD_BUTTON_LEFT_FACE_UP, Button::Up),
    (G::GAMEPAD_BUTTON_LEFT_FACE_DOWN, Button::Down),
    (G::GAMEPAD_BUTTON_LEFT_FACE_LEFT, Button::Left),
    (G::GAMEPAD_BUTTON_LEFT_FACE_RIGHT, Button::Right),
    (G::GAMEPAD_BUTTON_RIGHT_FACE_RIGHT, Button::A),
    (G::GAMEPAD_BUTTON_RIGHT_FACE_DOWN, Button::B),
    (G::GAMEPAD_BUTTON_RIGHT_FACE_UP, Button::X),
    (G::GAMEPAD_BUTTON_RIGHT_FACE_LEFT, Button::Y),
    (G::GAMEPAD_BUTTON_LEFT_TRIGGER_1, Button::L1),
    (G::GAMEPAD_BUTTON_RIGHT_TRIGGER_1, Button::R1),
    (G::GAMEPAD_BUTTON_LEFT_TRIGGER_2, Button::L2),
    (G::GAMEPAD_BUTTON_RIGHT_TRIGGER_2, Button::R2),
    (G::GAMEPAD_BUTTON_MIDDLE_LEFT, Button::Select),
    (G::GAMEPAD_BUTTON_MIDDLE_RIGHT, Button::Start),
    (G::GAMEPAD_BUTTON_MIDDLE, Button::Home),
];

fn raylib_pads() -> Vec<(i32, String)> {
    (0..4)
        .filter(|&i| unsafe { ffi::IsGamepadAvailable(i) })
        .map(|i| unsafe {
            let p = ffi::GetGamepadName(i);
            let name = if p.is_null() {
                format!("Gamepad {}", i + 1)
            } else {
                std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
            };
            (i, name)
        })
        .collect()
}

fn raylib_pad(index: i32, name: String) -> RawPad {
    let mut raw = RawPad { name: Some(name), ..RawPad::default() };
    for (g, b) in PAD_BUTTONS {
        if unsafe { ffi::IsGamepadButtonDown(index, g as i32) } {
            raw.set(b);
        }
    }
    if unsafe { ffi::IsGamepadButtonDown(index, G::GAMEPAD_BUTTON_LEFT_THUMB as i32) } {
        raw.set(Button::L3);
    }
    if unsafe { ffi::IsGamepadButtonDown(index, G::GAMEPAD_BUTTON_RIGHT_THUMB as i32) } {
        raw.set(Button::R3);
    }
    let axes = [
        Axis::GAMEPAD_AXIS_LEFT_X,
        Axis::GAMEPAD_AXIS_LEFT_Y,
        Axis::GAMEPAD_AXIS_RIGHT_X,
        Axis::GAMEPAD_AXIS_RIGHT_Y,
        Axis::GAMEPAD_AXIS_LEFT_TRIGGER,
        Axis::GAMEPAD_AXIS_RIGHT_TRIGGER,
    ];
    for (slot, a) in raw.axes.iter_mut().zip(axes) {
        let v = unsafe { ffi::GetGamepadAxisMovement(index, a as i32) };
        // Triggers rest at -1 or 0 depending on the mapping; only the pressed half counts.
        *slot = if matches!(a, Axis::GAMEPAD_AXIS_LEFT_TRIGGER | Axis::GAMEPAD_AXIS_RIGHT_TRIGGER) {
            v.clamp(0.0, 1.0)
        } else {
            v
        };
    }
    raw
}

fn actions_of(pad: &RawPad, swap_ab: bool, out: &mut Vec<Action>) {
    let (lx, ly) = (pad.axes[0], pad.axes[1]);
    let (a, b) = if swap_ab { (Button::B, Button::A) } else { (Button::A, Button::B) };
    let pairs = [
        (Action::Up, pad.down(Button::Up) || ly < -STICK),
        (Action::Down, pad.down(Button::Down) || ly > STICK),
        (Action::Left, pad.down(Button::Left) || lx < -STICK),
        (Action::Right, pad.down(Button::Right) || lx > STICK),
        (Action::A, pad.down(a)),
        (Action::B, pad.down(b)),
        (Action::X, pad.down(Button::X)),
        (Action::Y, pad.down(Button::Y)),
        (Action::L, pad.down(Button::L1)),
        (Action::R, pad.down(Button::R1)),
        (Action::Start, pad.down(Button::Start)),
        (Action::Select, pad.down(Button::Select)),
        (Action::VolumeUp, pad.down(Button::VolumeUp)),
        (Action::VolumeDown, pad.down(Button::VolumeDown)),
    ];
    for (action, down) in pairs {
        if down && !out.contains(&action) {
            out.push(action);
        }
    }
}

#[derive(Default)]
pub struct Controls {
    held_for: Vec<(Action, f32, f32)>,
    #[cfg(all(target_os = "linux", feature = "gizlet-os"))]
    evdev: crate::evdev::Evdev,
}

impl Controls {
    /// Names of the connected controllers.
    pub fn gamepads(&self) -> Vec<String> {
        #[cfg(all(target_os = "linux", feature = "gizlet-os"))]
        {
            let pads = self.evdev.pads();
            if !pads.is_empty() {
                return pads;
            }
        }
        raylib_pads().into_iter().map(|(_, n)| n).collect()
    }

    /// Reads this frame's controls. While `text_mode` is on, the keyboard types
    /// instead of pressing buttons, except for the arrow keys.
    pub fn read(&mut self, dt: f32, time: f64, text_mode: bool, swap_ab: bool) -> (Input, RawPad) {
        let mut input = Input::default();
        if text_mode {
            loop {
                let c = unsafe { ffi::GetCharPressed() };
                if c == 0 {
                    break;
                }
                if let Some(ch) = char::from_u32(c as u32) {
                    input.text.push(TextEvent::Char(ch));
                }
            }
            if key_pressed(K::KEY_BACKSPACE) {
                input.text.push(TextEvent::Backspace);
            }
            if key_pressed(K::KEY_ENTER) {
                input.text.push(TextEvent::Submit);
            }
            if key_pressed(K::KEY_ESCAPE) {
                input.text.push(TextEvent::Cancel);
            }
        } else {
            // Drain typed characters so they don't arrive when a text field opens later.
            while unsafe { ffi::GetCharPressed() } != 0 {}
        }

        let mut keys = RawPad::default();
        for (k, b) in KEYS {
            if (!text_mode || !text_key(k)) && key(k) {
                keys.set(b);
            }
        }

        // The handheld's own devices replace raylib's view of the same gamepad.
        let mut pad = RawPad::default();
        #[cfg(all(target_os = "linux", feature = "gizlet-os"))]
        let own = {
            self.evdev.poll(time);
            self.evdev.state(&mut pad)
        };
        #[cfg(not(all(target_os = "linux", feature = "gizlet-os")))]
        let own = {
            let _ = time;
            false
        };
        if !own {
            if let Some((i, name)) = raylib_pads().into_iter().next() {
                pad = raylib_pad(i, name);
            }
        }

        let mut down = Vec::new();
        actions_of(&pad, swap_ab, &mut down);
        actions_of(&keys, false, &mut down);

        let mut held_for = Vec::new();
        for a in Action::ALL {
            if !down.contains(&a) {
                continue;
            }
            input.held.push(a);
            match self.held_for.iter().find(|(h, _, _)| *h == a) {
                None => {
                    input.pressed.push(a);
                    held_for.push((a, 0.0, REPEAT_DELAY));
                }
                Some(&(_, t, mut next)) => {
                    let t = t + dt;
                    if a.repeats() && t >= next {
                        next += REPEAT_EVERY;
                        input.pressed.push(a);
                    }
                    held_for.push((a, t, next));
                }
            }
        }
        self.held_for = held_for;

        let mut raw = pad;
        raw.merge(&keys);
        (input, raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_type_in_text_mode_except_the_arrows() {
        // = and + belong in passwords; Enter submits instead.
        assert!(text_key(K::KEY_EQUAL));
        assert!(text_key(K::KEY_MINUS));
        assert!(text_key(K::KEY_ENTER));
        for arrow in [K::KEY_UP, K::KEY_DOWN, K::KEY_LEFT, K::KEY_RIGHT] {
            assert!(!text_key(arrow));
        }
    }
}
