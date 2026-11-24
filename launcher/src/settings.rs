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

    /// The concrete language: `ZhCn` or `En`.
    pub fn resolve(self) -> Language {
        match self {
            Language::Auto => {
                let env = ["LC_ALL", "LC_MESSAGES", "LANG"]
                    .iter()
                    .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
                    .unwrap_or_default();
                if env.to_ascii_lowercase().starts_with("zh") || system_prefers_chinese() {
                    Language::ZhCn
                } else {
                    Language::En
                }
            }
            other => other,
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
    fn round_trip_and_defaults() {
        let s = Settings { theme: Theme::Light, swap_ab: true, language: Language::ZhCn, ..Settings::default() };
        assert_eq!(Settings::parse(&s.render()), s);
        let partial = Settings::parse("theme = light\nswap_ab = maybe\nnoise\n");
        assert_eq!(partial.theme, Theme::Light);
        assert!(!partial.swap_ab);
        assert!(partial.clock_24h);
    }
}
