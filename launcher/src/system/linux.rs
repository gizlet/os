//! Linux devices through kernel interfaces and standard tools: sysfs for the
//! battery and backlight, ALSA's `amixer` for volume, iwd's `iwctl` for
//! Wi-Fi and systemd for ssh, network time and power.
//!
//! On Gizlet OS, Wi-Fi and ssh are switched and the time zone is set with
//! `gizlet-switch`, which keeps the choice on the data partition so it holds
//! again after a reboot.

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
        clock: GizletClock::detect().map(|c| Box::new(c) as Box<dyn Clock>),
        power: LinuxPower::detect().map(|p| Box::new(p) as Box<dyn Power>),
        simulated: false,
        load: LoadMeter::default(),
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
        let num = |name: &str| read_trim(&dir.join(name)).and_then(|v| v.parse::<i64>().ok());
        let first = |names: &[&str]| names.iter().find_map(|n| num(n));
        // Gauges report µA and µV, some µW; the sign of the current differs
        // between drivers. The averages are steadier than the instant values.
        let current = first(&["current_avg", "current_now"]).map(i64::abs).filter(|c| *c > 0);
        let power_uw = first(&["power_avg", "power_now"]).map(i64::abs).filter(|p| *p > 0).or_else(|| {
            let volts = first(&["voltage_avg", "voltage_now"])?;
            Some(current? * volts / 1_000_000)
        });
        let minutes_left = match charge {
            Charge::Discharging => num("time_to_empty_avg")
                .or_else(|| num("time_to_empty_now"))
                .filter(|s| *s > 0)
                .map(|s| s / 60)
                .or_else(|| Some(first(&["charge_now"])? * 60 / current?))
                .map(|m| m.clamp(0, u32::MAX as i64) as u32),
            _ => None,
        };
        Ok(Battery { percent, charge, power_mw: power_uw.map(|p| (p / 1000) as u32), minutes_left })
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

/// Wi-Fi through iwd: its D-Bus objects, read with busctl, for exact network
/// names, and its command-line client for the rest.
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

    /// iwd's station object for `iface`, such as `/net/connman/iwd/0/4`.
    fn dbus_station(iface: &str) -> Option<String> {
        let tree = busctl(&["--list", "tree", IWD_BUS]).ok()?;
        tree.lines().map(str::trim).filter(|p| p.split('/').filter(|c| !c.is_empty()).count() == 5).find_map(|path| {
            let name = busctl(&["get-property", IWD_BUS, path, "net.connman.iwd.Device", "Name"]).ok()?;
            (busctl_values(&name).get(1).map(String::as_str) == Some(iface)).then(|| path.to_string())
        })
    }

    /// The networks iwd sees from `station`, strongest first: (object path, signal in dBm).
    fn dbus_networks(station: &str) -> Option<Vec<(String, i32)>> {
        let out = busctl(&["call", IWD_BUS, station, "net.connman.iwd.Station", "GetOrderedNetworks"]).ok()?;
        // a(on) COUNT "PATH" SIGNAL ...; the signal is in hundredths of a dBm.
        let values = busctl_values(&out);
        Some(values.get(2..)?.chunks(2).filter_map(|c| Some((c.first()?.clone(), c.get(1)?.parse::<i32>().ok()? / 100))).collect())
    }

    fn dbus_connected(station: &str) -> Option<String> {
        let out = busctl(&["get-property", IWD_BUS, station, "net.connman.iwd.Station", "ConnectedNetwork"]).ok()?;
        busctl_values(&out).get(1).cloned()
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
const IWD_BUS: &str = "net.connman.iwd";

fn busctl(args: &[&str]) -> Result<String, String> {
    run_for(Duration::from_secs(5), "busctl", args)
}

/// The values busctl prints, such as `s "wlan0"` or `a(on) 1 "/x" -5100`:
/// words, and double-quoted strings with C escapes undone.
fn busctl_values(out: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut chars = out.trim().chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '"' {
            chars.next();
            let mut v = String::new();
            while let Some(c) = chars.next() {
                match c {
                    '"' => break,
                    '\\' => v.extend(chars.next()),
                    c => v.push(c),
                }
            }
            values.push(v);
        } else {
            let mut v = String::new();
            while let Some(&c) = chars.peek().filter(|c| !c.is_whitespace()) {
                v.push(c);
                chars.next();
            }
            values.push(v);
        }
    }
    values
}

/// A network object `/net/connman/iwd/0/4/HEX_TYPE`: its SSID, exactly, from
/// the hex bytes, and whether it is secured.
fn network_object(path: &str) -> Option<(String, bool)> {
    let (hex, kind) = path.rsplit('/').next()?.rsplit_once('_')?;
    (hex.len() % 2 == 0 && !hex.is_empty()).then_some(())?;
    let bytes: Option<Vec<u8>> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok()).collect();
    Some((String::from_utf8(bytes?).ok()?, kind != "open"))
}

fn signal_percent(dbm: i32) -> u8 {
    ((dbm + 100) * 2).clamp(0, 100) as u8
}

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

/// Rows below the last dashed separator of an iwctl table, for when iwd's
/// D-Bus objects cannot be read. The first column starts where the header's
/// first title does: what comes before it is the connected marker, and the
/// rest is the row as printed, so a name may start with `>` or a space. The
/// table pads names, so trailing spaces in one cannot be told apart.
fn table_rows(out: &str) -> Vec<(bool, String)> {
    let lines: Vec<&str> = out.lines().collect();
    let separator = |l: &&str| l.trim_start().starts_with("----");
    let start = lines.iter().rposition(separator).map_or(0, |i| i + 1);
    let column = lines[..start.saturating_sub(1)]
        .iter()
        .rev()
        .find(|l| !l.trim().is_empty() && !separator(l))
        .and_then(|h| h.chars().position(|c| !c.is_whitespace()));
    lines[start..]
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| match column {
            Some(c) if l.chars().count() > c => {
                let marker: String = l.chars().take(c).collect();
                (marker.contains('>'), l.chars().skip(c).collect::<String>().trim_end().to_string())
            }
            _ => {
                let t = l.trim_start();
                (t.starts_with('>'), t.trim_start_matches('>').trim().to_string())
            }
        })
        .collect()
}

/// A `get-networks` row: name, security and signal. Security and signal are
/// single words, so they are taken from the right and the name keeps every
/// space inside it.
fn network_row(row: &str) -> Option<(String, String, String)> {
    let (rest, signal) = row.trim_end().rsplit_once(char::is_whitespace)?;
    let (ssid, security) = rest.trim_end().rsplit_once(char::is_whitespace)?;
    let ssid = ssid.trim_end();
    (!ssid.trim().is_empty()).then(|| (ssid.to_string(), security.to_string(), signal.to_string()))
}

impl Wifi for Iwd {
    fn state(&mut self) -> Result<WifiState, String> {
        let on = Self::running();
        let iface = Self::iface();
        let mut state = WifiState { on, adapter: iface.is_some(), link: WifiLink::default() };
        let (true, Some(iface)) = (on, iface) else { return Ok(state) };
        if let Some(station) = Self::dbus_station(&iface) {
            if let Some(path) = Self::dbus_connected(&station) {
                state.link.ssid = network_object(&path).map(|(ssid, _)| ssid);
                let networks = Self::dbus_networks(&station).unwrap_or_default();
                state.link.signal = networks.iter().find(|(p, _)| *p == path).map(|(_, dbm)| signal_percent(*dbm));
            }
            return Ok(state);
        }
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
                    state.link.signal = Some(signal_percent(dbm));
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
        let known = Self::known();
        if let Some(networks) = Self::dbus_station(&iface).and_then(|s| Self::dbus_networks(&s)) {
            return Ok(networks
                .into_iter()
                .filter_map(|(path, dbm)| {
                    let (ssid, secured) = network_object(&path)?;
                    Some(WifiNetwork { known: known.contains(&ssid), ssid, signal: signal_percent(dbm), secured })
                })
                .collect());
        }
        let out = Self::iwctl(&["station", &iface, "get-networks"], 10)?;
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

/// The time zone through `gizlet-switch`; elsewhere the system sets it.
struct GizletClock;

/// systemd-timesyncd creates this once it has set the clock from the network.
const TIME_SYNCED: &str = "/run/systemd/timesync/synchronized";

impl GizletClock {
    fn detect() -> Result<GizletClock, Missing> {
        if !Path::new(SWITCH).exists() {
            return Err(Missing::ManagedBySystem);
        }
        Ok(GizletClock)
    }
}

impl Clock for GizletClock {
    fn state(&mut self) -> Result<ClockState, String> {
        let zone = run(SWITCH, &["timezone"])?.trim().to_string();
        Ok(ClockState { zone, synced: Path::new(TIME_SYNCED).exists() })
    }

    fn set_zone(&mut self, zone: &str) -> Result<(), String> {
        run(SWITCH, &["timezone", zone]).map(|_| ())
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
    /// Sleep when the kernel can suspend to RAM and gizlet-power, which does
    /// it, runs (it needs a backlight).
    fn actions(&self) -> Vec<PowerAction> {
        let can_sleep = read_trim(Path::new("/sys/power/state")).is_some_and(|s| s.split(' ').any(|w| w == "mem"))
            && active("gizlet-power");
        let mut actions = if can_sleep { vec![PowerAction::Sleep] } else { Vec::new() };
        actions.extend([PowerAction::Reboot, PowerAction::PowerOff]);
        actions
    }

    /// gizlet-power puts the device to sleep, so the press that wakes it
    /// reaches no app.
    fn run(&mut self, action: PowerAction) -> Result<(), String> {
        let args: &[&str] = match action {
            PowerAction::Sleep => &["kill", "--kill-whom=main", "--signal=SIGUSR1", "gizlet-power.service"],
            PowerAction::Reboot => &["reboot"],
            PowerAction::PowerOff => &["poweroff"],
        };
        run("systemctl", args).map(|_| ())
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
      >Home                             psk                 **
       Lead                             open                *
";
        let rows = table_rows(&strip_ansi(out));
        assert_eq!(rows.len(), 5);
        assert!(rows[0].0 && !rows[1].0);
        let net = |i: usize| network_row(&rows[i].1).unwrap();
        assert_eq!(net(0), ("Home Net".to_string(), "psk".into(), "****".into()));
        assert_eq!(net(1), ("Cafe".to_string(), "open".into(), "**".into()));
        // Repeated spaces stay in the name.
        assert_eq!(net(2).0, "Home  Net 5G");
        // A name that starts with > is not the connected marker, and a leading space stays.
        assert!(!rows[3].0);
        assert_eq!(net(3).0, ">Home");
        assert_eq!(net(4).0, " Lead");
    }

    #[test]
    fn reads_iwd_dbus_objects() {
        let out = r#"a(on) 2 "/net/connman/iwd/0/4/3e486f6d6520_psk" -5100 "/net/connman/iwd/0/4/e5b08fe7b1b3_open" -7000"#;
        let v = busctl_values(out);
        assert_eq!(v.len(), 6);
        assert_eq!(network_object(&v[2]), Some((">Home ".to_string(), true)));
        assert_eq!(network_object(&v[4]), Some(("小米".to_string(), false)));
        assert_eq!(v[3].parse::<i32>().unwrap() / 100, -51);
        assert_eq!(busctl_values(r#"s "a \"b\"""#), ["s", "a \"b\""]);
        assert_eq!(network_object("/net/connman/iwd/0/4"), None);
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
