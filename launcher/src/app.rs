//! The launcher's state and what each control does. Pages in `ui` only draw it.

use crate::apps::{self, Library};
use crate::clock;
use crate::i18n::{human_size, T};
use crate::input::{Action, Input, TextEvent};
use crate::keyboard::{self, Keyboard};
use crate::settings::{Language, Power, Settings, Theme};
use crate::sound::Sfx;
use crate::system::{valid_hostname, Event, Missing, PowerAction, Request, Snapshot, System, WifiNetwork};
use crate::zones::{Zone, ZONES};
use gizlet_launcher::ota;
use gizlet_launcher::services::{self, Service};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

pub const TOAST_SECONDS: f32 = 2.8;
/// How long Back is held to leave the button test.
pub const TEST_EXIT_SECONDS: f32 = 1.2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Home,
    Services,
    Settings,
    ButtonTest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HomeRow {
    Tiles,
    Buttons,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HomeButton {
    Library,
    Services,
    Store,
    ButtonTest,
    Settings,
    Power,
}

impl HomeButton {
    pub const ALL: [HomeButton; 6] =
        [HomeButton::Library, HomeButton::Services, HomeButton::Store, HomeButton::ButtonTest, HomeButton::Settings, HomeButton::Power];

    pub fn title(self) -> T {
        match self {
            HomeButton::Library => T::Library,
            HomeButton::Services => T::Services,
            HomeButton::Store => T::Store,
            HomeButton::ButtonTest => T::ButtonTest,
            HomeButton::Settings => T::Settings,
            HomeButton::Power => T::Power,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Network,
    Ssh,
    Display,
    Sound,
    Battery,
    Controls,
    Language,
    Time,
    System,
}

impl Category {
    pub const ALL: [Category; 9] = [
        Category::Network,
        Category::Ssh,
        Category::Display,
        Category::Sound,
        Category::Battery,
        Category::Controls,
        Category::Language,
        Category::Time,
        Category::System,
    ];

    pub fn title(self) -> T {
        match self {
            Category::Network => T::CatNetwork,
            Category::Ssh => T::CatSsh,
            Category::Display => T::CatDisplay,
            Category::Sound => T::CatSound,
            Category::Battery => T::CatBattery,
            Category::Controls => T::CatControls,
            Category::Language => T::CatLanguage,
            Category::Time => T::CatTime,
            Category::System => T::CatSystem,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ItemId {
    None,
    WifiPower,
    WifiNetwork(String),
    WifiScan,
    WifiDisconnect,
    Ssh,
    SshPassword,
    Brightness,
    Theme,
    SleepAfter,
    SleepAfterCharging,
    Volume,
    UiSounds,
    BatteryPercent,
    ButtonTest,
    SwapAb,
    Language(Language),
    Clock24,
    TimeZone(String),
    Hostname,
    CheckUpdate,
    InstallUpdate,
    RestartUpdate,
    /// Whether the service runs now.
    ServiceRunning(String),
    /// Whether the service starts at boot.
    ServiceBoot(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ItemValue {
    /// A heading; not focusable.
    Section,
    /// A message with an optional second line; not focusable.
    Notice { body: String },
    /// A label with a value on the right.
    Info(String),
    Button,
    Toggle(bool),
    Slider(f32),
    Radio(bool),
    Choice(String),
    Network { signal: u8, secured: bool, status: Option<String> },
    /// A level bar with its value written above it; not focusable.
    Bar { value: String, fraction: f32, warn: bool },
    /// A line with a spinner; not focusable.
    Busy,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: ItemId,
    pub label: String,
    pub value: ItemValue,
}

impl Item {
    fn new(id: ItemId, label: impl Into<String>, value: ItemValue) -> Item {
        Item { id, label: label.into(), value }
    }

    fn info(label: impl Into<String>, value: impl Into<String>) -> Item {
        Item::new(ItemId::None, label, ItemValue::Info(value.into()))
    }

    fn section(label: impl Into<String>) -> Item {
        Item::new(ItemId::None, label, ItemValue::Section)
    }

    fn notice(label: impl Into<String>, body: impl Into<String>) -> Item {
        Item::new(ItemId::None, label, ItemValue::Notice { body: body.into() })
    }

    pub fn focusable(&self) -> bool {
        !matches!(self.value, ItemValue::Section | ItemValue::Notice { .. } | ItemValue::Busy | ItemValue::Bar { .. })
    }
}

#[derive(Clone, Debug, Default)]
pub struct SettingsFocus {
    pub category: usize,
    pub item: usize,
    pub in_content: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DialogAction {
    Power(PowerAction),
    Quit,
    WifiDisconnect,
    WifiForget(String),
    InstallUpdate,
    Close,
}

pub struct Dialog {
    pub title: String,
    pub buttons: Vec<(String, DialogAction)>,
    pub focus: usize,
    pub opened: f64,
}

pub struct Toast {
    pub text: String,
    pub age: f32,
}

#[derive(Default)]
pub struct WifiUi {
    pub scanning: bool,
    pub networks: Option<Vec<WifiNetwork>>,
    pub connecting: Option<String>,
    /// Turning Wi-Fi on or off, until the device reports it.
    pub switching: Option<bool>,
}

enum UpdateState {
    Idle,
    Checking,
    Current,
    Available(ota::Offer),
    Installing(ota::Progress),
    Failed(bool),
}

enum UpdateEvent {
    Installed(ota::Installed),
    Checked(Result<Option<ota::Offer>, String>),
    Installing(ota::Progress),
    Staged(Result<String, String>),
}

struct Updates {
    installed: Option<ota::Installed>,
    work: Option<Receiver<UpdateEvent>>,
    state: UpdateState,
}

impl Updates {
    fn new() -> Option<Self> {
        if !cfg!(feature = "gizlet-os") || !std::path::Path::new("/usr/lib/gizlet/gizlet-ota-boot").is_file() {
            return None;
        }
        let (send, work) = mpsc::channel();
        std::thread::spawn(move || { let _ = send.send(UpdateEvent::Installed(ota::installed())); });
        Some(Self { installed: None, work: Some(work), state: UpdateState::Idle })
    }

    fn check(&mut self) {
        if self.work.is_some() { return; }
        let Some(installed) = self.installed.clone() else { return };
        let (send, work) = mpsc::channel();
        self.state = UpdateState::Checking;
        self.work = Some(work);
        std::thread::spawn(move || { let _ = send.send(UpdateEvent::Checked(ota::check(&installed))); });
    }

    fn install(&mut self) {
        if self.work.is_some() { return; }
        let UpdateState::Available(offer) = &self.state else { return };
        let offer = offer.clone();
        let (send, work) = mpsc::channel();
        self.state = UpdateState::Installing(ota::Progress::Runtime(0));
        self.work = Some(work);
        std::thread::spawn(move || {
            let result = ota::install(&offer, |phase| { let _ = send.send(UpdateEvent::Installing(phase)); });
            let _ = send.send(UpdateEvent::Staged(result.map(|()| offer.tag)));
        });
    }

    fn has_spinner(&self) -> bool {
        self.work.is_some()
            && (self.installed.is_none()
                || matches!(self.state, UpdateState::Checking | UpdateState::Installing(ota::Progress::Verify)))
    }

    fn poll(&mut self) -> bool {
        let Some(work) = &self.work else { return false };
        let event = match work.try_recv() {
            Ok(event) => event,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.state = UpdateState::Failed(matches!(self.state, UpdateState::Installing(_)));
                self.work = None;
                return true;
            }
        };
        self.state = match event {
            UpdateEvent::Installed(installed) => {
                self.installed = Some(installed);
                UpdateState::Idle
            }
            UpdateEvent::Checked(Ok(Some(offer))) => UpdateState::Available(offer),
            UpdateEvent::Checked(Ok(None)) => UpdateState::Current,
            UpdateEvent::Installing(phase) => {
                self.state = UpdateState::Installing(phase);
                return true;
            }
            UpdateEvent::Staged(Ok(tag)) => {
                if let Some(installed) = &mut self.installed {
                    installed.pending = Some(tag);
                }
                UpdateState::Idle
            }
            UpdateEvent::Checked(Err(message)) | UpdateEvent::Staged(Err(message)) => {
                eprintln!("Gizlet OTA: {message}");
                UpdateState::Failed(matches!(self.state, UpdateState::Installing(_)))
            }
        };
        self.work = None;
        true
    }
}

/// What the Services page changes.
#[derive(Clone, Debug)]
enum ServiceOp {
    Refresh,
    Run(String, bool),
    Boot(String, bool),
}

struct ServicesDone {
    failed: Option<String>,
    list: Result<Vec<Service>, String>,
}

/// The installed services, read and switched on a
/// worker thread, since systemd can take seconds to start or stop one.
pub struct ServicesUi {
    pub list: Option<Result<Vec<Service>, String>>,
    work: Option<Receiver<ServicesDone>>,
    /// The switch turned, and to what, until the list after it arrives.
    pub switching: Option<(ItemId, bool)>,
    /// When the list was last asked for.
    asked: f64,
    /// `GIZLET_FAKE_DEVICES=1`: services simulated in memory.
    simulated: bool,
}

/// How often the list is read again while Services is open.
const SERVICES_REFRESH: f64 = 4.0;

impl ServicesUi {
    fn new() -> ServicesUi {
        let simulated = std::env::var("GIZLET_FAKE_DEVICES").is_ok_and(|v| v == "1");
        ServicesUi { list: None, work: None, switching: None, asked: f64::NEG_INFINITY, simulated }
    }

    fn busy(&self) -> bool {
        self.work.is_some()
    }

    fn send(&mut self, op: ServiceOp, now: f64) {
        if self.busy() {
            return;
        }
        self.asked = now;
        let (send, work) = mpsc::channel();
        if self.simulated {
            let mut list = match &self.list {
                Some(Ok(list)) => list.clone(),
                _ => simulated_services(),
            };
            for service in list.iter_mut() {
                match &op {
                    ServiceOp::Run(id, on) if id == service.id() => {
                        service.state = if *on { services::State::Running } else { services::State::Stopped };
                    }
                    ServiceOp::Boot(id, on) if id == service.id() => service.boot = *on,
                    _ => {}
                }
            }
            let _ = send.send(ServicesDone { failed: None, list: Ok(list) });
        } else {
            std::thread::spawn(move || {
                let library = Library::open();
                let failed = match &op {
                    ServiceOp::Refresh => None,
                    ServiceOp::Run(id, true) => library.start_service(id).err(),
                    ServiceOp::Run(id, false) => library.stop_service(id).err(),
                    ServiceOp::Boot(id, on) => library.set_service_boot(id, *on).err(),
                };
                let _ = send.send(ServicesDone { failed, list: library.services() });
            });
        }
        self.work = Some(work);
    }

    /// The finished work, if any.
    fn poll(&mut self) -> Option<ServicesDone> {
        let done = match self.work.as_ref()?.try_recv() {
            Ok(done) => done,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => ServicesDone { failed: None, list: Err("the services worker stopped".into()) },
        };
        self.work = None;
        self.switching = None;
        self.list = Some(done.list.clone());
        Some(done)
    }
}

/// Two services for working on the page with `GIZLET_FAKE_DEVICES=1`.
fn simulated_services() -> Vec<Service> {
    let service = |id: &str, name: &str, zh: &str, state, boot| {
        let manifest = serde_json::json!({
            "schema": 2, "kind": "service", "id": id, "name": name, "names": { "zh-CN": zh }, "version": "1.0.0",
            "target": apps::device_target(), "tier": "lite", "entry": "bin/service",
        });
        let manifest = apps::Manifest::parse(&manifest.to_string()).expect("a valid manifest");
        let package = apps::App { dir: PathBuf::from("/opt/gizlet/services").join(id), manifest, installed: std::time::SystemTime::UNIX_EPOCH };
        Service { package, boot, state, pid: None }
    };
    vec![
        service("io.gizlet.tailscale", "Tailscale", "Tailscale", services::State::Running, true),
        service("dev.example.clock-sync", "Clock Sync", "时钟同步", services::State::Failed("exit-code".into()), false),
    ]
}

/// Where the focus is, to hear it move.
#[derive(Clone, Debug, PartialEq)]
struct Place {
    depth: usize,
    home: (HomeRow, usize, usize),
    settings: (usize, usize, bool),
    dialog: Option<usize>,
    /// The keyboard's key and layout, and how much is typed.
    keyboard: Option<(usize, usize, bool, bool, usize)>,
}

pub struct App {
    pub settings: Settings,
    settings_path: PathBuf,
    pub power: Power,
    pub version: &'static str,
    pages: Vec<Page>,
    pub home_row: HomeRow,
    pub home_button: usize,
    /// Installed apps, as the home shelf shows them, and the focused one.
    pub apps: Vec<apps::App>,
    pub home_tile: usize,
    library: Library,
    apps_stamp: Option<std::time::SystemTime>,
    /// Set when the launcher should quit, as when it hands the screen to an app.
    leave: bool,
    pub focus: SettingsFocus,
    pub snapshot: Option<Snapshot>,
    system: System,
    updates: Option<Updates>,
    pub services: ServicesUi,
    pub wifi: WifiUi,
    pub ssh_switching: Option<bool>,
    /// The time zone being set, until the device reports it.
    pub zone_switching: Option<String>,
    pub dialog: Option<Dialog>,
    pub keyboard: Option<Keyboard>,
    pub toasts: Vec<Toast>,
    pub test_hold: f32,
    pub time: f64,
    pub gamepads: Vec<String>,
    pub can_quit: bool,
    /// Counts the device reports received, so the screen is redrawn after one.
    pub revision: u64,
    /// Sounds to play, taken once a frame by [`App::take_sounds`].
    sounds: Vec<Sfx>,
    /// The brightness and volume requests sent, to recognize a snapshot taken
    /// before the latest one reached the device.
    brightness_sent: u64,
    volume_sent: u64,
}

impl App {
    pub fn new(data_dir: PathBuf, version: &'static str) -> App {
        let settings_path = data_dir.join("settings.ini");
        let library = Library::open();
        let apps_stamp = library.stamp();
        let apps = library.scan();
        App {
            settings: Settings::load(&settings_path),
            settings_path,
            power: Power::load(Power::path()),
            version,
            system: System::start(data_dir),
            updates: None,
            services: ServicesUi::new(),
            pages: vec![Page::Home],
            // The most recently played app has focus; without apps, Settings.
            home_row: if apps.is_empty() { HomeRow::Buttons } else { HomeRow::Tiles },
            home_button: HomeButton::ALL.iter().position(|button| *button == HomeButton::Settings).unwrap(),
            apps,
            home_tile: 0,
            library,
            apps_stamp,
            leave: false,
            focus: SettingsFocus::default(),
            snapshot: None,
            wifi: WifiUi::default(),
            ssh_switching: None,
            zone_switching: None,
            dialog: None,
            keyboard: None,
            toasts: Vec::new(),
            test_hold: 0.0,
            time: 0.0,
            gamepads: Vec::new(),
            can_quit: true,
            revision: 0,
            sounds: Vec::new(),
            brightness_sent: 0,
            volume_sent: 0,
        }
    }

    pub fn lang(&self) -> Language {
        self.settings.language.resolve()
    }

    pub fn load_update_status(&mut self) {
        self.updates = Updates::new();
    }

    pub fn installing_update(&self) -> bool {
        self.updates.as_ref().is_some_and(|updates| matches!(updates.state, UpdateState::Installing(_)))
    }

    pub fn t(&self, t: T) -> &'static str {
        t.get(self.lang())
    }

    pub fn page(&self) -> Page {
        *self.pages.last().unwrap_or(&Page::Home)
    }

    pub fn depth(&self) -> usize {
        self.pages.len()
    }

    pub fn text_input_active(&self) -> bool {
        self.keyboard.is_some()
    }

    /// Something moves on screen without input: a menu, the keyboard, a
    /// toast, a spinner while the device works, or the button test's live sticks.
    pub fn animating(&self) -> bool {
        self.dialog.is_some()
            || self.keyboard.is_some()
            || !self.toasts.is_empty()
            || self.page() == Page::ButtonTest
            || (self.page() == Page::Services && (self.services.busy() || self.services.list.is_none()))
            || (self.page() == Page::Settings
                && self.focused_category() == Category::System
                && self.updates.as_ref().is_some_and(Updates::has_spinner))
            || self.wifi.scanning
            || self.wifi.connecting.is_some()
            || self.wifi.switching.is_some()
            || self.ssh_switching.is_some()
            || self.zone_switching.is_some()
    }

    pub fn toast(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), age: 0.0 });
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
    }

    /// The sounds to play now.
    pub fn take_sounds(&mut self) -> Vec<Sfx> {
        std::mem::take(&mut self.sounds)
    }

    /// How loud sounds play: the simulated volume, as nothing else applies
    /// it, or in full where the system's volume does.
    pub fn sound_gain(&self) -> f32 {
        match self.snapshot.as_ref() {
            Some(s) if s.simulated => *s.volume.as_ref().unwrap_or(&1.0),
            _ => 1.0,
        }
    }

    /// Queues an interface sound, when they are turned on.
    fn ui_sound(&mut self, sfx: Sfx) {
        if self.settings.ui_sounds && !self.sounds.iter().any(|&s| s != Sfx::Volume) {
            self.sounds.push(sfx);
        }
    }

    fn place(&self) -> Place {
        Place {
            depth: self.pages.len(),
            home: (self.home_row, self.home_button, self.home_tile),
            settings: (self.focus.category, self.focus.item, self.focus.in_content),
            dialog: self.dialog.as_ref().map(|d| d.focus),
            keyboard: self.keyboard.as_ref().map(|k| (k.row, k.col, k.shift, k.symbols, k.text.chars().count())),
        }
    }

    /// The sound for the focus moving from `before` this frame: opening a
    /// page, menu or keyboard confirms; closing one goes back, or confirms
    /// when A chose something in it; any other move ticks.
    fn hear(&mut self, before: &Place, input: &Input) {
        let after = self.place();
        if after == *before {
            return;
        }
        let opened = after.depth > before.depth
            || (after.dialog.is_some() && before.dialog.is_none())
            || (after.keyboard.is_some() && before.keyboard.is_none());
        let closed = after.depth < before.depth
            || (after.dialog.is_none() && before.dialog.is_some())
            || (after.keyboard.is_none() && before.keyboard.is_some());
        let chose = input.pressed(Action::A) || input.pressed(Action::Start) || input.text.contains(&TextEvent::Submit);
        let sfx = if opened || (closed && chose) {
            Sfx::Confirm
        } else if closed || input.pressed(Action::B) {
            Sfx::Back
        } else {
            Sfx::Move
        };
        self.ui_sound(sfx);
    }

    /// Reads the installed apps again when one was installed or removed,
    /// keeping focus on the same app.
    pub fn refresh_apps(&mut self) {
        let stamp = self.library.stamp();
        if stamp == self.apps_stamp {
            return;
        }
        self.apps_stamp = stamp;
        let focused = self.apps.get(self.home_tile).map(|a| a.id().to_string());
        self.apps = self.library.scan();
        // The shelf changes without input: draw it, and let it settle.
        self.revision += 1;
        self.home_tile = focused.and_then(|id| self.apps.iter().position(|a| a.id() == id)).unwrap_or(0);
        if self.apps.is_empty() && self.home_row == HomeRow::Tiles {
            self.home_row = HomeRow::Buttons;
        }
    }

    /// Starts the app on the shelf at `i`. On Gizlet OS the launcher quits and
    /// its start script runs the app, then the launcher again; on a computer
    /// the app opens beside it.
    fn launch(&mut self, i: usize) {
        if self.installing_update() {
            self.toast(self.t(T::OtaWait));
            return;
        }
        let Some(app) = self.apps.get(i) else { return };
        let id = app.id().to_string();
        let name = app.manifest.name_for(self.lang_code()).to_string();
        let result = if cfg!(feature = "gizlet-os") {
            apps::request_launch(&id).map(|()| self.leave = true)
        } else {
            // The desktop executable's run command supervises the app. A
            // thread waits for that process so it leaves no zombie.
            std::env::current_exe().and_then(|exe| std::process::Command::new(exe).args(["run", &id]).spawn()).map(|mut child| {
                std::thread::spawn(move || child.wait());
            })
        };
        match result {
            Ok(()) => self.toast(T::Starting.fmt(self.lang(), &[&name])),
            Err(e) => self.toast(T::Failed.fmt(self.lang(), &[&e.to_string()])),
        }
    }

    /// The language as apps get it in `GIZLET_LANG`.
    pub fn lang_code(&self) -> &'static str {
        self.settings.language.app_code()
    }

    fn save(&mut self) {
        if let Err(e) = self.settings.save(&self.settings_path) {
            let msg = T::Failed.fmt(self.lang(), &[&e.to_string()]);
            self.toast(msg);
        }
    }

    pub fn focused_category(&self) -> Category {
        Category::ALL[self.focus.category.min(Category::ALL.len() - 1)]
    }

    /// Advances by `dt` seconds with this frame's input. Returns false to quit.
    pub fn update(&mut self, input: &Input, dt: f32) -> bool {
        self.time += dt as f64;
        for t in &mut self.toasts {
            t.age += dt;
        }
        self.toasts.retain(|t| t.age < TOAST_SECONDS);
        let events = self.system.poll();
        if self.updates.as_ref().is_some_and(|updates| updates.work.is_some()) {
            let focused = if self.focused_category() == Category::System { self.focused_item() } else { None };
            if self.updates.as_mut().is_some_and(Updates::poll) {
                self.revision += 1;
                self.refocus(focused);
            }
        }
        self.update_services();
        if !events.is_empty() {
            let focused = self.focused_item();
            for event in events {
                self.on_event(event);
                self.revision += 1;
            }
            self.refocus(focused);
        }
        self.auto_scan();

        let before = self.place();
        let keep = self.handle_input(input, dt);
        self.hear(&before, input);
        keep
    }

    /// Reads the services while their page is open, and takes in
    /// what the worker finished.
    fn update_services(&mut self) {
        let open = self.page() == Page::Services;
        if open && !self.services.busy() && self.time - self.services.asked >= SERVICES_REFRESH {
            self.services.send(ServiceOp::Refresh, self.time);
        }
        if !self.services.busy() {
            return;
        }
        let focused = if open { self.service_items().get(self.focus.item).map(|item| item.id.clone()) } else { None };
        if let Some(done) = self.services.poll() {
            if let Some(e) = done.failed {
                self.toast(T::Failed.fmt(self.lang(), &[&e]));
            }
            self.revision += 1;
            if open {
                let items = self.service_items();
                self.focus.item = focused
                    .filter(|id| *id != ItemId::None)
                    .and_then(|id| items.iter().position(|row| row.id == id))
                    .or_else(|| items.iter().position(|item| item.id != ItemId::None))
                    .unwrap_or(0);
            }
        }
    }

    /// This frame's input on the open page, menu or keyboard. False to quit.
    fn handle_input(&mut self, input: &Input, dt: f32) -> bool {
        if self.keyboard.is_some() {
            self.keyboard_input(input);
            return true;
        }
        if self.dialog.is_some() {
            return self.dialog_input(input);
        }
        if self.page() != Page::ButtonTest {
            for (a, step) in [(Action::VolumeUp, 0.05), (Action::VolumeDown, -0.05)] {
                if input.pressed(a) {
                    self.nudge_volume(step);
                }
            }
        }
        match self.page() {
            Page::Home => {
                self.home_input(input);
                if self.leave {
                    return false;
                }
            }
            Page::Settings => self.settings_input(input),
            Page::Services => self.services_input(input),
            Page::ButtonTest => {
                if input.held(Action::B) {
                    self.test_hold += dt;
                    if self.test_hold >= TEST_EXIT_SECONDS {
                        self.test_hold = 0.0;
                        self.pages.pop();
                    }
                } else {
                    self.test_hold = 0.0;
                }
            }
        }
        true
    }

    fn on_event(&mut self, event: Event) {
        let l = self.lang();
        match event {
            Event::Snapshot(mut s) => {
                // A report read before the latest change reached the device
                // would move the slider back; the next one reports it.
                if let Some(old) = &self.snapshot {
                    if s.brightness_sets < self.brightness_sent {
                        s.brightness = old.brightness.clone();
                    }
                    if s.volume_sets < self.volume_sent {
                        s.volume = old.volume.clone();
                    }
                }
                if let Ok(w) = &s.wifi {
                    if self.wifi.switching == Some(w.on) {
                        self.wifi.switching = None;
                    }
                    if !w.on || !w.adapter {
                        self.wifi.networks = None;
                        self.wifi.scanning = false;
                    }
                }
                if let Ok(ssh) = &s.ssh {
                    if self.ssh_switching == Some(ssh.on) {
                        self.ssh_switching = None;
                    }
                }
                if let Ok(c) = &s.clock {
                    if self.zone_switching.as_ref() == Some(&c.zone) {
                        self.zone_switching = None;
                    }
                }
                self.snapshot = Some(*s);
            }
            Event::Volume => self.sounds.push(Sfx::Volume),
            Event::WifiNetworks(result) => {
                self.wifi.scanning = false;
                match result {
                    Ok(mut nets) => {
                        nets.sort_by(|a, b| b.signal.cmp(&a.signal));
                        self.wifi.networks = Some(nets);
                    }
                    Err(e) => {
                        self.wifi.networks.get_or_insert_with(Vec::new);
                        self.toast(T::Failed.fmt(l, &[&e]));
                    }
                }
            }
            Event::WifiConnect { ssid, result } => {
                self.wifi.connecting = None;
                match result {
                    Ok(()) => {
                        self.toast(T::WifiConnectedTo.fmt(l, &[&ssid]));
                        self.scan();
                    }
                    Err(e) => self.toast(T::WifiConnectFailed.fmt(l, &[&ssid, &e])),
                }
            }
            Event::Failed(e) => {
                self.wifi.switching = None;
                self.ssh_switching = None;
                self.zone_switching = None;
                self.toast(T::Failed.fmt(l, &[&e]));
            }
        }
    }

    fn wifi_ready(&self) -> bool {
        matches!(self.snapshot.as_ref().map(|s| &s.wifi), Some(Ok(w)) if w.on && w.adapter)
    }

    /// Looks for networks when the Network page shows Wi-Fi without a list.
    fn auto_scan(&mut self) {
        let showing = self.page() == Page::Settings && self.focused_category() == Category::Network;
        if showing && self.wifi_ready() && self.wifi.networks.is_none() && !self.wifi.scanning {
            self.scan();
        }
    }

    fn scan(&mut self) {
        if self.wifi_ready() && !self.wifi.scanning {
            self.wifi.scanning = true;
            self.system.send(Request::WifiScan);
        }
    }

    fn nudge_volume(&mut self, step: f32) {
        if let Some(Ok(v)) = self.snapshot.as_ref().map(|s| &s.volume) {
            let v = *v;
            self.slide(&ItemId::Volume, v + step);
        }
    }

    pub fn push(&mut self, page: Page) {
        self.pages.push(page);
        if matches!(page, Page::Settings | Page::Services) {
            self.focus = SettingsFocus::default();
        }
        self.test_hold = 0.0;
    }

    fn power_dialog(&mut self) {
        if self.installing_update() {
            self.toast(self.t(T::OtaWait));
            return;
        }
        let mut buttons: Vec<(String, DialogAction)> = Vec::new();
        if let Some(s) = &self.snapshot {
            for a in &s.power {
                let label = match a {
                    PowerAction::Sleep => T::Sleep,
                    PowerAction::Reboot => T::Reboot,
                    PowerAction::PowerOff => T::PowerOff,
                };
                buttons.push((self.t(label).into(), DialogAction::Power(*a)));
            }
        }
        if self.can_quit {
            buttons.push((self.t(T::QuitLauncher).into(), DialogAction::Quit));
        }
        self.dialog = Some(Dialog { title: self.t(T::PowerTitle).into(), buttons, focus: 0, opened: self.time });
    }

    fn home_input(&mut self, input: &Input) {
        let n = HomeButton::ALL.len();
        let tiles = self.apps.len();
        for &a in &input.pressed {
            match (a, self.home_row) {
                (Action::Down, HomeRow::Tiles) => self.home_row = HomeRow::Buttons,
                (Action::Up, HomeRow::Buttons) => self.home_row = HomeRow::Tiles,
                (Action::Left, HomeRow::Tiles) if tiles > 0 => self.home_tile = self.home_tile.saturating_sub(1),
                (Action::Right, HomeRow::Tiles) if tiles > 0 => self.home_tile = (self.home_tile + 1).min(tiles - 1),
                (Action::Left, HomeRow::Buttons) => self.home_button = (self.home_button + n - 1) % n,
                (Action::Right, HomeRow::Buttons) => self.home_button = (self.home_button + 1) % n,
                (Action::A, HomeRow::Tiles) if tiles > 0 => {
                    self.launch(self.home_tile);
                    if self.leave {
                        return;
                    }
                }
                (Action::A, HomeRow::Tiles) => self.toast(self.t(T::LibrarySoon)),
                (Action::A, HomeRow::Buttons) => match HomeButton::ALL[self.home_button] {
                    HomeButton::Library if tiles > 0 => self.home_row = HomeRow::Tiles,
                    HomeButton::Library => self.toast(self.t(T::LibrarySoon)),
                    HomeButton::Services => {
                        self.services.asked = f64::NEG_INFINITY;
                        self.push(Page::Services);
                        self.focus.item = self.service_items().iter().position(|item| item.id != ItemId::None).unwrap_or(0);
                        return;
                    }
                    HomeButton::Store => self.toast(self.t(T::StoreSoon)),
                    HomeButton::ButtonTest => self.push(Page::ButtonTest),
                    HomeButton::Settings => self.push(Page::Settings),
                    HomeButton::Power => self.power_dialog(),
                },
                (Action::Start, _) => self.push(Page::Settings),
                _ => {}
            }
        }
    }

    fn services_input(&mut self, input: &Input) {
        if input.pressed.is_empty() {
            return;
        }
        let items = self.service_items();
        for &action in &input.pressed {
            match action {
                Action::B => {
                    self.pages.pop();
                    return;
                }
                Action::Up => {
                    if let Some(index) = (0..self.focus.item).rev().find(|&index| items[index].id != ItemId::None) {
                        self.focus.item = index;
                    }
                }
                Action::Down => {
                    if let Some(index) = (self.focus.item + 1..items.len()).find(|&index| items[index].id != ItemId::None) {
                        self.focus.item = index;
                    }
                }
                Action::A => {
                    if let Some(item) = items.get(self.focus.item).filter(|item| item.id != ItemId::None) {
                        self.activate(item);
                    }
                }
                _ => {}
            }
        }
    }

    fn settings_input(&mut self, input: &Input) {
        for &a in &input.pressed {
            let items = self.settings_items(self.focused_category());
            if !self.focus.in_content {
                let n = Category::ALL.len();
                match a {
                    Action::Up => self.focus.category = (self.focus.category + n - 1) % n,
                    Action::Down => self.focus.category = (self.focus.category + 1) % n,
                    Action::A | Action::Right => {
                        if let Some(i) = items.iter().position(Item::focusable) {
                            self.focus.item = i;
                            self.focus.in_content = true;
                        }
                    }
                    Action::B => {
                        self.pages.pop();
                        return;
                    }
                    _ => {}
                }
                continue;
            }
            let sel = self.focus.item.min(items.len().saturating_sub(1));
            let item = items.get(sel).cloned();
            match a {
                Action::Up | Action::Down => {
                    let focusable: Vec<usize> = (0..items.len()).filter(|&i| items[i].focusable()).collect();
                    if let Some(pos) = focusable.iter().position(|&i| i >= sel) {
                        let next = if a == Action::Up { pos.checked_sub(1) } else { Some(pos + usize::from(focusable[pos] == sel)) };
                        if let Some(&i) = next.and_then(|p| focusable.get(p)) {
                            self.focus.item = i;
                        }
                    }
                }
                Action::Left | Action::Right => {
                    let step = if a == Action::Left { -1.0 } else { 1.0 };
                    match item.as_ref().map(|i| (&i.id, &i.value)) {
                        Some((id, ItemValue::Slider(v))) => self.slide(id, *v + step * 0.05),
                        Some((ItemId::Theme, _)) => {
                            self.toggle_theme();
                            self.ui_sound(Sfx::Move);
                        }
                        Some((ItemId::SleepAfter, _)) => {
                            self.cycle_sleep_after(false, step as i32);
                            self.ui_sound(Sfx::Move);
                        }
                        Some((ItemId::SleepAfterCharging, _)) => {
                            self.cycle_sleep_after(true, step as i32);
                            self.ui_sound(Sfx::Move);
                        }
                        _ if a == Action::Left => self.focus.in_content = false,
                        _ => {}
                    }
                }
                Action::A => {
                    if let Some(item) = item {
                        self.activate(&item);
                    }
                }
                Action::Y if self.focused_category() == Category::Network => {
                    self.wifi.networks = None;
                    self.scan();
                }
                Action::X => {
                    if let Some(ItemId::WifiNetwork(ssid)) = item.as_ref().map(|i| &i.id) {
                        let saved = self.wifi.networks.iter().flatten().any(|n| &n.ssid == ssid && n.known);
                        if saved {
                            let connected = matches!(&item.as_ref().map(|i| &i.value), Some(ItemValue::Network { status: Some(s), .. }) if s == self.t(T::WifiConnected));
                            self.network_menu(&ssid.clone(), connected);
                        }
                    }
                }
                Action::B => self.focus.in_content = false,
                _ => {}
            }
        }
        // The list can shrink under the focus, as when a scan finishes.
        if self.focus.in_content {
            let items = self.settings_items(self.focused_category());
            if !items.get(self.focus.item).is_some_and(Item::focusable) {
                let sel = self.focus.item;
                let best = (0..items.len()).filter(|&i| items[i].focusable()).min_by_key(|&i| i.abs_diff(sel));
                match best {
                    Some(i) => self.focus.item = i,
                    None => self.focus.in_content = false,
                }
            }
        }
    }

    /// The settings row in focus, to find again after the list changes.
    fn focused_item(&self) -> Option<Item> {
        if self.page() != Page::Settings || !self.focus.in_content {
            return None;
        }
        self.settings_items(self.focused_category()).get(self.focus.item).cloned()
    }

    /// Keeps the focus on the same row when a device report adds or removes
    /// rows above it, as the battery's Time Left does when the charger is
    /// plugged in: by its id, or by its label for information rows.
    fn refocus(&mut self, before: Option<Item>) {
        let Some(before) = before else { return };
        let items = self.settings_items(self.focused_category());
        if items.get(self.focus.item).is_some_and(|i| i.id == before.id && i.label == before.label) {
            return;
        }
        let found = if before.id == ItemId::None {
            items.iter().position(|i| i.id == ItemId::None && i.label == before.label && i.focusable())
        } else {
            items.iter().position(|i| i.id == before.id)
        };
        if let Some(i) = found {
            self.focus.item = i;
        }
    }

    /// Sets the brightness or volume to `v`, on a 5% grid. The page shows it at
    /// once; reports read before the device has it keep that value.
    fn slide(&mut self, id: &ItemId, v: f32) {
        let v = (v * 20.0).round() / 20.0;
        let v = v.clamp(0.0, 1.0);
        let Some(s) = self.snapshot.as_mut() else { return };
        match id {
            ItemId::Brightness => {
                let moved = s.brightness.as_ref().map_or(true, |old| (old - v).abs() > 0.001);
                s.brightness = Ok(v);
                self.brightness_sent += 1;
                self.system.send(Request::SetBrightness(v));
                if moved {
                    self.ui_sound(Sfx::Move);
                }
            }
            // The device answers with Event::Volume, which plays the volume's sound at the new level.
            ItemId::Volume => {
                s.volume = Ok(v);
                self.volume_sent += 1;
                self.system.send(Request::SetVolume(v));
            }
            _ => {}
        }
    }

    fn cycle_sleep_after(&mut self, charging: bool, step: i32) {
        self.power.cycle(charging, step);
        if let Err(e) = self.power.save() {
            let msg = T::Failed.fmt(self.lang(), &[&e.to_string()]);
            self.toast(msg);
        }
    }

    /// A sleep choice as shown: minutes, or never.
    pub fn sleep_after_label(&self, seconds: u32) -> String {
        match seconds {
            0 => self.t(T::Never).to_string(),
            s => T::Minutes.fmt(self.lang(), &[&(s / 60).to_string()]),
        }
    }

    fn toggle_theme(&mut self) {
        self.settings.theme = match self.settings.theme {
            Theme::Dark => Theme::Light,
            Theme::Light => Theme::Dark,
        };
        self.save();
    }

    fn activate(&mut self, item: &Item) {
        if !matches!(item.id, ItemId::Brightness | ItemId::Volume | ItemId::None) {
            self.ui_sound(Sfx::Confirm);
        }
        let current = match &item.value {
            ItemValue::Toggle(on) => *on,
            _ => false,
        };
        match &item.id {
            ItemId::WifiPower => {
                self.wifi.switching = Some(!current);
                self.system.send(Request::WifiPower(!current));
            }
            ItemId::Ssh => {
                self.ssh_switching = Some(!current);
                self.system.send(Request::Ssh(!current));
            }
            ItemId::ServiceRunning(id) | ItemId::ServiceBoot(id) if !self.services.busy() => {
                let op = match &item.id {
                    ItemId::ServiceRunning(_) => ServiceOp::Run(id.clone(), !current),
                    _ => ServiceOp::Boot(id.clone(), !current),
                };
                self.services.switching = Some((item.id.clone(), !current));
                self.services.send(op, self.time);
            }
            ItemId::SshPassword => {
                let current = match self.snapshot.as_ref().map(|s| &s.ssh) {
                    Some(Ok(s)) => s.password.clone().unwrap_or_default(),
                    _ => String::new(),
                };
                self.keyboard = Some(Keyboard::ssh_password(self.t(T::SshPasswordTitle).into(), &current));
            }
            ItemId::Hostname => {
                let current = self.snapshot.as_ref().map(|s| s.hostname.clone()).unwrap_or_default();
                self.keyboard = Some(Keyboard::hostname(self.t(T::HostnameTitle).into(), &current));
            }
            ItemId::CheckUpdate => {
                if let Some(updates) = &mut self.updates { updates.check(); }
            }
            ItemId::InstallUpdate => {
                self.dialog = Some(Dialog {
                    title: self.t(T::OtaConfirm).into(),
                    buttons: vec![
                        (self.t(T::Close).into(), DialogAction::Close),
                        (self.t(T::OtaDownload).into(), DialogAction::InstallUpdate),
                    ],
                    focus: 0,
                    opened: self.time,
                });
            }
            ItemId::RestartUpdate => {
                self.dialog = Some(Dialog {
                    title: self.t(T::OtaRestart).into(),
                    buttons: vec![
                        (self.t(T::Close).into(), DialogAction::Close),
                        (self.t(T::Reboot).into(), DialogAction::Power(PowerAction::Reboot)),
                    ],
                    focus: 0,
                    opened: self.time,
                });
            }
            ItemId::WifiScan => {
                self.wifi.networks = None;
                self.scan();
            }
            ItemId::WifiDisconnect => self.system.send(Request::WifiDisconnect),
            ItemId::WifiNetwork(ssid) => self.choose_network(ssid),
            ItemId::Theme => self.toggle_theme(),
            ItemId::SleepAfter => self.cycle_sleep_after(false, 1),
            ItemId::SleepAfterCharging => self.cycle_sleep_after(true, 1),
            ItemId::BatteryPercent => {
                self.settings.battery_percent = !self.settings.battery_percent;
                self.save();
            }
            ItemId::UiSounds => {
                self.settings.ui_sounds = !self.settings.ui_sounds;
                self.save();
                // Heard when they come on.
                self.sounds.retain(|&s| s == Sfx::Volume);
                self.ui_sound(Sfx::Confirm);
            }
            ItemId::ButtonTest => self.push(Page::ButtonTest),
            ItemId::SwapAb => {
                self.settings.swap_ab = !self.settings.swap_ab;
                self.save();
            }
            ItemId::Language(lang) => {
                self.settings.language = *lang;
                self.save();
            }
            ItemId::Clock24 => {
                self.settings.clock_24h = !self.settings.clock_24h;
                self.save();
            }
            ItemId::TimeZone(zone) => {
                if item.value != ItemValue::Radio(true) {
                    self.zone_switching = Some(zone.clone());
                    self.system.send(Request::TimeZone(zone.clone()));
                }
            }
            // One service switch at a time: the next waits for the worker.
            ItemId::ServiceRunning(_) | ItemId::ServiceBoot(_) => {}
            ItemId::Brightness | ItemId::Volume | ItemId::None => {}
        }
    }

    fn choose_network(&mut self, ssid: &str) {
        if self.wifi.connecting.is_some() {
            return;
        }
        let connected = matches!(self.snapshot.as_ref().map(|s| &s.wifi), Some(Ok(w)) if w.link.ssid.as_deref() == Some(ssid));
        let network = self.wifi.networks.as_ref().and_then(|n| n.iter().find(|n| n.ssid == ssid)).cloned();
        if connected {
            self.network_menu(ssid, true);
        } else if network.is_some_and(|n| n.known || !n.secured) {
            // Saved and open networks need no passphrase.
            self.connect(ssid, None);
        } else {
            let title = T::WifiPassphrase.fmt(self.lang(), &[ssid]);
            self.keyboard = Some(Keyboard::passphrase(title, ssid.into()));
        }
    }

    /// Disconnect (when connected), Forget and Close for a saved network.
    fn network_menu(&mut self, ssid: &str, connected: bool) {
        let mut buttons = Vec::new();
        if connected {
            buttons.push((self.t(T::WifiDisconnect).into(), DialogAction::WifiDisconnect));
        }
        buttons.push((self.t(T::WifiForget).into(), DialogAction::WifiForget(ssid.into())));
        buttons.push((self.t(T::Close).into(), DialogAction::Close));
        self.dialog = Some(Dialog { title: ssid.into(), buttons, focus: 0, opened: self.time });
    }

    fn connect(&mut self, ssid: &str, passphrase: Option<String>) {
        self.wifi.connecting = Some(ssid.into());
        self.system.send(Request::WifiConnect { ssid: ssid.into(), passphrase });
    }

    fn dialog_input(&mut self, input: &Input) -> bool {
        let Some(d) = self.dialog.as_mut() else { return true };
        let n = d.buttons.len().max(1);
        for &a in &input.pressed {
            match a {
                Action::Up | Action::Left => d.focus = (d.focus + n - 1) % n,
                Action::Down | Action::Right => d.focus = (d.focus + 1) % n,
                Action::B => {
                    self.dialog = None;
                    return true;
                }
                Action::A => {
                    let action = d.buttons.get(d.focus).map(|b| b.1.clone()).unwrap_or(DialogAction::Close);
                    self.dialog = None;
                    match action {
                        DialogAction::Quit => return false,
                        DialogAction::Power(p) => self.system.send(Request::Power(p)),
                        DialogAction::WifiDisconnect => self.system.send(Request::WifiDisconnect),
                        DialogAction::WifiForget(ssid) => {
                            self.system.send(Request::WifiForget(ssid));
                            self.wifi.networks = None;
                        }
                        DialogAction::InstallUpdate => {
                            if let Some(updates) = &mut self.updates { updates.install(); }
                        }
                        DialogAction::Close => {}
                    }
                    return true;
                }
                _ => {}
            }
        }
        true
    }

    fn keyboard_input(&mut self, input: &Input) {
        let Some(kb) = self.keyboard.as_mut() else { return };
        let mut submit = None;
        let mut cancel = false;
        for e in &input.text {
            match e {
                TextEvent::Char(c) => kb.type_char(*c),
                TextEvent::Backspace => kb.backspace(),
                TextEvent::Submit => submit = Some(kb.text.clone()),
                TextEvent::Cancel => cancel = true,
            }
        }
        for &a in &input.pressed {
            match a {
                Action::Up => kb.move_by(0, -1),
                Action::Down => kb.move_by(0, 1),
                Action::Left => kb.move_by(-1, 0),
                Action::Right => kb.move_by(1, 0),
                // B deletes; on an empty field it closes the keyboard.
                Action::B if kb.text.is_empty() => cancel = true,
                Action::B => kb.backspace(),
                Action::Y => kb.type_char(' '),
                Action::X => kb.masked = !kb.masked,
                Action::Start => submit = Some(kb.text.clone()),
                _ => {}
            }
        }
        if let keyboard::Outcome::Submit(text) = kb.a_input(input.pressed(Action::A), input.held(Action::A), self.time) {
            submit = Some(text);
        }
        if cancel {
            self.keyboard = None;
        } else if let Some(text) = submit {
            let purpose = kb.purpose.clone();
            // A name that cannot be used keeps the keyboard open to fix it.
            let name = text.trim().to_ascii_lowercase();
            if purpose == keyboard::Purpose::Hostname && !name.is_empty() && !valid_hostname(&name) {
                self.toast(self.t(T::HostnameInvalid));
                return;
            }
            self.keyboard = None;
            match purpose {
                // The same name is not stored, so a default one keeps following the adapter.
                keyboard::Purpose::Hostname if self.snapshot.as_ref().is_some_and(|s| s.hostname == name) => {}
                keyboard::Purpose::Hostname => {
                    let saved = if name.is_empty() { self.t(T::HostnameDefault).into() } else { T::HostnameSaved.fmt(self.lang(), &[&name]) };
                    self.system.send(Request::Hostname(name));
                    self.toast(saved);
                }
                keyboard::Purpose::WifiPassphrase(ssid) => self.connect(&ssid, Some(text)),
                keyboard::Purpose::SshPassword if text.is_empty() => {}
                keyboard::Purpose::SshPassword => {
                    self.system.send(Request::SshPassword(text));
                    self.toast(self.t(T::SshPasswordSaved));
                }
            }
        }
    }

    fn missing(&self, m: &Missing, no_hw: T, no_hw_body: Option<T>) -> Item {
        let l = self.lang();
        match m {
            Missing::NoHardware => Item::notice(no_hw.get(l), no_hw_body.map(|b| b.get(l)).unwrap_or("")),
            Missing::NoService(name) => Item::notice(T::MissingService.fmt(l, &[name]), T::MissingServiceBody.get(l)),
            Missing::ManagedBySystem => Item::notice(T::ManagedBySystem.get(l), T::ManagedBySystemBody.get(l)),
            Missing::Error(e) => Item::notice(T::DeviceError.fmt(l, &[e]), ""),
        }
    }

    /// The rows of a settings category, from the current state.
    pub fn settings_items(&self, cat: Category) -> Vec<Item> {
        let l = self.lang();
        let t = |k: T| k.get(l).to_string();
        let mut items = Vec::new();
        let Some(snap) = self.snapshot.as_ref() else {
            items.push(Item::new(ItemId::None, t(T::WifiScanning), ItemValue::Busy));
            return items;
        };
        match cat {
            Category::Network => {
                match &snap.wifi {
                    Err(m) => items.push(self.missing(m, T::WifiNoHardware, Some(T::WifiNoHardwareBody))),
                    Ok(w) => self.wifi_items(w, &mut items),
                }
            }
            Category::Ssh => {
                match &snap.ssh {
                    Err(m) => items.push(self.missing(m, T::Ssh, None)),
                    Ok(s) => {
                        let on = self.ssh_switching.unwrap_or(s.on);
                        items.push(Item::new(ItemId::Ssh, t(T::Ssh), ItemValue::Toggle(on)));
                        let status = if s.running { t(T::SshRunning) } else { t(T::SshStopped) };
                        items.push(Item::info(t(T::BatteryState), status));
                        let password = s.password.clone().unwrap_or_else(|| t(T::SshPasswordUnset));
                        items.push(Item::new(ItemId::SshPassword, t(T::SshPassword), ItemValue::Info(password)));
                        if s.running {
                            match &snap.ip {
                                // NAME.local where the system announces it, and the address.
                                Some(ip) if snap.can_rename => {
                                    items.push(Item::info(t(T::SshLogin), format!("ssh root@{}.local", snap.hostname)));
                                    items.push(Item::info("", format!("ssh root@{ip}")));
                                }
                                Some(ip) => items.push(Item::info(t(T::SshLogin), format!("ssh root@{ip}"))),
                                None => items.push(Item::info(t(T::SshLogin), t(T::SshNeedsNetwork))),
                            }
                            let keys = match s.keys {
                                Some(0) | None => t(T::SshNoKeys),
                                Some(n) => T::SshKeyCount.fmt(l, &[&n.to_string()]),
                            };
                            items.push(Item::info(t(T::SshKeys), keys));
                        }
                    }
                }
            }
            Category::Display => {
                match &snap.brightness {
                    Ok(v) => items.push(Item::new(ItemId::Brightness, t(T::Brightness), ItemValue::Slider(*v))),
                    Err(m) => items.push(self.missing(m, T::NoBacklight, None)),
                }
                let theme = match self.settings.theme {
                    Theme::Dark => t(T::ThemeDark),
                    Theme::Light => t(T::ThemeLight),
                };
                items.push(Item::new(ItemId::Theme, t(T::ThemeLabel), ItemValue::Choice(theme)));
            }
            Category::Sound => {
                match &snap.volume {
                    Ok(v) => items.push(Item::new(ItemId::Volume, t(T::Volume), ItemValue::Slider(*v))),
                    Err(m) => items.push(self.missing(m, T::NoAudio, Some(T::NoAudioBody))),
                }
                items.push(Item::new(ItemId::UiSounds, t(T::UiSounds), ItemValue::Toggle(self.settings.ui_sounds)));
            }
            Category::Battery => {
                match &snap.battery {
                    Ok(b) => {
                        items.push(Item::new(
                            ItemId::None,
                            t(T::BatteryLevel),
                            ItemValue::Bar { value: format!("{}%", b.percent), fraction: b.percent as f32 / 100.0, warn: b.percent <= 15 },
                        ));
                        let state = match b.charge {
                            crate::system::Charge::Charging => t(T::Charging),
                            crate::system::Charge::Full => t(T::Full),
                            crate::system::Charge::Discharging => t(T::Discharging),
                            crate::system::Charge::Plugged => t(T::Plugged),
                        };
                        items.push(Item::info(t(T::BatteryState), state));
                        if let Some(mw) = b.power_mw {
                            let label = if b.charge == crate::system::Charge::Charging { T::ChargePower } else { T::PowerDraw };
                            items.push(Item::info(t(label), format!("{:.2} W", mw as f32 / 1000.0)));
                        }
                        if let Some(m) = b.minutes_left {
                            let left = if m < 60 {
                                T::Minutes.fmt(l, &[&m.to_string()])
                            } else {
                                T::HoursMinutes.fmt(l, &[&(m / 60).to_string(), &(m % 60).to_string()])
                            };
                            items.push(Item::info(t(T::TimeLeft), left));
                        }
                    }
                    Err(m) => items.push(self.missing(m, T::NoBattery, Some(T::NoBatteryBody))),
                }
                // Only where the device can sleep.
                if snap.power.contains(&PowerAction::Sleep) {
                    let battery = self.sleep_after_label(self.power.sleep_after);
                    let charging = self.sleep_after_label(self.power.sleep_after_charging);
                    items.push(Item::new(ItemId::SleepAfter, t(T::SleepAfter), ItemValue::Choice(battery)));
                    items.push(Item::new(ItemId::SleepAfterCharging, t(T::SleepAfterCharging), ItemValue::Choice(charging)));
                }
                items.push(Item::new(ItemId::BatteryPercent, t(T::BatteryPercent), ItemValue::Toggle(self.settings.battery_percent)));
                // The clocks and CPU use behind the draw, refreshed with the snapshot.
                let load = snap.load;
                if load.cpu_mhz.is_some() || load.gpu_mhz.is_some() {
                    items.push(Item::section(t(T::Performance)));
                }
                if let Some(mhz) = load.cpu_mhz {
                    let value = match load.cpu_busy {
                        Some(busy) => T::CpuBusy.fmt(l, &[&mhz.to_string(), &busy.to_string()]),
                        None => format!("{mhz} MHz"),
                    };
                    items.push(Item::info(t(T::CpuClock), value));
                }
                if let Some(mhz) = load.gpu_mhz {
                    items.push(Item::info(t(T::GpuClock), format!("{mhz} MHz")));
                }
            }
            Category::Controls => {
                items.push(Item::new(ItemId::ButtonTest, t(T::ButtonTestOpen), ItemValue::Button));
                items.push(Item::new(ItemId::SwapAb, t(T::SwapAb), ItemValue::Toggle(self.settings.swap_ab)));
                items.push(Item::section(t(T::Gamepads)));
                if self.gamepads.is_empty() {
                    items.push(Item::notice(t(T::NoGamepads), ""));
                }
                for (i, name) in self.gamepads.iter().enumerate() {
                    items.push(Item::info(format!("#{}", i + 1), name.clone()));
                }
            }
            Category::Language => {
                for lang in Language::ALL {
                    let label = match lang {
                        Language::Auto => t(T::LangAuto),
                        Language::ZhCn => "简体中文".into(),
                        Language::En => "English".into(),
                    };
                    items.push(Item::new(ItemId::Language(lang), label, ItemValue::Radio(self.settings.language == lang)));
                }
            }
            Category::Time => {
                let now = clock::now();
                items.push(Item::info(t(T::TimeNow), format!("{}  {}", now.date(l == Language::ZhCn), now.time(self.settings.clock_24h))));
                if let Ok(c) = &snap.clock {
                    items.push(Item::info(t(T::NetworkTime), if c.synced { t(T::TimeSynced) } else { t(T::TimeNotSynced) }));
                }
                items.push(Item::new(ItemId::Clock24, t(T::Clock24), ItemValue::Toggle(self.settings.clock_24h)));
                items.push(Item::section(t(T::TimeZone)));
                match &snap.clock {
                    Ok(c) => {
                        let current = self.zone_switching.as_ref().unwrap_or(&c.zone);
                        let listed = Zone::find(current);
                        // A zone set another way, such as over ssh, is shown by its name.
                        if listed.is_none() {
                            items.push(Item::new(ItemId::TimeZone(current.clone()), current.clone(), ItemValue::Radio(true)));
                        }
                        for z in ZONES {
                            let selected = listed.is_some_and(|f| f.name == z.name);
                            items.push(Item::new(ItemId::TimeZone(z.name.into()), z.label(l), ItemValue::Radio(selected)));
                        }
                    }
                    Err(m) => items.push(self.missing(m, T::TimeZone, None)),
                }
            }
            Category::System => {
                self.update_items(&mut items);
                items.push(Item::section(t(T::About)));
                if self.updates.is_none() {
                    items.push(Item::info(t(T::Version), self.version));
                }
                let unknown = || t(T::Unknown);
                items.push(Item::info(t(T::Device), snap.info.model.clone().unwrap_or_else(unknown)));
                items.push(Item::info(t(T::OperatingSystem), snap.info.os.clone()));
                items.push(Item::info(t(T::Kernel), snap.info.kernel.clone()));
                items.push(Item::info(t(T::Arch), snap.info.arch.clone()));
                items.push(Item::info(t(T::Cpus), snap.info.cpus.to_string()));
                items.push(Item::info(t(T::Memory), snap.info.memory_bytes.map(human_size).unwrap_or_else(unknown)));
                if snap.can_rename {
                    items.push(Item::new(ItemId::Hostname, t(T::Hostname), ItemValue::Info(snap.hostname.clone())));
                    items.push(Item::info(t(T::LocalAddress), format!("{}.local", snap.hostname)));
                } else {
                    items.push(Item::info(t(T::Hostname), snap.hostname.clone()));
                }
                items.push(Item::info(t(T::IpAddress), snap.ip.clone().unwrap_or_else(|| t(T::WifiNotConnected))));
                if let Some(st) = snap.storage {
                    let used = st.total.saturating_sub(st.free);
                    items.push(Item::new(
                        ItemId::None,
                        t(T::Storage),
                        ItemValue::Bar {
                            value: T::StorageUsed.fmt(l, &[&human_size(used), &human_size(st.total)]),
                            fraction: if st.total > 0 { used as f32 / st.total as f32 } else { 0.0 },
                            warn: st.total > 0 && used * 10 > st.total * 9,
                        },
                    ));
                }
                if snap.simulated {
                    items.push(Item::info(t(T::Simulated), t(T::SimulatedOn)));
                }
            }
        }
        items
    }

    /// Each installed service: whether it runs, whether it starts at boot, and its state.
    pub fn service_items(&self) -> Vec<Item> {
        let l = self.lang();
        let t = |k: T| k.get(l).to_string();
        let list = match &self.services.list {
            None => return vec![Item::new(ItemId::None, t(T::ServicesReading), ItemValue::Busy)],
            Some(Err(e)) => return vec![Item::notice(t(T::ServicesUnavailable), e.clone())],
            Some(Ok(list)) => list,
        };
        let mut items = Vec::new();
        let switched = |id: &ItemId, now: bool| match &self.services.switching {
            Some((switching, to)) if switching == id => *to,
            _ => now,
        };
        for service in list {
            let m = &service.package.manifest;
            items.push(Item::section(format!("{} · {}", m.name_for(self.lang_code()), m.version)));
            let running = ItemId::ServiceRunning(m.id.clone());
            let on = switched(&running, service.state.on());
            items.push(Item::new(running, t(T::ServiceRunning), ItemValue::Toggle(on)));
            let boot = ItemId::ServiceBoot(m.id.clone());
            let at_boot = switched(&boot, service.boot);
            items.push(Item::new(boot, t(T::ServiceAtBoot), ItemValue::Toggle(at_boot)));
            let state = match &service.state {
                _ if matches!(self.services.switching, Some((ItemId::ServiceRunning(ref id), _)) if *id == m.id) => {
                    if on { t(T::ServiceStarting) } else { t(T::ServiceStopping) }
                }
                services::State::Running => t(T::SshRunning),
                services::State::Starting => t(T::ServiceStarting),
                services::State::Stopping => t(T::ServiceStopping),
                services::State::Stopped => t(T::ServiceStopped),
                services::State::Restarting(why) => T::ServiceRestarting.fmt(l, &[why]),
                services::State::Failed(why) => T::ServiceFailed.fmt(l, &[why]),
            };
            items.push(Item::info(t(T::BatteryState), state));
        }
        items
    }

    fn update_items(&self, items: &mut Vec<Item>) {
        let Some(updates) = &self.updates else { return };
        let lang = self.lang();
        let t = |key: T| key.get(lang);
        items.push(Item::section(t(T::OtaTitle)));
        let Some(installed) = &updates.installed else {
            items.push(Item::new(ItemId::None, t(T::OtaReading), ItemValue::Busy));
            return;
        };
        let runtime = installed.runtime.as_ref().map(|hash| format!("1 · {}", &hash[..12]));
        items.push(Item::info(t(T::OtaRuntime), runtime.as_deref().unwrap_or(t(T::Unknown))));
        items.push(Item::info(t(T::OtaLauncher), &installed.launcher));
        if let Some(tag) = &installed.pending {
            items.push(Item::notice(T::OtaStaged.fmt(lang, &[tag]), t(T::OtaRestart)));
            items.push(Item::new(ItemId::RestartUpdate, t(T::Reboot), ItemValue::Button));
            return;
        }
        match &updates.state {
            UpdateState::Idle => {
                if let Some(tag) = &installed.failed {
                    items.push(Item::notice(T::OtaRolledBack.fmt(lang, &[tag]), t(T::OtaRunningPrevious)));
                }
            }
            UpdateState::Checking => {
                items.push(Item::new(ItemId::None, t(T::OtaChecking), ItemValue::Busy));
                return;
            }
            UpdateState::Current => items.push(Item::notice(t(T::OtaCurrent), "")),
            UpdateState::Available(offer) => {
                let component = match (offer.runtime_changed, offer.launcher_changed) {
                    (true, true) => T::OtaBoth,
                    (true, false) => T::OtaRuntime,
                    _ => T::OtaLauncher,
                };
                let size = human_size(offer.bytes());
                items.push(Item::notice(T::OtaAvailable.fmt(lang, &[&offer.tag]), format!("{} · {}", t(component), size)));
                items.push(Item::new(ItemId::InstallUpdate, t(T::OtaDownload), ItemValue::Button));
            }
            UpdateState::Installing(phase) => {
                match phase {
                    ota::Progress::Runtime(percent) | ota::Progress::Launcher(percent) => {
                        let label = if matches!(phase, ota::Progress::Runtime(_)) {
                            T::OtaPreparingRuntime
                        } else {
                            T::OtaDownloadingLauncher
                        };
                        items.push(Item::new(ItemId::None, t(label), ItemValue::Bar {
                            value: format!("{percent}%"),
                            fraction: f32::from(*percent) / 100.0,
                            warn: false,
                        }));
                    }
                    ota::Progress::Verify => {
                        items.push(Item::new(ItemId::None, t(T::OtaVerifying), ItemValue::Busy));
                    }
                }
                return;
            }
            UpdateState::Failed(install) => items.push(Item::notice(
                t(if *install { T::OtaInstallFailed } else { T::OtaCheckFailed }),
                t(T::OtaRetry),
            )),
        }
        items.push(Item::new(ItemId::CheckUpdate, t(T::OtaCheck), ItemValue::Button));
    }

    fn wifi_items(&self, w: &crate::system::WifiState, items: &mut Vec<Item>) {
        let l = self.lang();
        let t = |k: T| k.get(l).to_string();
        let on = self.wifi.switching.unwrap_or(w.on);
        items.push(Item::new(ItemId::WifiPower, t(T::Wifi), ItemValue::Toggle(on)));
        if self.wifi.switching == Some(true) && !w.on {
            items.push(Item::new(ItemId::None, t(T::WifiStarting), ItemValue::Busy));
            return;
        }
        if !w.on {
            items.push(Item::notice(t(T::WifiOffBody), ""));
            return;
        }
        if !w.adapter {
            items.push(Item::notice(t(T::WifiNoHardware), t(T::WifiNoHardwareBody)));
            return;
        }
        items.push(Item::section(t(T::WifiCurrent)));
        match &w.link.ssid {
            Some(ssid) => {
                items.push(Item::info(t(T::Wifi), ssid.clone()));
                items.push(Item::new(ItemId::WifiDisconnect, t(T::WifiDisconnect), ItemValue::Button));
            }
            None => items.push(Item::info(t(T::Wifi), t(T::WifiNotConnected))),
        }
        items.push(Item::section(t(T::WifiAvailable)));
        if self.wifi.scanning && self.wifi.networks.is_none() {
            items.push(Item::new(ItemId::None, t(T::WifiScanning), ItemValue::Busy));
        }
        if let Some(nets) = &self.wifi.networks {
            if nets.is_empty() {
                items.push(Item::notice(t(T::WifiNone), ""));
            }
            for n in nets {
                let status = if self.wifi.connecting.as_deref() == Some(n.ssid.as_str()) {
                    Some(t(T::WifiConnecting))
                } else if w.link.ssid.as_deref() == Some(n.ssid.as_str()) {
                    Some(t(T::WifiConnected))
                } else if n.known {
                    Some(t(T::WifiSaved))
                } else {
                    None
                };
                items.push(Item::new(
                    ItemId::WifiNetwork(n.ssid.clone()),
                    n.ssid.clone(),
                    ItemValue::Network { signal: n.signal, secured: n.secured, status },
                ));
            }
        }
        if self.wifi.scanning && self.wifi.networks.is_some() {
            items.push(Item::new(ItemId::None, t(T::WifiScanning), ItemValue::Busy));
        } else if !self.wifi.scanning {
            items.push(Item::new(ItemId::WifiScan, t(T::WifiScan), ItemValue::Button));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed() -> ota::Installed {
        ota::Installed { launcher: "1.2.3".into(), runtime: None, pending: None, failed: None }
    }

    #[test]
    fn ota_spinner_keeps_only_system_settings_animating() {
        let mut app = App::new(std::env::temp_dir(), "0.1.0");
        let (send, work) = mpsc::channel();
        app.updates = Some(Updates { installed: None, work: Some(work), state: UpdateState::Idle });
        assert!(!app.animating());

        app.push(Page::Settings);
        assert!(!app.animating());
        app.focus.category = Category::ALL.iter().position(|&category| category == Category::System).unwrap();
        assert!(app.animating());

        let updates = app.updates.as_mut().unwrap();
        updates.installed = Some(installed());
        updates.state = UpdateState::Checking;
        assert!(app.animating());
        app.updates.as_mut().unwrap().state = UpdateState::Installing(ota::Progress::Runtime(50));
        assert!(!app.animating());
        app.updates.as_mut().unwrap().state = UpdateState::Installing(ota::Progress::Verify);
        assert!(app.animating());

        send.send(UpdateEvent::Staged(Ok("v1.2.4".into()))).unwrap();
        assert!(app.updates.as_mut().unwrap().poll());
        assert!(!app.animating());
    }

    #[test]
    fn system_settings_show_one_launcher_version() {
        let mut app = App::new(std::env::temp_dir(), "0.1.0");
        app.snapshot = Some(crate::system::probe(std::env::temp_dir()));
        let version_rows = |app: &App| {
            app.settings_items(Category::System).into_iter()
                .filter(|item| item.label == app.t(T::Version) || item.label == app.t(T::OtaLauncher))
                .collect::<Vec<_>>()
        };
        assert_eq!(version_rows(&app), vec![Item::info(app.t(T::Version), "0.1.0")]);

        let (_send, work) = mpsc::channel();
        app.updates = Some(Updates { installed: None, work: Some(work), state: UpdateState::Idle });
        assert!(version_rows(&app).is_empty());
        app.updates.as_mut().unwrap().installed = Some(installed());
        assert_eq!(version_rows(&app), vec![Item::info(app.t(T::OtaLauncher), "1.2.3")]);
    }
}
