//! The on-screen keyboard: a grid driven by the D-pad, which a physical keyboard can also type into.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    Char(char),
    Shift,
    Symbols,
    Backspace,
    Space,
    Done,
}

impl Key {
    /// Width in key units.
    pub fn width(self) -> f32 {
        match self {
            Key::Char(_) => 1.0,
            Key::Shift | Key::Backspace => 1.5,
            Key::Symbols | Key::Done => 2.0,
            Key::Space => 6.0,
        }
    }
}

pub const DONE_HOLD_SECONDS: u32 = 2;

const LETTERS: [&str; 4] = ["1234567890", "qwertyuiop", "asdfghjkl", "zxcvbnm"];
const SYMBOLS: [&str; 4] = ["1234567890", "!@#$%^&*()", "-_=+[]{};:", "'\",.?/\\|~`<>"];

/// What the typed text is for.
#[derive(Clone, Debug, PartialEq)]
pub enum Purpose {
    /// The passphrase of this Wi-Fi network.
    WifiPassphrase(String),
    /// root's password for ssh logins.
    SshPassword,
    /// The device's name.
    Hostname,
    /// A profile's name: of the profile with this ID, or of a new one.
    ProfileName(Option<String>),
}

pub enum Outcome {
    None,
    Submit(String),
}

#[derive(Clone, Debug)]
pub struct Keyboard {
    pub title: String,
    pub purpose: Purpose,
    pub text: String,
    pub masked: bool,
    pub shift: bool,
    pub symbols: bool,
    pub row: usize,
    pub col: usize,
    pub max_len: usize,
    a_press: Option<(Key, f64)>,
}

impl Keyboard {
    pub fn passphrase(title: String, ssid: String) -> Keyboard {
        Keyboard {
            title,
            purpose: Purpose::WifiPassphrase(ssid),
            text: String::new(),
            masked: true,
            shift: false,
            symbols: false,
            row: 1,
            col: 0,
            // WPA passphrases are 8 to 63 characters.
            max_len: 63,
            a_press: None,
        }
    }

    /// The ssh password, shown in the clear and starting from the current one.
    pub fn ssh_password(title: String, current: &str) -> Keyboard {
        let mut kb = Keyboard::passphrase(title, String::new());
        kb.purpose = Purpose::SshPassword;
        kb.masked = false;
        kb.text = current.to_string();
        kb
    }

    /// The device's name, starting from the current one: one DNS label.
    pub fn hostname(title: String, current: &str) -> Keyboard {
        let mut kb = Keyboard::ssh_password(title, current);
        kb.purpose = Purpose::Hostname;
        kb.max_len = 63;
        kb
    }

    /// A profile's name, starting from `current`.
    pub fn profile_name(title: String, profile: Option<String>, current: &str) -> Keyboard {
        let mut kb = Keyboard::ssh_password(title, current);
        kb.purpose = Purpose::ProfileName(profile);
        kb.max_len = gizlet_launcher::profiles::MAX_NAME_CHARS;
        kb
    }

    pub fn rows(&self) -> Vec<Vec<Key>> {
        let src = if self.symbols { SYMBOLS } else { LETTERS };
        let upper = self.shift && !self.symbols;
        let mut rows: Vec<Vec<Key>> = src
            .iter()
            .map(|s| s.chars().map(|c| Key::Char(if upper { c.to_ascii_uppercase() } else { c })).collect())
            .collect();
        rows[2].push(Key::Backspace);
        rows[3].insert(0, Key::Shift);
        rows.push(vec![Key::Symbols, Key::Space, Key::Done]);
        rows
    }

    pub fn focused(&self) -> Key {
        let rows = self.rows();
        let row = &rows[self.row.min(rows.len() - 1)];
        row[self.col.min(row.len() - 1)]
    }

    /// Moves the focus; vertical moves land on the key closest below or above.
    pub fn move_by(&mut self, dx: i32, dy: i32) {
        let rows = self.rows();
        if dx != 0 {
            let len = rows[self.row].len() as i32;
            self.col = (self.col as i32 + dx).rem_euclid(len) as usize;
        }
        if dy != 0 {
            let center = key_center(&rows[self.row], self.col);
            self.row = (self.row as i32 + dy).rem_euclid(rows.len() as i32) as usize;
            let target = &rows[self.row];
            self.col = (0..target.len())
                .min_by(|&a, &b| (key_center(target, a) - center).abs().total_cmp(&(key_center(target, b) - center).abs()))
                .unwrap_or(0);
        }
    }

    /// A tap activates the key focused on press; a hold submits without activating that key.
    pub fn a_input(&mut self, pressed: bool, held: bool, now: f64) -> Outcome {
        if pressed {
            let key = self.focused();
            if key == Key::Done {
                return self.press(key);
            }
            self.a_press = Some((key, now));
        }
        let Some((key, since)) = self.a_press else { return Outcome::None };
        let key = if !held {
            key
        } else if now - since >= f64::from(DONE_HOLD_SECONDS) {
            Key::Done
        } else {
            return Outcome::None;
        };
        self.a_press = None;
        self.press(key)
    }

    fn press(&mut self, key: Key) -> Outcome {
        match key {
            Key::Char(c) => {
                self.type_char(c);
                if self.shift && !self.symbols {
                    self.shift = false;
                }
            }
            Key::Shift => self.shift = !self.shift,
            Key::Symbols => {
                self.symbols = !self.symbols;
                let rows = self.rows();
                self.row = self.row.min(rows.len() - 1);
                self.col = self.col.min(rows[self.row].len() - 1);
            }
            Key::Backspace => self.backspace(),
            Key::Space => self.type_char(' '),
            Key::Done => return Outcome::Submit(self.text.clone()),
        }
        Outcome::None
    }

    pub fn type_char(&mut self, c: char) {
        if !c.is_control() && self.text.chars().count() < self.max_len {
            self.text.push(c);
        }
    }

    pub fn backspace(&mut self) {
        self.text.pop();
    }
}

/// The center of key `i`, measured from the middle of its row (rows are centered).
pub fn key_center(row: &[Key], i: usize) -> f32 {
    let total: f32 = row.iter().map(|k| k.width()).sum();
    let before: f32 = row[..i].iter().map(|k| k.width()).sum();
    before + row[i].width() / 2.0 - total / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_shifts_and_moves_by_position() {
        let mut kb = Keyboard::passphrase("t".into(), "x".into());
        assert_eq!(kb.focused(), Key::Char('q'));
        kb.row = 3;
        kb.col = 0;
        kb.press(Key::Shift);
        kb.move_by(1, 0);
        assert_eq!(kb.focused(), Key::Char('Z'));
        kb.press(kb.focused());
        assert_eq!(kb.text, "Z");
        assert_eq!(kb.focused(), Key::Char('z'));
        kb.move_by(0, 1); // z sits closest to the symbols key
        assert_eq!(kb.focused(), Key::Symbols);
        kb.move_by(0, -1);
        kb.move_by(3, 0); // c sits over the space bar
        kb.move_by(0, 1);
        assert_eq!(kb.focused(), Key::Space);
        kb.move_by(1, 0);
        assert!(matches!(kb.a_input(true, true, 0.0), Outcome::Submit(t) if t == "Z"));
    }

    #[test]
    fn tapping_a_types_the_key_focused_at_press_on_release() {
        let mut kb = Keyboard::passphrase("t".into(), "x".into());
        assert!(matches!(kb.a_input(true, true, 0.0), Outcome::None));
        assert!(kb.text.is_empty());
        kb.move_by(1, 0);
        assert!(matches!(kb.a_input(false, false, 0.1), Outcome::None));
        assert_eq!(kb.text, "q");
    }

    #[test]
    fn holding_a_submits_once_without_typing() {
        let mut kb = Keyboard::hostname("t".into(), "gizlet");
        let threshold = f64::from(DONE_HOLD_SECONDS);
        // The press that opened the keyboard may still be held.
        assert!(matches!(kb.a_input(false, true, 0.0), Outcome::None));
        assert!(matches!(kb.a_input(false, true, threshold), Outcome::None));
        assert!(matches!(kb.a_input(false, false, threshold + 0.1), Outcome::None));

        let start = threshold + 1.0;
        assert!(matches!(kb.a_input(true, true, start), Outcome::None));
        assert!(matches!(kb.a_input(false, true, start + threshold - 0.1), Outcome::None));
        assert!(matches!(kb.a_input(false, true, start + threshold), Outcome::Submit(t) if t == "gizlet"));
        assert!(matches!(kb.a_input(false, true, start + threshold + 0.1), Outcome::None));
        assert!(matches!(kb.a_input(false, false, start + threshold + 0.2), Outcome::None));
        assert_eq!(kb.text, "gizlet");
    }
}
