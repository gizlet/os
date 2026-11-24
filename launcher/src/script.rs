//! Scripted input for tests and screenshots: `GIZLET_SCRIPT` holds commands
//! separated by `;`.
//!
//! - `wait N`: let N frames pass.
//! - `press ACTION`: press an action for one frame (`up`, `a`, `start`, …).
//! - `hold ACTION N`: hold it for N frames.
//! - `shot PATH`: save the screen as a PNG.
//! - `quit`: leave.

use crate::input::{Action, Button, Input, RawPad};
use std::collections::VecDeque;

enum Command {
    Wait(u32),
    Press(Action),
    Hold(Action, u32),
    Shot(String),
    Quit,
}

pub enum Step {
    Shot(String),
    Quit,
}

#[derive(Default)]
pub struct Script {
    commands: VecDeque<Command>,
    wait: u32,
    holding: Option<(Action, u32, bool)>,
}

fn parse(text: &str) -> Result<VecDeque<Command>, String> {
    let mut out = VecDeque::new();
    for part in text.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let words: Vec<&str> = part.split_whitespace().collect();
        let action = |w: &str| Action::parse(w).ok_or_else(|| format!("unknown action {w}"));
        let number = |w: &str| w.parse::<u32>().map_err(|_| format!("bad number {w}"));
        out.push_back(match words.as_slice() {
            ["wait", n] => Command::Wait(number(n)?),
            ["press", a] => Command::Press(action(a)?),
            ["hold", a, n] => Command::Hold(action(a)?, number(n)?),
            ["shot", path] => Command::Shot((*path).to_string()),
            ["quit"] => Command::Quit,
            _ => return Err(format!("bad command {part}")),
        });
    }
    Ok(out)
}

fn button(a: Action) -> Option<Button> {
    Some(match a {
        Action::Up => Button::Up,
        Action::Down => Button::Down,
        Action::Left => Button::Left,
        Action::Right => Button::Right,
        Action::A => Button::A,
        Action::B => Button::B,
        Action::X => Button::X,
        Action::Y => Button::Y,
        Action::L => Button::L1,
        Action::R => Button::R1,
        Action::Start => Button::Start,
        Action::Select => Button::Select,
        Action::VolumeUp => Button::VolumeUp,
        Action::VolumeDown => Button::VolumeDown,
    })
}

impl Script {
    pub fn from_env() -> Script {
        let Ok(text) = std::env::var("GIZLET_SCRIPT") else { return Script::default() };
        match parse(&text) {
            Ok(commands) => Script { commands, ..Script::default() },
            Err(e) => {
                eprintln!("gizlet-launcher: GIZLET_SCRIPT: {e}");
                Script::default()
            }
        }
    }

    /// Adds this frame's scripted input.
    pub fn step(&mut self, input: &mut Input, pad: &mut RawPad) -> Option<Step> {
        if let Some((a, left, first)) = self.holding.as_mut() {
            input.held.push(*a);
            if *first {
                input.pressed.push(*a);
                *first = false;
            }
            if let Some(b) = button(*a) {
                pad.set(b);
            }
            *left -= 1;
            if *left == 0 {
                self.holding = None;
            }
            return None;
        }
        if self.wait > 0 {
            self.wait -= 1;
            return None;
        }
        match self.commands.pop_front()? {
            Command::Wait(n) => self.wait = n,
            Command::Press(a) => {
                input.pressed.push(a);
                input.held.push(a);
            }
            Command::Hold(a, n) => self.holding = Some((a, n.max(1), true)),
            Command::Shot(path) => return Some(Step::Shot(path)),
            Command::Quit => return Some(Step::Quit),
        }
        None
    }
}
