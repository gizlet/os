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
#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(target_os = "linux"))]
mod other;
mod util;

pub use info::{Info, Storage};

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
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Battery {
    pub percent: u8,
    pub charge: Charge,
}

impl Battery {
    pub fn charging(self) -> bool {
        self.charge != Charge::Discharging
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerAction {
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
    Power(PowerAction),
}

#[derive(Clone, Debug)]
pub enum Event {
    Snapshot(Box<Snapshot>),
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
    pub power: Vec<PowerAction>,
    pub info: Info,
    pub ip: Option<String>,
    pub storage: Option<Storage>,
    pub simulated: bool,
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
}

pub trait Power: Send {
    fn actions(&self) -> Vec<PowerAction>;
    fn run(&mut self, action: PowerAction) -> Result<(), String>;
}

pub struct Devices {
    pub battery: Box<dyn BatterySource>,
    pub backlight: Result<Box<dyn Level>, Missing>,
    pub volume: Result<Box<dyn Level>, Missing>,
    pub wifi: Result<Box<dyn Wifi>, Missing>,
    pub ssh: Result<Box<dyn Ssh>, Missing>,
    pub power: Option<Box<dyn Power>>,
    pub simulated: bool,
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
        power: dev.power.as_ref().map(|p| p.actions()).unwrap_or_default(),
        info: info.clone(),
        ip: info::local_ip(),
        storage: info::storage(storage_root),
        simulated: dev.simulated,
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
                    dev.volume.as_mut().ok().and_then(|l| failed(l.set(v.clamp(0.0, 1.0))))
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
        if refresh && tx.send(Event::Snapshot(Box::new(snapshot(&mut dev, &info, &storage_root)))).is_err() {
            return;
        }
    }
}
