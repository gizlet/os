//! The device around the launcher. Every peripheral is detected at runtime;
//! what is missing is reported as [`Missing`], so a page can say so instead
//! of failing.
//!
//! Device access runs on one worker thread: pages send [`Request`]s and poll
//! [`Event`]s once a frame, so slow commands such as a Wi-Fi scan never stop
//! drawing. `GIZLET_FAKE_DEVICES=1` simulates every device, for working on
//! the interface on a computer.

mod fake;
mod info;
mod load;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(target_os = "linux"))]
mod other;
mod util;

pub use info::{Info, Storage};
pub use load::{Load, LoadMeter};

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

/// Why a device cannot be used.
#[derive(Clone, Debug, PartialEq)]
pub enum Missing {
    /// No such hardware was found.
    NoHardware,
    /// The hardware exists but the named system component is absent.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    NoService(&'static str),
    /// The operating system manages this itself, as on a desktop.
    ManagedBySystem,
    /// Detected but failing.
    Error(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Charge {
    Charging,
    Discharging,
    Full,
    /// On external power but not charging, as when the charger is too weak.
    Plugged,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Battery {
    pub percent: u8,
    pub charge: Charge,
    /// Milliwatts leaving the battery, or entering it while charging, when the
    /// gauge reports them.
    pub power_mw: Option<u32>,
    /// Minutes until empty at the present draw, while on battery.
    pub minutes_left: Option<u32>,
}

impl Battery {
    /// On external power: charging, full or plugged in.
    pub fn charging(self) -> bool {
        self.charge != Charge::Discharging
    }
}

/// The battery's state from its `status`. "Not charging" means external
/// power, and the R36S's RK817 reports it, with its charger offline, while it
/// charges: then the current flowing in decides.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn charge_state(status: Option<&str>, supply_online: bool, inflow: bool) -> Charge {
    match status {
        Some("Charging") => Charge::Charging,
        Some("Full") => Charge::Full,
        Some("Not charging") if inflow => Charge::Charging,
        Some("Not charging") => Charge::Plugged,
        _ if supply_online && inflow => Charge::Charging,
        _ if supply_online => Charge::Plugged,
        _ => Charge::Discharging,
    }
}

#[cfg(test)]
mod charge_tests {
    use super::*;

    #[test]
    fn reads_the_charge_state() {
        assert_eq!(charge_state(Some("Charging"), false, false), Charge::Charging);
        assert_eq!(charge_state(Some("Full"), true, false), Charge::Full);
        // The RK817 while it charges, and with a charger too weak to.
        assert_eq!(charge_state(Some("Not charging"), false, true), Charge::Charging);
        assert_eq!(charge_state(Some("Not charging"), false, false), Charge::Plugged);
        assert_eq!(charge_state(Some("Discharging"), true, true), Charge::Charging);
        assert_eq!(charge_state(Some("Discharging"), false, false), Charge::Discharging);
        assert_eq!(charge_state(Some("Discharging"), false, true), Charge::Discharging, "status wins without a supply");
        assert_eq!(charge_state(None, false, false), Charge::Discharging);
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct WifiLink {
    pub ssid: Option<String>,
    /// 0–100.
    pub signal: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct WifiState {
    /// The Wi-Fi service is running.
    pub on: bool,
    /// A wireless adapter is present.
    pub adapter: bool,
    pub link: WifiLink,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WifiNetwork {
    pub ssid: String,
    /// 0–100.
    pub signal: u8,
    pub secured: bool,
    pub known: bool,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct SshState {
    /// Turned on in the launcher; it starts again at boot.
    pub on: bool,
    pub running: bool,
    /// Public keys accepted, once the server has started.
    pub keys: Option<usize>,
    /// root's password for ssh logins, when one is set.
    pub password: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ClockState {
    /// The time zone, as a name under /usr/share/zoneinfo such as `Asia/Shanghai`.
    pub zone: String,
    /// Network time (systemd-timesyncd) has set the clock since boot.
    pub synced: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerAction {
    Sleep,
    Reboot,
    PowerOff,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Refresh,
    WifiPower(bool),
    WifiScan,
    WifiConnect { ssid: String, passphrase: Option<String> },
    WifiDisconnect,
    WifiForget(String),
    SetBrightness(f32),
    SetVolume(f32),
    Ssh(bool),
    SshPassword(String),
    TimeZone(String),
    /// A name for the device, or an empty one to go back to the default.
    Hostname(String),
    Power(PowerAction),
}

#[derive(Clone, Debug)]
pub enum Event {
    Snapshot(Box<Snapshot>),
    /// The output volume was just set, so a sound played now is heard at the new level.
    Volume,
    WifiNetworks(Result<Vec<WifiNetwork>, String>),
    WifiConnect { ssid: String, result: Result<(), String> },
    Failed(String),
}

/// Everything the pages show about the device.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub battery: Result<Battery, Missing>,
    pub brightness: Result<f32, Missing>,
    pub volume: Result<f32, Missing>,
    pub wifi: Result<WifiState, Missing>,
    pub ssh: Result<SshState, Missing>,
    pub clock: Result<ClockState, Missing>,
    pub power: Vec<PowerAction>,
    pub info: Info,
    pub load: Load,
    /// The device's name now; `NAME.local` on the network where it can be renamed.
    pub hostname: String,
    /// The launcher can rename the device.
    pub can_rename: bool,
    pub ip: Option<String>,
    pub storage: Option<Storage>,
    pub simulated: bool,
    /// The `SetBrightness` and `SetVolume` requests the worker had received
    /// when it took this snapshot. A page that has sent more keeps its own
    /// value: the snapshot was read before the latest change reached the device.
    pub brightness_sets: u64,
    pub volume_sets: u64,
}

pub trait BatterySource: Send {
    fn read(&mut self) -> Result<Battery, Missing>;
}

/// A 0–1 control such as the backlight or the output volume.
pub trait Level: Send {
    fn get(&mut self) -> Option<f32>;
    fn set(&mut self, value: f32) -> Result<(), String>;
}

pub trait Wifi: Send {
    fn state(&mut self) -> Result<WifiState, String>;
    fn power(&mut self, on: bool) -> Result<(), String>;
    fn scan(&mut self) -> Result<Vec<WifiNetwork>, String>;
    fn connect(&mut self, ssid: &str, passphrase: Option<&str>) -> Result<(), String>;
    fn disconnect(&mut self) -> Result<(), String>;
    fn forget(&mut self, ssid: &str) -> Result<(), String>;
}

pub trait Ssh: Send {
    fn state(&mut self) -> Result<SshState, String>;
    fn set(&mut self, on: bool) -> Result<(), String>;
    fn set_password(&mut self, password: &str) -> Result<(), String>;
}

pub trait Power: Send {
    fn actions(&self) -> Vec<PowerAction>;
    fn run(&mut self, action: PowerAction) -> Result<(), String>;
}

pub trait Clock: Send {
    fn state(&mut self) -> Result<ClockState, String>;
    fn set_zone(&mut self, zone: &str) -> Result<(), String>;
}

/// The device's name, kept across reboots.
pub trait Host: Send {
    fn name(&mut self) -> String;
    /// Sets a name [`valid_hostname`] accepts, or the default for an empty one.
    fn set_name(&mut self, name: &str) -> Result<(), String>;
}

/// One DNS label, as `gizlet-switch hostname` accepts it once lowercased: 1 to
/// 63 letters, digits and hyphens, neither starting nor ending with a hyphen.
pub fn valid_hostname(name: &str) -> bool {
    (1..=63).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}

pub struct Devices {
    pub battery: Box<dyn BatterySource>,
    pub backlight: Result<Box<dyn Level>, Missing>,
    pub volume: Result<Box<dyn Level>, Missing>,
    pub wifi: Result<Box<dyn Wifi>, Missing>,
    pub ssh: Result<Box<dyn Ssh>, Missing>,
    pub clock: Result<Box<dyn Clock>, Missing>,
    /// None where the system names itself.
    pub host: Option<Box<dyn Host>>,
    pub power: Option<Box<dyn Power>>,
    pub simulated: bool,
    pub load: LoadMeter,
}

impl Devices {
    fn detect() -> Devices {
        if std::env::var("GIZLET_FAKE_DEVICES").is_ok_and(|v| v == "1") {
            return fake::devices();
        }
        #[cfg(target_os = "linux")]
        return linux::devices();
        #[cfg(not(target_os = "linux"))]
        return other::devices();
    }
}

/// The handle to the device worker.
pub struct System {
    tx: Sender<Request>,
    rx: Receiver<Event>,
}

impl System {
    /// Detects devices and starts the worker. `storage_root` is the folder whose disk is reported.
    pub fn start(storage_root: PathBuf) -> System {
        let (req_tx, req_rx) = mpsc::channel::<Request>();
        let (ev_tx, ev_rx) = mpsc::channel::<Event>();
        std::thread::Builder::new()
            .name("system".into())
            .spawn(move || worker(Devices::detect(), storage_root, req_rx, ev_tx))
            .expect("spawn the system worker");
        System { tx: req_tx, rx: ev_rx }
    }

    pub fn send(&self, request: Request) {
        let _ = self.tx.send(request);
    }

    pub fn poll(&self) -> Vec<Event> {
        self.rx.try_iter().collect()
    }
}

/// A snapshot taken now, for `--probe`.
pub fn probe(storage_root: PathBuf) -> Snapshot {
    snapshot(&mut Devices::detect(), &Info::detect(), &storage_root)
}

const REFRESH: Duration = Duration::from_secs(4);

fn level(dev: &mut Result<Box<dyn Level>, Missing>) -> Result<f32, Missing> {
    match dev {
        Ok(l) => l.get().ok_or(Missing::Error("unreadable".into())),
        Err(m) => Err(m.clone()),
    }
}

fn snapshot(dev: &mut Devices, info: &Info, storage_root: &std::path::Path) -> Snapshot {
    Snapshot {
        battery: dev.battery.read(),
        brightness: level(&mut dev.backlight),
        volume: level(&mut dev.volume),
        wifi: match &mut dev.wifi {
            Ok(w) => w.state().map_err(Missing::Error),
            Err(m) => Err(m.clone()),
        },
        ssh: match &mut dev.ssh {
            Ok(s) => s.state().map_err(Missing::Error),
            Err(m) => Err(m.clone()),
        },
        clock: match &mut dev.clock {
            Ok(c) => c.state().map_err(Missing::Error),
            Err(m) => Err(m.clone()),
        },
        power: dev.power.as_ref().map(|p| p.actions()).unwrap_or_default(),
        info: info.clone(),
        load: dev.load.read(dev.simulated),
        // Read each time: it may be changed over ssh, or by a Wi-Fi adapter appearing.
        hostname: dev.host.as_mut().map_or_else(info::hostname, |h| h.name()),
        can_rename: dev.host.is_some(),
        ip: info::local_ip(),
        storage: info::storage(storage_root),
        simulated: dev.simulated,
        brightness_sets: 0,
        volume_sets: 0,
    }
}

fn unavailable(m: &Missing) -> String {
    match m {
        Missing::NoService(s) => format!("{s} is missing"),
        Missing::Error(e) => e.clone(),
        _ => "unavailable".into(),
    }
}

fn worker(mut dev: Devices, storage_root: PathBuf, rx: Receiver<Request>, tx: Sender<Event>) {
    let info = Info::detect();
    if tx.send(Event::Snapshot(Box::new(snapshot(&mut dev, &info, &storage_root)))).is_err() {
        return;
    }
    let (mut brightness_sets, mut volume_sets) = (0u64, 0u64);
    let report = |dev: &mut Devices, brightness_sets: u64, volume_sets: u64| {
        let mut s = snapshot(dev, &info, &storage_root);
        s.brightness_sets = brightness_sets;
        s.volume_sets = volume_sets;
        Event::Snapshot(Box::new(s))
    };
    loop {
        let first = match rx.recv_timeout(REFRESH) {
            Ok(r) => r,
            Err(RecvTimeoutError::Timeout) => Request::Refresh,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        // Sliders send many values; only the latest of each kind matters.
        let mut batch = vec![first];
        batch.extend(rx.try_iter());
        let last_brightness = batch.iter().rposition(|r| matches!(r, Request::SetBrightness(_)));
        let last_volume = batch.iter().rposition(|r| matches!(r, Request::SetVolume(_)));
        brightness_sets += batch.iter().filter(|r| matches!(r, Request::SetBrightness(_))).count() as u64;
        volume_sets += batch.iter().filter(|r| matches!(r, Request::SetVolume(_))).count() as u64;
        let mut refresh = false;
        for (i, req) in batch.into_iter().enumerate() {
            let failed = |r: Result<(), String>| r.err().map(Event::Failed);
            let event = match req {
                Request::Refresh => {
                    refresh = true;
                    None
                }
                Request::SetBrightness(v) if Some(i) == last_brightness => {
                    refresh = true;
                    dev.backlight.as_mut().ok().and_then(|l| failed(l.set(v.clamp(0.0, 1.0))))
                }
                Request::SetVolume(v) if Some(i) == last_volume => {
                    refresh = true;
                    let v = v.clamp(0.0, 1.0);
                    match dev.volume.as_mut() {
                        Ok(l) => Some(l.set(v).map_or_else(Event::Failed, |()| Event::Volume)),
                        Err(_) => None,
                    }
                }
                Request::SetBrightness(_) | Request::SetVolume(_) => None,
                Request::WifiPower(on) => {
                    refresh = true;
                    match &mut dev.wifi {
                        Ok(w) => failed(w.power(on)),
                        Err(m) => Some(Event::Failed(unavailable(m))),
                    }
                }
                Request::WifiScan => Some(Event::WifiNetworks(match &mut dev.wifi {
                    Ok(w) => w.scan(),
                    Err(m) => Err(unavailable(m)),
                })),
                Request::WifiConnect { ssid, passphrase } => {
                    refresh = true;
                    let result = match &mut dev.wifi {
                        Ok(w) => w.connect(&ssid, passphrase.as_deref()),
                        Err(m) => Err(unavailable(m)),
                    };
                    Some(Event::WifiConnect { ssid, result })
                }
                Request::WifiDisconnect => {
                    refresh = true;
                    dev.wifi.as_mut().ok().and_then(|w| failed(w.disconnect()))
                }
                Request::WifiForget(ssid) => {
                    refresh = true;
                    dev.wifi.as_mut().ok().and_then(|w| failed(w.forget(&ssid)))
                }
                Request::Ssh(on) => {
                    refresh = true;
                    match &mut dev.ssh {
                        Ok(s) => failed(s.set(on)),
                        Err(m) => Some(Event::Failed(unavailable(m))),
                    }
                }
                Request::SshPassword(password) => {
                    refresh = true;
                    match &mut dev.ssh {
                        Ok(s) => failed(s.set_password(&password)),
                        Err(m) => Some(Event::Failed(unavailable(m))),
                    }
                }
                Request::TimeZone(zone) => {
                    refresh = true;
                    match &mut dev.clock {
                        Ok(c) => failed(c.set_zone(&zone)),
                        Err(m) => Some(Event::Failed(unavailable(m))),
                    }
                }
                Request::Hostname(name) => {
                    refresh = true;
                    match dev.host.as_mut() {
                        Some(h) => failed(h.set_name(&name)),
                        None => Some(Event::Failed("the system names this device itself".into())),
                    }
                }
                Request::Power(action) => match dev.power.as_mut() {
                    Some(p) => failed(p.run(action)),
                    None => Some(Event::Failed("power control is unavailable".into())),
                },
            };
            if let Some(event) = event {
                if tx.send(event).is_err() {
                    return;
                }
            }
        }
        if refresh && tx.send(report(&mut dev, brightness_sets, volume_sets)).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_count_the_level_requests_they_include() {
        let (req_tx, req_rx) = mpsc::channel();
        let (ev_tx, ev_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || worker(fake::devices(), std::env::temp_dir(), req_rx, ev_tx));
        for v in [0.3, 0.35, 0.4] {
            req_tx.send(Request::SetBrightness(v)).unwrap();
        }
        req_tx.send(Request::SetVolume(0.6)).unwrap();
        let mut heard = false;
        let last = loop {
            match ev_rx.recv_timeout(Duration::from_secs(10)).expect("a report") {
                Event::Volume => heard = true,
                Event::Snapshot(s) if s.brightness_sets == 3 && s.volume_sets == 1 => break s,
                Event::Snapshot(s) => assert!(s.brightness_sets <= 3 && s.volume_sets <= 1),
                _ => {}
            }
        };
        assert_eq!(last.brightness, Ok(0.4), "the latest brightness is applied");
        assert_eq!(last.volume, Ok(0.6));
        assert!(heard, "the volume is reported once set");
        drop(req_tx);
        worker.join().unwrap();
    }

    #[test]
    fn accepts_one_dns_label() {
        for name in ["gizlet-a1b2", "r36s", "a", &"a".repeat(63)] {
            assert!(valid_hostname(name), "{name}");
        }
        for name in ["", "-x", "x-", "a_b", "Ab", "a.b", "小米", &"a".repeat(64)] {
            assert!(!valid_hostname(name), "{name}");
        }
    }
}
