//! The launcher's own preferences, kept as `key = value` lines in its data folder.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    /// Follows `LC_ALL`, `LC_MESSAGES` or `LANG`, or the macOS preference.
    Auto,
    ZhCn,
    En,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::Auto, Language::ZhCn, Language::En];

    /// The concrete language: `ZhCn` or `En`. `Auto` is resolved once per
    /// run, as it is read on every frame and macOS's preference needs a process.
    pub fn resolve(self) -> Language {
        static SYSTEM: std::sync::OnceLock<Language> = std::sync::OnceLock::new();
        match self {
            Language::Auto => *SYSTEM.get_or_init(|| {
                let env = ["LC_ALL", "LC_MESSAGES", "LANG"]
                    .iter()
                    .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
                    .unwrap_or_default();
                if env.to_ascii_lowercase().starts_with("zh") || system_prefers_chinese() {
                    Language::ZhCn
                } else {
                    Language::En
                }
            }),
            other => other,
        }
    }

    /// The code apps get in `GIZLET_LANG`: `en`, or `zh-CN` otherwise.
    pub fn app_code(self) -> &'static str {
        match self.resolve() {
            Language::En => "en",
            _ => "zh-CN",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Language::Auto => "auto",
            Language::ZhCn => "zh-cn",
            Language::En => "en",
        }
    }
}

#[cfg(target_os = "macos")]
fn system_prefers_chinese() -> bool {
    std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
        .map(|o| {
            let s = String::from_utf8_lossy(&o.stdout);
            s.lines().find(|l| l.contains('"')).is_some_and(|l| l.contains("zh"))
        })
        .unwrap_or(false)
}

#[cfg(not(target_os = "macos"))]
fn system_prefers_chinese() -> bool {
    false
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub language: Language,
    pub theme: Theme,
    /// Confirm with the bottom face button and go back with the right one.
    pub swap_ab: bool,
    pub clock_24h: bool,
    pub battery_percent: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            language: Language::Auto,
            theme: Theme::Dark,
            swap_ab: false,
            clock_24h: true,
            battery_percent: true,
        }
    }
}

fn parse_bool(v: &str) -> Option<bool> {
    match v {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

impl Settings {
    /// Reads settings; unknown keys and bad values keep their defaults.
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            match k.trim() {
                "language" => {
                    if let Some(l) = Language::ALL.into_iter().find(|l| l.key() == v) {
                        s.language = l;
                    }
                }
                "theme" => match v {
                    "dark" => s.theme = Theme::Dark,
                    "light" => s.theme = Theme::Light,
                    _ => {}
                },
                "swap_ab" => s.swap_ab = parse_bool(v).unwrap_or(s.swap_ab),
                "clock_24h" => s.clock_24h = parse_bool(v).unwrap_or(s.clock_24h),
                "battery_percent" => s.battery_percent = parse_bool(v).unwrap_or(s.battery_percent),
                _ => {}
            }
        }
        s
    }

    pub fn render(&self) -> String {
        let theme = match self.theme {
            Theme::Dark => "dark",
            Theme::Light => "light",
        };
        format!(
            "language = {}\ntheme = {}\nswap_ab = {}\nclock_24h = {}\nbattery_percent = {}\n",
            self.language.key(),
            theme,
            self.swap_ab,
            self.clock_24h,
            self.battery_percent
        )
    }

    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path).map(|t| Settings::parse(&t)).unwrap_or_default()
    }

    /// Writes the file through a temporary one, so a power cut keeps the old copy.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, self.render())?;
        std::fs::rename(&tmp, path)
    }
}

/// The system's power settings, which gizlet-power reads: the seconds without
/// input before the device sleeps, 0 for never, on battery (`sleep_after`) and
/// on external power (`sleep_after_charging`).
pub struct Power {
    path: PathBuf,
    pub sleep_after: u32,
    pub sleep_after_charging: u32,
}

impl Power {
    /// The choices offered, in order: 1, 3, 5, 10 and 30 minutes, then never.
    pub const SLEEP_AFTER: [u32; 6] = [60, 180, 300, 600, 1800, 0];
    /// gizlet-power's default when the file has no value.
    const DEFAULT: u32 = 300;

    /// On Gizlet OS the file is on the data partition; elsewhere it sits with
    /// the launcher's settings and nothing reads it.
    pub fn path() -> PathBuf {
        if cfg!(feature = "gizlet-os") && std::env::var_os("GIZLET_LAUNCHER_DATA").is_none() {
            PathBuf::from("/run/gizlet-data/system/power.ini")
        } else {
            data_dir().join("power.ini")
        }
    }

    /// `screen_off`, the key before `sleep_after`, is read when that is absent;
    /// without `sleep_after_charging` the device never sleeps while charging,
    /// as in gizlet-power.
    pub fn load(path: PathBuf) -> Power {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let value = |key: &str| {
            text.lines().find_map(|l| {
                let (k, v) = l.split_once('=')?;
                (k.trim() == key).then(|| v.trim().parse().ok()).flatten()
            })
        };
        let sleep_after = value("sleep_after").or_else(|| value("screen_off")).unwrap_or(Power::DEFAULT);
        let sleep_after_charging = value("sleep_after_charging").unwrap_or(0);
        Power { path, sleep_after, sleep_after_charging }
    }

    /// Moves the battery or the charging value `step` choices on (±1).
    pub fn cycle(&mut self, charging: bool, step: i32) {
        let value = if charging { &mut self.sleep_after_charging } else { &mut self.sleep_after };
        let n = Power::SLEEP_AFTER.len() as i32;
        let at = Power::SLEEP_AFTER.iter().position(|s| s == value).unwrap_or(2) as i32;
        *value = Power::SLEEP_AFTER[(at + step).rem_euclid(n) as usize];
    }

    pub fn save(&self) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("tmp");
        std::fs::write(
            &tmp,
            format!(
                "# Seconds without input before the device sleeps, on battery and on\n# external power; 0 is never.\nsleep_after = {}\nsleep_after_charging = {}\n",
                self.sleep_after, self.sleep_after_charging
            ),
        )?;
        std::fs::rename(&tmp, &self.path)
    }
}

/// Where the launcher keeps its settings: `GIZLET_LAUNCHER_DATA`, the data
/// partition on Gizlet OS, or the platform's per-user folder.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("GIZLET_LAUNCHER_DATA") {
        return PathBuf::from(dir);
    }
    if cfg!(feature = "gizlet-os") {
        return PathBuf::from("/opt/gizlet/launcher");
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Gizlet Launcher")
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or(home).join("Gizlet Launcher")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("gizlet-launcher")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_cycles_and_round_trips() {
        let dir = std::env::temp_dir().join(format!("gizlet-power-{}", std::process::id()));
        let path = dir.join("power.ini");
        let mut p = Power::load(path.clone());
        assert_eq!(p.sleep_after, 300);
        assert_eq!(p.sleep_after_charging, 0, "never while charging by default");
        p.cycle(false, 1);
        assert_eq!(p.sleep_after, 600);
        p.cycle(false, 1);
        assert_eq!(p.sleep_after, 1800);
        p.cycle(false, 1);
        assert_eq!(p.sleep_after, 0);
        p.cycle(false, 1);
        assert_eq!(p.sleep_after, 60);
        p.cycle(false, -1);
        assert_eq!(p.sleep_after, 0);
        p.save().unwrap();
        assert_eq!(Power::load(path.clone()).sleep_after, 0);
        std::fs::write(&path, "screen_off = 0\n").unwrap();
        assert_eq!(Power::load(path.clone()).sleep_after, 0, "the old key still counts");
        std::fs::write(&path, "screen_off = 0\nsleep_after = 600\n").unwrap();
        assert_eq!(Power::load(path.clone()).sleep_after, 600, "the new key wins");
        assert_eq!(Power::load(path.clone()).sleep_after_charging, 0, "never while charging when unset");
        let mut p = Power::load(path.clone());
        p.cycle(true, 1);
        assert_eq!((p.sleep_after, p.sleep_after_charging), (600, 60), "each is set on its own");
        p.save().unwrap();
        let p = Power::load(path);
        assert_eq!((p.sleep_after, p.sleep_after_charging), (600, 60));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn round_trip_and_defaults() {
        let s = Settings { theme: Theme::Light, swap_ab: true, language: Language::ZhCn, ..Settings::default() };
        assert_eq!(Settings::parse(&s.render()), s);
        let partial = Settings::parse("theme = light\nswap_ab = maybe\nnoise\n");
        assert_eq!(partial.theme, Theme::Light);
        assert!(!partial.swap_ab);
        assert!(partial.clock_24h);
    }
}
