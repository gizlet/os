//! Linux devices through kernel interfaces and standard tools: sysfs for the
//! battery and backlight, ALSA's `amixer` for volume, iwd's `iwctl` for
//! Wi-Fi and systemd for ssh and power.
//!
//! On Gizlet OS, Wi-Fi and ssh are switched with `gizlet-switch`, which keeps
//! the choice on the data partition so the service starts again at boot.

use super::util::{columns, on_path, run, run_for, strip_ansi};
use super::*;
use std::fs;
use std::path::{Path, PathBuf};

const SWITCH: &str = "/usr/lib/gizlet/gizlet-switch";
const SSH_KEYS: &str = "/run/gizlet-ssh/authorized_keys";

pub fn devices() -> Devices {
    Devices {
        battery: Box::new(SysfsBattery),
        backlight: SysfsBacklight::detect().map(|b| Box::new(b) as Box<dyn Level>),
        volume: Amixer::detect().map(|v| Box::new(v) as Box<dyn Level>),
        wifi: Iwd::detect().map(|w| Box::new(w) as Box<dyn Wifi>),
        ssh: Sshd::detect().map(|s| Box::new(s) as Box<dyn Ssh>),
        power: LinuxPower::detect().map(|p| Box::new(p) as Box<dyn Power>),
        simulated: false,
    }
}

fn read_trim(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn class_dirs(class: &str) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(Path::new("/sys/class").join(class))
        .map(|it| it.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    dirs.sort();
    dirs
}

fn active(unit: &str) -> bool {
    run("systemctl", &["is-active", "--quiet", unit]).is_ok()
}

fn switch(name: &str, on: bool) -> Result<(), String> {
    run_for(Duration::from_secs(30), SWITCH, &[name, if on { "on" } else { "off" }]).map(|_| ())
}

/// Detected on every read: a battery driver may bind after the launcher starts.
struct SysfsBattery;

impl BatterySource for SysfsBattery {
    fn read(&mut self) -> Result<Battery, Missing> {
        let dir = class_dirs("power_supply")
            .into_iter()
            .find(|d| read_trim(&d.join("type")).as_deref() == Some("Battery") && d.join("capacity").exists())
            .ok_or(Missing::NoHardware)?;
        let percent = read_trim(&dir.join("capacity"))
            .and_then(|c| c.parse::<u16>().ok())
            .ok_or_else(|| Missing::Error("capacity unreadable".into()))?
            .min(100) as u8;
        let charge = match read_trim(&dir.join("status")).as_deref() {
            Some("Charging") => Charge::Charging,
            Some("Full") => Charge::Full,
            _ => Charge::Discharging,
        };
        Ok(Battery { percent, charge })
    }
}

struct SysfsBacklight {
    dir: PathBuf,
    max: u32,
}

impl SysfsBacklight {
    fn detect() -> Result<SysfsBacklight, Missing> {
        let dir = class_dirs("backlight").into_iter().next().ok_or(Missing::NoHardware)?;
        let max = read_trim(&dir.join("max_brightness"))
            .and_then(|m| m.parse().ok())
            .filter(|m| *m > 0)
            .ok_or_else(|| Missing::Error("max_brightness unreadable".into()))?;
        Ok(SysfsBacklight { dir, max })
    }
}

impl Level for SysfsBacklight {
    fn get(&mut self) -> Option<f32> {
        let v: u32 = read_trim(&self.dir.join("brightness"))?.parse().ok()?;
        Some(v as f32 / self.max as f32)
    }
    fn set(&mut self, value: f32) -> Result<(), String> {
        // Never fully dark: a black screen looks like a dead device.
        let raw = ((value * self.max as f32).round() as u32).clamp((self.max / 20).max(1), self.max);
        fs::write(self.dir.join("brightness"), raw.to_string()).map_err(|e| format!("backlight: {e}"))
    }
}

struct Amixer {
    control: String,
}

impl Amixer {
    const PREFERRED: [&'static str; 6] = ["Master", "Playback", "PCM", "Speaker", "Headphone", "DAC"];

    fn detect() -> Result<Amixer, Missing> {
        let cards = read_trim(Path::new("/proc/asound/cards")).unwrap_or_default();
        if cards.is_empty() || cards.contains("no soundcards") {
            return Err(Missing::NoHardware);
        }
        if !on_path("amixer") {
            return Err(Missing::NoService("alsa-utils"));
        }
        let list = run("amixer", &["scontrols"]).map_err(Missing::Error)?;
        let names: Vec<String> = list.lines().filter_map(|l| l.split('\'').nth(1).map(String::from)).collect();
        let control = Self::PREFERRED
            .iter()
            .find_map(|p| names.iter().find(|n| n.as_str() == *p))
            .or_else(|| names.first())
            .cloned()
            .ok_or(Missing::NoHardware)?;
        Ok(Amixer { control })
    }
}

impl Level for Amixer {
    fn get(&mut self) -> Option<f32> {
        let out = run("amixer", &["sget", &self.control]).ok()?;
        let pct = out.split('[').find_map(|p| p.split_once("%]").map(|(n, _)| n))?;
        Some(pct.parse::<f32>().ok()? / 100.0)
    }
    fn set(&mut self, value: f32) -> Result<(), String> {
        let pct = format!("{}%", (value * 100.0).round() as u32);
        run("amixer", &["-q", "sset", &self.control, &pct]).map(|_| ())
    }
}

/// Wi-Fi through iwd's command-line client.
struct Iwd {
    switchable: bool,
}

impl Iwd {
    fn detect() -> Result<Iwd, Missing> {
        if !on_path("iwctl") {
            return Err(if on_path("nmcli") { Missing::ManagedBySystem } else { Missing::NoService("iwd") });
        }
        Ok(Iwd { switchable: Path::new(SWITCH).exists() })
    }

    fn iface() -> Option<String> {
        class_dirs("net")
            .into_iter()
            .find(|d| d.join("wireless").exists() || d.join("phy80211").exists())
            .and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
    }

    fn running() -> bool {
        !on_path("systemctl") || active("iwd")
    }

    /// iwctl waits forever for an iwd that is not running, so callers check first.
    fn iwctl(args: &[&str], limit: u64) -> Result<String, String> {
        run_for(Duration::from_secs(limit), "iwctl", args).map(|s| strip_ansi(&s))
    }

    fn station() -> Result<String, String> {
        if !Self::running() {
            return Err("Wi-Fi is off".into());
        }
        Self::iface().ok_or_else(|| "no Wi-Fi adapter".into())
    }

    /// The saved networks, from iwd's own files: their names are exact,
    /// where iwctl's table loses repeated spaces. iwctl is the fallback.
    fn known() -> Vec<String> {
        if let Ok(dir) = fs::read_dir(IWD_NETWORKS) {
            return dir.flatten().filter_map(|e| known_name(&e.file_name().to_string_lossy())).collect();
        }
        Self::iwctl(&["known-networks", "list"], 10)
            .map(|out| table_rows(&out).into_iter().filter_map(|(_, row)| columns(&row).into_iter().next()).collect())
            .unwrap_or_default()
    }
}

const IWD_NETWORKS: &str = "/var/lib/iwd";

/// The SSID of an iwd network file: `NAME.psk`, or `=HEX.psk` for a name
/// with other characters than letters, digits, space, `-` and `_`.
fn known_name(file: &str) -> Option<String> {
    let (stem, kind) = file.rsplit_once('.')?;
    if !matches!(kind, "psk" | "open" | "8021x") || stem.is_empty() {
        return None;
    }
    let Some(hex) = stem.strip_prefix('=') else { return Some(stem.to_string()) };
    let bytes: Option<Vec<u8>> = (0..hex.len())
        .step_by(2)
        .map(|i| hex.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok()))
        .collect();
    bytes.and_then(|b| String::from_utf8(b).ok())
}

/// Rows below the last dashed separator of an iwctl table, with the connected
/// marker split off and the text otherwise as printed.
fn table_rows(out: &str) -> Vec<(bool, String)> {
    let lines: Vec<&str> = out.lines().collect();
    let start = lines.iter().rposition(|l| l.trim_start().starts_with("----")).map_or(0, |i| i + 1);
    lines[start..]
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let t = l.trim_start();
            (t.starts_with('>'), t.trim_start_matches('>').trim().to_string())
        })
        .collect()
}

/// A `get-networks` row: name, security and signal. Security and signal are
/// single words, so they are taken from the right and the name keeps every
/// space inside it.
fn network_row(row: &str) -> Option<(String, String, String)> {
    let (rest, signal) = row.trim_end().rsplit_once(char::is_whitespace)?;
    let (ssid, security) = rest.trim_end().rsplit_once(char::is_whitespace)?;
    let ssid = ssid.trim();
    (!ssid.is_empty()).then(|| (ssid.to_string(), security.to_string(), signal.to_string()))
}

impl Wifi for Iwd {
    fn state(&mut self) -> Result<WifiState, String> {
        let on = Self::running();
        let iface = Self::iface();
        let mut state = WifiState { on, adapter: iface.is_some(), link: WifiLink::default() };
        let (true, Some(iface)) = (on, iface) else { return Ok(state) };
        // iwd needs a moment after starting before it lists the station.
        let Ok(out) = Self::iwctl(&["station", &iface, "show"], 5) else { return Ok(state) };
        for line in out.lines() {
            // The name is the rest of the line, with its spaces.
            if let Some(name) = line.trim().strip_prefix("Connected network") {
                state.link.ssid = Some(name.trim().to_string()).filter(|n| !n.is_empty());
                continue;
            }
            match columns(line).as_slice() {
                [k, v] if k == "RSSI" => {
                    let dbm: i32 = v.trim_end_matches(" dBm").trim().parse().unwrap_or(-100);
                    state.link.signal = Some(((dbm + 100) * 2).clamp(0, 100) as u8);
                }
                _ => {}
            }
        }
        Ok(state)
    }

    fn power(&mut self, on: bool) -> Result<(), String> {
        if !self.switchable {
            return Err("this system starts Wi-Fi itself".into());
        }
        switch("wifi", on)?;
        // Give iwd time to find the adapter before the next snapshot.
        for _ in 0..20 {
            if !on || Self::iwctl(&["station", "list"], 3).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        Ok(())
    }

    fn scan(&mut self) -> Result<Vec<WifiNetwork>, String> {
        let iface = Self::station()?;
        // A scan already in progress is fine; its results are read below.
        let _ = Self::iwctl(&["station", &iface, "scan"], 10);
        std::thread::sleep(Duration::from_secs(3));
        let out = Self::iwctl(&["station", &iface, "get-networks"], 10)?;
        let known = Self::known();
        Ok(table_rows(&out)
            .into_iter()
            .filter_map(|(_, row)| {
                let (ssid, security, signal) = network_row(&row)?;
                Some(WifiNetwork {
                    known: known.contains(&ssid),
                    ssid,
                    signal: (signal.chars().filter(|c| *c == '*').count() as u8 * 25).min(100),
                    secured: security != "open",
                })
            })
            .collect())
    }

    fn connect(&mut self, ssid: &str, passphrase: Option<&str>) -> Result<(), String> {
        let iface = Self::station()?;
        let mut args = Vec::new();
        if let Some(p) = passphrase {
            args.extend(["--passphrase", p]);
        }
        args.extend(["station", &iface, "connect", ssid]);
        Self::iwctl(&args, 40).map(|_| ())
    }

    fn disconnect(&mut self) -> Result<(), String> {
        let iface = Self::station()?;
        Self::iwctl(&["station", &iface, "disconnect"], 10).map(|_| ())
    }

    fn forget(&mut self, ssid: &str) -> Result<(), String> {
        Self::station()?;
        Self::iwctl(&["known-networks", ssid, "forget"], 10).map(|_| ())
    }
}

struct Sshd;

impl Sshd {
    fn detect() -> Result<Sshd, Missing> {
        if !Path::new(SWITCH).exists() {
            return Err(Missing::ManagedBySystem);
        }
        if !Path::new("/usr/sbin/sshd").exists() {
            return Err(Missing::NoService("openssh-server"));
        }
        Ok(Sshd)
    }
}

impl Ssh for Sshd {
    fn state(&mut self) -> Result<SshState, String> {
        let on = run(SWITCH, &["ssh"])?.trim() == "on";
        let running = active("ssh");
        let keys = running.then(|| {
            fs::read_to_string(SSH_KEYS)
                .map(|t| t.lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#')).count())
                .unwrap_or(0)
        });
        let password = run(SWITCH, &["ssh-password"]).ok().filter(|p| !p.is_empty());
        Ok(SshState { on, running, keys, password })
    }

    fn set(&mut self, on: bool) -> Result<(), String> {
        switch("ssh", on)
    }

    fn set_password(&mut self, password: &str) -> Result<(), String> {
        run(SWITCH, &["ssh-password", password]).map(|_| ())
    }
}

/// Power control is offered when the launcher runs as root, as the system's shell.
struct LinuxPower;

impl LinuxPower {
    fn detect() -> Option<LinuxPower> {
        (unsafe { libc::geteuid() } == 0 && on_path("systemctl")).then_some(LinuxPower)
    }
}

impl Power for LinuxPower {
    fn actions(&self) -> Vec<PowerAction> {
        vec![PowerAction::Reboot, PowerAction::PowerOff]
    }

    fn run(&mut self, action: PowerAction) -> Result<(), String> {
        let verb = match action {
            PowerAction::Reboot => "reboot",
            PowerAction::PowerOff => "poweroff",
        };
        run("systemctl", &[verb]).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iwctl_networks_table() {
        let out = "\
                               Available networks
--------------------------------------------------------------------------------
      Network name                      Security            Signal
--------------------------------------------------------------------------------
  \u{1b}[0m>\u{1b}[0m   Home Net                          psk                 ****
      Cafe                              open                **
      Home  Net 5G                      psk                 \u{1b}[1;90m*\u{1b}[0m***
";
        let rows = table_rows(&strip_ansi(out));
        assert_eq!(rows.len(), 3);
        assert!(rows[0].0 && !rows[1].0);
        let net = |i: usize| network_row(&rows[i].1).unwrap();
        assert_eq!(net(0), ("Home Net".to_string(), "psk".into(), "****".into()));
        assert_eq!(net(1), ("Cafe".to_string(), "open".into(), "**".into()));
        // Repeated spaces stay in the name.
        assert_eq!(net(2).0, "Home  Net 5G");
    }

    #[test]
    fn names_iwd_network_files() {
        assert_eq!(known_name("Home  Net.psk").as_deref(), Some("Home  Net"));
        assert_eq!(known_name("Cafe.open").as_deref(), Some("Cafe"));
        // "小米" in UTF-8.
        assert_eq!(known_name("=e5b08fe7b1b3.psk").as_deref(), Some("小米"));
        assert_eq!(known_name("notes.txt"), None);
        assert_eq!(known_name("=zz.psk"), None);
    }
}
