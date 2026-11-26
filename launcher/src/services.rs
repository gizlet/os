//! Installed services: `.gzl` packages whose manifest (schema 2) has kind
//! `service`, background programs with no screen of their own.
//!
//! `gizlet install` places a service beside the apps and registers it with
//! the system's service manager, systemd on Gizlet OS, as
//! `gizlet-service-<id>.service`. The base system is read-only, so the unit
//! lives in `/run/systemd/system` and is written again on every boot by
//! `gizlet service boot` (`gizlet-services.service`), which then starts the
//! services set to start at boot. The unit runs `gizlet service exec <id>`,
//! which prepares the service's folders and environment and replaces itself
//! with the package's entry. A service is a system unit, never a child of the
//! launcher, so it keeps running while the launcher quits for an app or
//! restarts.
//!
//! | Path | Holds |
//! | --- | --- |
//! | `<prefix>/services/<id>/` | The package |
//! | `<prefix>/services/.boot-<id>` | Present while it starts at boot |
//! | `<prefix>/service-data/<id>/` | Its state and configuration (`GIZLET_STATE_DIR`), kept across updates |
//! | `/run/gizlet-services/<id>/` | Its sockets and other runtime files (`GIZLET_RUNTIME_DIR`), removed when it stops |
//!
//! Uninstalling stops it and every process it started, unregisters the unit
//! and deletes all of these.

use crate::apps::{self, App, Kind, Library, Manifest};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The units of Gizlet services are named `gizlet-service-<id>.service`.
pub const UNIT_PREFIX: &str = "gizlet-service-";
/// Where the units are written: systemd's runtime unit folder, emptied at every boot.
const UNIT_DIR: &str = "/run/systemd/system";
/// Where each service's runtime folder is created: systemd's `RuntimeDirectory=`, under `/run`.
pub const RUN_DIR: &str = "/run/gizlet-services";
/// After starting, how long a service must keep running to count as started.
const PROBATION: Duration = Duration::from_secs(3);

pub fn unit_name(id: &str) -> String {
    format!("{UNIT_PREFIX}{id}.service")
}

/// What systemd reports about a unit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnitState {
    /// `loaded`, or `not-found` for a unit that is not registered.
    pub load: String,
    /// `active`, `activating`, `deactivating`, `reloading`, `inactive` or `failed`.
    pub active: String,
    pub sub: String,
    /// How it last ended: `success`, `exit-code`, `signal`, `timeout`, ...
    pub result: String,
    pub pid: u32,
    /// Automatic restarts since it was last started.
    pub restarts: u32,
    /// Its control group while any of its processes remain.
    pub cgroup: String,
}

impl UnitState {
    /// Some process of it may be running.
    pub fn running(&self) -> bool {
        matches!(self.active.as_str(), "active" | "activating" | "deactivating" | "reloading") || !self.cgroup.is_empty()
    }

    /// Why a service that was just started is not running. A service runs
    /// until it is stopped, so exiting at all, even successfully, is a failed start.
    fn start_problem(&self) -> Option<String> {
        let result = if self.result.is_empty() { "unknown" } else { &self.result };
        if self.active == "failed" {
            Some(format!("it failed ({result})"))
        } else if self.sub == "auto-restart" || self.restarts > 0 {
            Some(format!("it exited and is being restarted ({result})"))
        } else if self.active == "inactive" {
            Some(format!("it exited ({result})"))
        } else {
            None
        }
    }
}

/// The service manager the units are registered with: [`Systemd`], or a
/// stand-in in tests.
pub trait Manager: Send + Sync {
    /// Whether services can run here, or why not.
    fn available(&self) -> Result<(), String>;
    /// Writes the unit, or removes it when `text` is `None`. True if that changed anything.
    fn write_unit(&self, unit: &str, text: Option<&str>) -> Result<bool, String>;
    /// The Gizlet service units written.
    fn units(&self) -> Vec<String>;
    /// A unit as written, if it is.
    fn read_unit(&self, unit: &str) -> Option<String>;
    /// Makes the manager read the units again.
    fn reload(&self) -> Result<(), String>;
    /// Starts the unit; without `wait`, only queues the start.
    fn start(&self, unit: &str, wait: bool) -> Result<(), String>;
    /// Stops the unit and every process in it.
    fn stop(&self, unit: &str) -> Result<(), String>;
    /// Forgets that the unit failed.
    fn reset_failed(&self, unit: &str);
    fn states(&self, units: &[String]) -> Result<Vec<UnitState>, String>;
    fn probation(&self) -> Duration {
        PROBATION
    }
}

/// systemd, through `systemctl`.
pub struct Systemd {
    units: PathBuf,
}

impl Systemd {
    pub fn new() -> Systemd {
        Systemd { units: PathBuf::from(UNIT_DIR) }
    }

    fn systemctl(&self, args: &[&str]) -> Result<String, String> {
        let out = Command::new("systemctl")
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("cannot run systemctl: {e}"))?;
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
        }
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        Err(if err.is_empty() { format!("systemctl {} failed ({})", args.join(" "), out.status) } else { err.to_string() })
    }
}

impl Default for Systemd {
    fn default() -> Self {
        Systemd::new()
    }
}

impl Manager for Systemd {
    fn available(&self) -> Result<(), String> {
        // sd_booted(3): systemd is the init system.
        if cfg!(target_os = "linux") && Path::new("/run/systemd/system").is_dir() {
            Ok(())
        } else {
            Err("services run under systemd, as on Gizlet OS, and this system does not run it".into())
        }
    }

    fn write_unit(&self, unit: &str, text: Option<&str>) -> Result<bool, String> {
        let path = self.units.join(unit);
        let Some(text) = text else {
            return match fs::remove_file(&path) {
                Ok(()) => Ok(true),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
                Err(e) => Err(format!("{}: {e}", path.display())),
            };
        };
        if fs::read_to_string(&path).is_ok_and(|old| old == text) {
            return Ok(false);
        }
        let tmp = self.units.join(format!(".{unit}.new"));
        fs::write(&tmp, text).and_then(|()| fs::rename(&tmp, &path)).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("{}: {e}", path.display())
        })?;
        Ok(true)
    }

    fn units(&self) -> Vec<String> {
        let mut units: Vec<String> = fs::read_dir(&self.units)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(UNIT_PREFIX) && n.ends_with(".service"))
            .collect();
        units.sort();
        units
    }

    fn read_unit(&self, unit: &str) -> Option<String> {
        fs::read_to_string(self.units.join(unit)).ok()
    }

    fn reload(&self) -> Result<(), String> {
        self.systemctl(&["daemon-reload"]).map(drop)
    }

    fn start(&self, unit: &str, wait: bool) -> Result<(), String> {
        let args: &[&str] = if wait { &["start", unit] } else { &["start", "--no-block", unit] };
        self.systemctl(args).map(drop)
    }

    fn stop(&self, unit: &str) -> Result<(), String> {
        self.systemctl(&["stop", unit]).map(drop)
    }

    fn reset_failed(&self, unit: &str) {
        let _ = self.systemctl(&["reset-failed", unit]);
    }

    fn states(&self, units: &[String]) -> Result<Vec<UnitState>, String> {
        if units.is_empty() {
            return Ok(Vec::new());
        }
        let mut args = vec!["show", "--property=Id,LoadState,ActiveState,SubState,Result,MainPID,NRestarts,ControlGroup", "--"];
        args.extend(units.iter().map(String::as_str));
        let out = self.systemctl(&args)?;
        let mut found = std::collections::BTreeMap::new();
        for block in out.split("\n\n") {
            let mut id = String::new();
            let mut state = UnitState::default();
            for line in block.lines() {
                let Some((key, value)) = line.split_once('=') else { continue };
                let value = value.to_string();
                match key {
                    "Id" => id = value,
                    "LoadState" => state.load = value,
                    "ActiveState" => state.active = value,
                    "SubState" => state.sub = value,
                    "Result" => state.result = value,
                    "MainPID" => state.pid = value.parse().unwrap_or(0),
                    "NRestarts" => state.restarts = value.parse().unwrap_or(0),
                    "ControlGroup" => state.cgroup = value,
                    _ => {}
                }
            }
            if !id.is_empty() {
                found.insert(id, state);
            }
        }
        Ok(units
            .iter()
            .map(|u| found.remove(u).unwrap_or_else(|| UnitState { load: "not-found".into(), active: "inactive".into(), ..UnitState::default() }))
            .collect())
    }
}

/// Whether a service runs, as the launcher and `gizlet service` show it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Running,
    Starting,
    Stopping,
    Stopped,
    /// It exited and systemd starts it again shortly; with systemd's reason.
    Restarting(String),
    /// It failed and is not restarted; with systemd's reason.
    Failed(String),
}

impl State {
    fn of(unit: &UnitState) -> State {
        match (unit.active.as_str(), unit.sub.as_str()) {
            ("active" | "reloading", _) => State::Running,
            ("activating", "auto-restart") => State::Restarting(unit.result.clone()),
            ("activating", _) => State::Starting,
            ("deactivating", _) => State::Stopping,
            ("failed", _) => State::Failed(unit.result.clone()),
            _ => State::Stopped,
        }
    }

    /// Started or about to be, so stopping is what a switch does: what a Running switch shows.
    pub fn on(&self) -> bool {
        matches!(self, State::Running | State::Starting | State::Restarting(_))
    }

    pub fn word(&self) -> String {
        match self {
            State::Running => "running".into(),
            State::Starting => "starting".into(),
            State::Stopping => "stopping".into(),
            State::Stopped => "stopped".into(),
            State::Restarting(why) => format!("restarting ({why})"),
            State::Failed(why) => format!("failed ({why})"),
        }
    }
}

/// An installed service and how it runs.
#[derive(Clone, Debug)]
pub struct Service {
    pub package: App,
    /// It starts at boot.
    pub boot: bool,
    pub state: State,
    pub pid: Option<u32>,
}

impl Service {
    pub fn id(&self) -> &str {
        self.package.id()
    }
}

/// The service manager of this system.
pub fn system_manager() -> Box<dyn Manager> {
    Box::new(Systemd::new())
}

/// Characters a path may have to be written into a unit file as it is.
fn unit_safe(path: &Path) -> Option<&str> {
    let text = path.to_str()?;
    (text.starts_with('/') && text.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._-+".contains(&b))).then_some(text)
}

/// The command a unit runs: `/usr/bin/gizlet` on Gizlet OS, which runs the
/// CLI of the Launcher release selected at boot; elsewhere this program,
/// the CLI or the launcher, which both take `service exec ID`.
fn cli_path() -> Result<PathBuf, String> {
    if cfg!(feature = "gizlet-os") {
        return Ok(PathBuf::from("/usr/bin/gizlet"));
    }
    std::env::current_exe().map_err(|e| format!("cannot find the gizlet command: {e}"))
}

impl Library {
    pub fn services_dir(&self) -> PathBuf {
        self.prefix().join("services")
    }

    /// The service's state and configuration, kept across updates and deleted on uninstall.
    pub fn service_data(&self, id: &str) -> PathBuf {
        self.prefix().join("service-data").join(id)
    }

    /// The service's runtime folder, which systemd creates as it starts and removes once it stops.
    pub fn service_runtime(&self, id: &str) -> PathBuf {
        self.run_dir().join(id)
    }

    fn boot_flag(&self, id: &str) -> PathBuf {
        self.services_dir().join(format!(".boot-{id}"))
    }

    /// The package of a service being uninstalled: renamed here once it is
    /// stopped and unregistered, it marks the uninstall until it is finished.
    pub(crate) fn uninstalling(&self, id: &str) -> PathBuf {
        self.services_dir().join(format!(".uninstall-{id}"))
    }

    /// Present while an update replaces a service that was running: in
    /// `/run`, so it says so only until the next boot.
    fn restart_flag(&self, id: &str) -> PathBuf {
        self.run_dir().join(format!(".restart-{id}"))
    }

    /// The unit that runs the service.
    pub fn unit_text(&self, id: &str) -> Result<String, String> {
        let cli = cli_path()?;
        let cli = unit_safe(&cli).ok_or_else(|| format!("{} cannot be written into a unit", cli.display()))?;
        let prefix = self.prefix();
        let prefix = unit_safe(prefix).ok_or_else(|| format!("{} cannot be written into a unit", prefix.display()))?;
        let runtime = RUN_DIR.trim_start_matches("/run/");
        // On Gizlet OS a service needs the data partition, and stops before it at shutdown.
        let data = if cfg!(feature = "gizlet-os") {
            "Requires=gizlet-data.service\nConditionPathExists=/run/gizlet-data.ready\n"
        } else {
            ""
        };
        Ok(format!(
            "# Written by gizlet for the service {id}; `gizlet uninstall {id}` removes it.\n\
             [Unit]\n\
             Description=Gizlet service {id}\n\
             After=gizlet-data.service network.target\n\
             {data}\
             StartLimitIntervalSec=60\n\
             StartLimitBurst=5\n\
             \n\
             [Service]\n\
             Type=exec\n\
             ExecStart={cli} service exec {id}\n\
             Environment=GIZLET_PREFIX={prefix}\n\
             Restart=on-failure\n\
             RestartSec=2\n\
             TimeoutStopSec=15\n\
             KillMode=control-group\n\
             RuntimeDirectory={runtime}/{id}\n\
             RuntimeDirectoryMode=0700\n\
             PrivateTmp=yes\n\
             StandardInput=null\n\
             StandardOutput=journal\n\
             StandardError=journal\n\
             SyslogIdentifier={id}\n"
        ))
    }

    /// The prefix a written unit runs its service from.
    fn unit_prefix(text: &str) -> Option<&str> {
        text.lines().find_map(|line| line.strip_prefix("Environment=GIZLET_PREFIX="))
    }

    /// Whether this library may change `unit`: it is not written, or written for this prefix.
    fn owns(&self, unit: &str) -> Result<(), String> {
        let Some(text) = self.manager().read_unit(unit) else { return Ok(()) };
        match Self::unit_prefix(&text) {
            Some(prefix) if Path::new(prefix) == self.prefix() => Ok(()),
            other => Err(format!("{unit} runs a service installed in {}, not in {}; it is left alone", other.unwrap_or("another place"), self.prefix().display())),
        }
    }

    /// The service `id` as installed, without the lock.
    pub(crate) fn find_service(&self, id: &str) -> Result<App, String> {
        if !apps::is_valid_id(id) {
            return Err(format!("{id:?} is not a service ID"));
        }
        let dir = self.services_dir().join(id);
        if !dir.is_dir() {
            return Err(if self.apps_dir().join(id).is_dir() {
                format!("{id} is an app, not a service")
            } else {
                format!("{id} is not installed")
            });
        }
        let package = self.load(&dir)?;
        if package.manifest.kind() != Kind::Service {
            return Err(format!("{} holds {id}, which is not a service", dir.display()));
        }
        Ok(package)
    }

    /// Installed service packages, by ID.
    fn service_packages(&self) -> Vec<App> {
        let mut found: Vec<App> = fs::read_dir(self.services_dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .filter_map(|e| self.find_service(&e.file_name().to_string_lossy()).ok())
            .collect();
        found.sort_by(|a, b| a.id().cmp(b.id()));
        found
    }

    fn unit_state(&self, id: &str) -> Result<UnitState, String> {
        let mut states = self.manager().states(&[unit_name(id)])?;
        Ok(states.pop().unwrap_or_default())
    }

    fn status(&self, package: App, unit: &UnitState) -> Service {
        let boot = self.boot_flag(package.id()).exists();
        Service { boot, state: State::of(unit), pid: (unit.pid != 0).then_some(unit.pid), package }
    }

    /// The installed services and how each runs. Like the shelf's scan, it
    /// first finishes an interrupted update or uninstall if nothing else holds
    /// the library.
    pub fn services(&self) -> Result<Vec<Service>, String> {
        self.recover_if_free();
        let packages = self.service_packages();
        if packages.is_empty() {
            return Ok(Vec::new());
        }
        self.manager().available()?;
        let units: Vec<String> = packages.iter().map(|p| unit_name(p.id())).collect();
        let states = self.manager().states(&units)?;
        Ok(packages.into_iter().zip(states.iter()).map(|(p, s)| self.status(p, s)).collect())
    }

    pub fn service(&self, id: &str) -> Result<Service, String> {
        self.recover_if_free();
        let package = self.find_service(id)?;
        self.manager().available()?;
        let unit = self.unit_state(id)?;
        Ok(self.status(package, &unit))
    }

    /// Writes the service's unit and has the manager read it if it changed.
    fn register(&self, id: &str) -> Result<(), String> {
        self.owns(&unit_name(id))?;
        let text = self.unit_text(id)?;
        if self.manager().write_unit(&unit_name(id), Some(&text)).map_err(|e| format!("cannot register {id}: {e}"))? {
            self.manager().reload().map_err(|e| format!("cannot register {id}: {e}"))?;
        }
        Ok(())
    }

    /// Starts the service and waits until it has kept running for a moment.
    /// A start asked for is never refused for earlier failures: their count
    /// limits automatic restarts only.
    fn start_checked(&self, id: &str) -> Result<(), String> {
        let unit = unit_name(id);
        self.manager().reset_failed(&unit);
        self.manager().start(&unit, true)?;
        let deadline = Instant::now() + self.manager().probation();
        loop {
            if let Some(problem) = self.unit_state(id)?.start_problem() {
                return Err(problem);
            }
            if Instant::now() >= deadline {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Stops the service and every process it started, then holds its
    /// running lock: nothing of it runs until the lock is dropped.
    fn stop_verified(&self, id: &str) -> Result<File, String> {
        let unit = unit_name(id);
        if let Err(e) = self.manager().available() {
            // Without a manager nothing was started for it, except perhaps by hand.
            return self.not_running_settled(id).map_err(|_| format!("{id} is running ({e})"));
        }
        self.owns(&unit)?;
        let before = self.unit_state(id)?;
        if before.load != "not-found" || before.running() {
            self.manager().stop(&unit)?;
        }
        let after = self.unit_state(id)?;
        if after.running() {
            let left = if after.cgroup.is_empty() { String::new() } else { format!(", processes remain in {}", after.cgroup) };
            return Err(format!("{unit} is still {} ({}){left}", after.active, after.sub));
        }
        self.not_running_settled(id).map_err(|_| format!("{id} is still running outside {unit}, as `gizlet service exec {id}`"))
    }

    /// The service's running lock, held exclusively. A process another thread
    /// is starting holds a copy of every open descriptor until it runs its
    /// program, so a lock that seems held is tried again for a moment.
    fn not_running_settled(&self, id: &str) -> Result<File, String> {
        let root = self.services_dir();
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match self.not_running(&root, id) {
                Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
                result => return result,
            }
        }
    }

    /// Installs or updates the service unpacked in `staging`, with the
    /// library locked. A new service starts and starts at boot. An update
    /// stops the running version first, keeps the state and the boot choice,
    /// and starts the new version if the old one was running; if it fails to
    /// start, the previous version comes back.
    pub(crate) fn install_service(&self, manifest: &Manifest, staging: &Path) -> Result<App, String> {
        let id = manifest.id.as_str();
        if self.apps_dir().join(id).exists() {
            return Err(format!("{id} is installed as an app; uninstall it first"));
        }
        if self.uninstalling(id).exists() {
            return Err(format!("an earlier uninstall of {id} did not finish; run `gizlet uninstall {id}` first"));
        }
        self.manager().available().map_err(|e| format!("cannot install the service {id}: {e}"))?;
        let root = self.services_dir();
        fs::create_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
        let dest = root.join(id);
        let old = self.backup_in(&root, id);
        let previous = match dest.exists() {
            true => Some(self.find_service(id).map_err(|e| format!("the installed {id} is damaged ({e}); run `gizlet uninstall {id}` first"))?),
            false => None,
        };
        let was_running = previous.is_some() && self.unit_state(id)?.running();
        // If this update is cut short, recovery in this boot starts the previous version again.
        let _restart = if was_running { Some(Flag::write(self.restart_flag(id))?) } else { None };
        let running = self.stop_verified(id).map_err(|e| format!("cannot stop {id} to update it: {e}; the installed version is unchanged"))?;
        let swapped = (|| {
            if previous.is_some() {
                fs::rename(&dest, &old).map_err(|e| format!("{}: {e}", dest.display()))?;
            }
            if let Err(e) = fs::rename(staging, &dest) {
                if previous.is_some() {
                    let _ = fs::rename(&old, &dest);
                }
                return Err(format!("{}: {e}", dest.display()));
            }
            Ok(())
        })();
        drop(running);
        if let Err(e) = swapped {
            if !was_running {
                return Err(e);
            }
            if !dest.is_dir() {
                // Recovery puts the installed version back and, from this flag, starts it.
                std::mem::forget(_restart);
                return Err(format!("{e}; the next `gizlet` command puts the installed version back and starts it"));
            }
            return Err(match self.start_checked(id) {
                Ok(()) => format!("{e}; the installed version is unchanged and running again"),
                Err(again) => format!("{e}; the installed version is unchanged but did not start again: {again}"),
            });
        }
        apps::sync();
        let started = self.register(id).and_then(|()| {
            if previous.is_none() {
                fs::write(self.boot_flag(id), "").map_err(|e| format!("{}: {e}", self.boot_flag(id).display()))?;
                apps::sync();
            }
            if previous.is_none() || was_running {
                self.start_checked(id)?;
            }
            Ok(())
        });
        // Committing renames the backup away in one step before deleting it,
        // so recovery never puts back a half-deleted one. An update that
        // cannot be committed is undone now, as recovery would undo it.
        let trash = root.join(format!(".remove-{id}-{}-old", std::process::id()));
        let started_ok = started.is_ok();
        let committed = started.and_then(|()| match previous {
            Some(_) => fs::rename(&old, &trash).map_err(|e| format!("{}: {e}", old.display())),
            None => Ok(()),
        });
        let Err(problem) = committed else {
            if previous.is_some() {
                apps::sync();
                let _ = fs::remove_dir_all(&trash);
            }
            return self.load(&dest);
        };
        let logs = format!("see `journalctl -u {}`", unit_name(id));
        let Some(previous) = previous else {
            return Err(match self.remove_service(id) {
                Ok(()) => format!("{id} did not start: {problem}; it was not installed ({logs})"),
                Err(e) => format!("{id} did not start: {problem} ({logs}); removing it again failed: {e}"),
            });
        };
        // Put the previous version back as it was.
        let version = &previous.manifest.version;
        let failed = if started_ok { "started, but its update cannot be confirmed" } else { "did not start" };
        let restored = self.stop_verified(id).and_then(|running| {
            let trash = root.join(format!(".remove-{id}-{}-new", std::process::id()));
            fs::rename(&dest, &trash).map_err(|e| format!("{}: {e}", dest.display()))?;
            fs::rename(&old, &dest).map_err(|e| format!("{}: {e}", old.display()))?;
            apps::sync();
            let _ = fs::remove_dir_all(&trash);
            drop(running);
            self.register(id)?;
            if was_running {
                self.start_checked(id)?;
            }
            Ok(())
        });
        if restored.is_err() && was_running {
            // Recovery finishes the rollback later, and from this flag starts the previous version.
            std::mem::forget(_restart);
        }
        Err(match restored {
            Ok(()) if was_running => format!("{id} {} {failed}: {problem} ({logs}); {version} is back and running", manifest.version),
            Ok(()) => format!("{id} {} {failed}: {problem} ({logs}); {version} is back", manifest.version),
            Err(e) => format!("{id} {} {failed}: {problem} ({logs}); restoring {version} failed: {e}", manifest.version),
        })
    }

    /// Uninstalls the service with the library locked: it no longer starts
    /// at boot, it and every process it started stop, its unit is
    /// unregistered, and its package, state and runtime folder are deleted.
    /// Each failure says what is left and what to do.
    pub(crate) fn remove_service(&self, id: &str) -> Result<(), String> {
        let root = self.services_dir();
        let unit = unit_name(id);
        let again = format!("then run `gizlet uninstall {id}` again");
        let flag = self.boot_flag(id);
        match fs::remove_file(&flag) {
            Ok(()) => apps::sync(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("cannot turn off {id} at boot: {}: {e}; nothing was removed", flag.display())),
        }
        let running = self.stop_verified(id).map_err(|e| {
            format!("cannot stop {id}: {e}. Nothing was removed, and it no longer starts at boot: restart the device, {again}")
        })?;
        if self.manager().available().is_ok() {
            let removed = self.manager().write_unit(&unit, None).map_err(|e| format!("cannot unregister {unit}: {e}; {again}"))?;
            if removed {
                self.manager().reload().map_err(|e| format!("cannot unregister {unit}: {e}; {again}"))?;
            }
            self.manager().reset_failed(&unit);
        }
        // From here it is no longer installed, and the renamed package marks
        // the uninstall until everything is deleted, even across a power loss.
        let dir = root.join(id);
        fs::rename(&dir, self.uninstalling(id)).map_err(|e| format!("{id} is stopped and unregistered, but {}: {e}; {again}", dir.display()))?;
        apps::sync();
        self.finish_uninstall(id, Some(running))
    }

    /// Deletes what an uninstall leaves once the service is stopped and
    /// unregistered: its state, runtime folder and lock, then its package.
    /// `running` is its running lock if the caller holds it.
    pub(crate) fn finish_uninstall(&self, id: &str, running: Option<File>) -> Result<(), String> {
        let again = format!("then run `gizlet uninstall {id}` again");
        let root = self.services_dir();
        let running = match running {
            Some(lock) => lock,
            None => self.not_running_settled(id).map_err(|_| format!("{id} is still running, as `gizlet service exec {id}`; stop it, {again}"))?,
        };
        let mut folders = vec![self.service_data(id)];
        // The runtime folder is shared by name; another prefix may run its own service of this ID by now.
        if self.owns(&unit_name(id)).is_ok() {
            folders.push(self.service_runtime(id));
        }
        for dir in folders {
            remove_tree(&dir).map_err(|e| format!("{id} is stopped and unregistered, but {} cannot be deleted: {e}; {again}", dir.display()))?;
        }
        let _ = fs::remove_file(self.boot_flag(id));
        drop(running);
        let _ = fs::remove_file(self.running_in(&root, id));
        let package = self.uninstalling(id);
        remove_tree(&package).map_err(|e| format!("{id} is stopped and unregistered, but {} cannot be deleted: {e}; {again}", package.display()))?;
        apps::sync();
        Ok(())
    }

    /// After a crash in the middle of an update, with the library locked: a
    /// backup is deleted only once the new version has started, so one left
    /// beside an installed version means that version was never confirmed.
    /// It is stopped and the backup goes back in place; boot then starts it
    /// as before. Until the service can be stopped, both are left as they are.
    pub(crate) fn restore_service(&self, id: &str, backup: &Path) {
        let root = self.services_dir();
        let dest = root.join(id);
        if dest.exists() {
            let Ok(running) = self.stop_verified(id) else { return };
            let trash = root.join(format!(".remove-{id}-{}", std::process::id()));
            if fs::rename(&dest, &trash).is_err() {
                return;
            }
            let _ = fs::rename(backup, &dest);
            apps::sync();
            let _ = fs::remove_dir_all(&trash);
            drop(running);
        } else if fs::rename(backup, &dest).is_ok() {
            apps::sync();
        }
        // The update was cut short in this boot: the previous version ran, so it runs again.
        let flag = self.restart_flag(id);
        if flag.exists() && dest.is_dir() && self.register(id).is_ok() {
            let _ = self.manager().start(&unit_name(id), false);
        }
        let _ = fs::remove_file(flag);
    }

    /// After a crash in the middle of an uninstall, with the library locked:
    /// deletes the boot flags, running locks and state of services that are
    /// no longer installed.
    pub(crate) fn sweep_services(&self) {
        let root = self.services_dir();
        // A backup that recovery could not yet put back still holds the service.
        let installed = |id: &str| root.join(id).exists() || self.backup_in(&root, id).exists();
        // An update cut short before its backup was made stopped a running
        // service that is still installed as it was: it runs again.
        for e in fs::read_dir(self.run_dir()).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(id) = name.strip_prefix(".restart-").filter(|id| apps::is_valid_id(id)) else { continue };
            if !root.join(id).is_dir() || self.backup_in(&root, id).exists() || self.owns(&unit_name(id)).is_err() {
                continue;
            }
            if self.register(id).is_ok() {
                let _ = self.manager().start(&unit_name(id), false);
            }
            let _ = fs::remove_file(e.path());
        }
        for e in fs::read_dir(&root).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(id) = name.strip_prefix(".boot-").filter(|id| !installed(id)) {
                let _ = fs::remove_file(self.boot_flag(id));
            } else if let Some(id) = name.strip_prefix(".running-").filter(|id| !installed(id)) {
                if let Ok(lock) = self.not_running(&root, id) {
                    let _ = fs::remove_file(e.path());
                    drop(lock);
                }
            }
        }
        // Runtime folders are left alone: systemd removes each as its service
        // stops, /run is empty after a power loss, and another prefix's
        // services may share the folder.
        for e in fs::read_dir(self.prefix().join("service-data")).into_iter().flatten().flatten() {
            let id = e.file_name().to_string_lossy().into_owned();
            if apps::is_valid_id(&id) && !installed(&id) {
                let _ = remove_tree(&e.path());
            }
        }
    }

    /// Starts the service, registering its unit first if needed.
    pub fn start_service(&self, id: &str) -> Result<Service, String> {
        let _lock = self.lock_waiting()?;
        self.recover();
        self.find_service(id)?;
        self.manager().available()?;
        self.register(id)?;
        self.start_checked(id).map_err(|e| format!("{id} did not start: {e} (see `journalctl -u {}`)", unit_name(id)))?;
        self.service(id)
    }

    /// Stops the service and every process it started.
    pub fn stop_service(&self, id: &str) -> Result<Service, String> {
        let _lock = self.lock_waiting()?;
        self.recover();
        self.find_service(id)?;
        self.manager().available()?;
        self.stop_verified(id).map_err(|e| format!("cannot stop {id}: {e}"))?;
        self.service(id)
    }

    pub fn restart_service(&self, id: &str) -> Result<Service, String> {
        self.stop_service(id)?;
        self.start_service(id)
    }

    /// Chooses whether the service starts at boot; it does not start or stop it now.
    pub fn set_service_boot(&self, id: &str, on: bool) -> Result<Service, String> {
        let _lock = self.lock_waiting()?;
        self.recover();
        self.find_service(id)?;
        let flag = self.boot_flag(id);
        let changed = if on { fs::write(&flag, "") } else { fs::remove_file(&flag).or_else(|e| if e.kind() == io::ErrorKind::NotFound { Ok(()) } else { Err(e) }) };
        changed.map_err(|e| format!("{}: {e}", flag.display()))?;
        apps::sync();
        self.service(id)
    }

    /// At boot: registers every installed service, unregisters units of
    /// services no longer installed, and starts those set to start at boot
    /// without waiting for them. Returns the services started.
    pub fn boot_services(&self) -> Result<Vec<String>, String> {
        let _lock = self.lock_waiting()?;
        self.recover();
        let packages = self.service_packages();
        let stale: Vec<String> = self
            .manager()
            .units()
            .into_iter()
            .filter(|u| !packages.iter().any(|p| unit_name(p.id()) == *u) && self.owns(u).is_ok())
            .collect();
        if packages.is_empty() && stale.is_empty() {
            return Ok(Vec::new());
        }
        self.manager().available()?;
        let mut changed = false;
        let mut errors = Vec::new();
        let mut registered = Vec::new();
        for package in &packages {
            let unit = unit_name(package.id());
            match self.owns(&unit).and_then(|()| self.unit_text(package.id())).and_then(|text| self.manager().write_unit(&unit, Some(&text))) {
                Ok(wrote) => {
                    changed |= wrote;
                    registered.push(package);
                }
                Err(e) => errors.push(format!("cannot register {}: {e}", package.id())),
            }
        }
        for unit in &stale {
            match self.manager().write_unit(unit, None) {
                Ok(removed) => changed |= removed,
                Err(e) => errors.push(format!("cannot unregister {unit}: {e}")),
            }
        }
        if changed {
            self.manager().reload()?;
        }
        let mut started = Vec::new();
        // Only units written for this prefix: another prefix's unit of the same ID is not ours to start.
        for package in registered.into_iter().filter(|p| self.boot_flag(p.id()).exists()) {
            match self.manager().start(&unit_name(package.id()), false) {
                Ok(()) => started.push(package.id().to_string()),
                Err(e) => errors.push(format!("cannot start {}: {e}", package.id())),
            }
        }
        if errors.is_empty() {
            Ok(started)
        } else {
            Err(errors.join("\n"))
        }
    }

    /// Runs the service in this process, as its unit does: prepares its
    /// state and runtime folders and replaces this process with its entry,
    /// which keeps the service's running lock. Returns only on failure.
    pub fn exec_service(&self, id: &str) -> String {
        let root = self.services_dir();
        if let Err(e) = self.find_service(id) {
            return e;
        }
        // Held by the service and everything it starts: an update or
        // uninstall waits for all of them to stop.
        let running = match self.hold_running(&root, id) {
            Ok(file) => file,
            Err(e) => return format!("cannot mark {id} running: {e}"),
        };
        let package = match self.find_service(id) {
            Ok(package) => package,
            Err(e) => return e,
        };
        let state = self.service_data(id);
        let runtime = std::env::var_os("RUNTIME_DIRECTORY").map(PathBuf::from).unwrap_or_else(|| self.service_runtime(id));
        for dir in [&state, &runtime] {
            if let Err(e) = private_dir(dir) {
                return format!("{}: {e}", dir.display());
            }
        }
        let entry = package.entry();
        let mut cmd = match apps::entry_command(self, &entry) {
            Ok(cmd) => cmd,
            Err(e) => return format!("cannot prepare {id}: {e}"),
        };
        cmd.current_dir(&package.dir)
            .env("GIZLET_SERVICE_ID", id)
            .env("GIZLET_SERVICE_DIR", &package.dir)
            .env("GIZLET_STATE_DIR", &state)
            .env("GIZLET_RUNTIME_DIR", &runtime)
            .env("HOME", &state);
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            use std::os::unix::process::CommandExt;
            unsafe { libc::fcntl(running.as_raw_fd(), libc::F_SETFD, 0) };
            let err = cmd.exec();
            format!("cannot start {}: {err}", entry.display())
        }
        #[cfg(not(unix))]
        {
            drop(running);
            format!("cannot start {}: services need Unix", entry.display())
        }
    }
}

/// An empty file that exists until dropped, unless the process ends first.
struct Flag(PathBuf);

impl Flag {
    fn write(path: PathBuf) -> Result<Flag, String> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        fs::write(&path, "").map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Flag(path))
    }
}

impl Drop for Flag {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Deletes a folder and everything in it, if it exists.
fn remove_tree(dir: &Path) -> io::Result<()> {
    match fs::symlink_metadata(dir) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(dir),
        Ok(_) => fs::remove_file(dir),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Creates a folder only its owner can enter, if it does not exist.
fn private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::apps::tests::{leftovers, manifest_v, package, temp};
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::{Arc, Mutex};

    /// What the stand-in for systemd knows and was asked.
    #[derive(Default)]
    pub(crate) struct Units {
        pub text: BTreeMap<String, String>,
        pub active: BTreeSet<String>,
        pub failed: BTreeSet<String>,
        /// Each running unit's process holds its running lock.
        held: BTreeMap<String, File>,
        /// Units whose next start fails as soon as their process starts.
        pub crash_on_start: BTreeSet<String>,
        /// Units whose next start exits successfully at once.
        pub exit_on_start: BTreeSet<String>,
        /// Units that cannot be stopped.
        pub stuck: BTreeSet<String>,
        /// Units that stop this many more times, then cannot be stopped.
        pub stops_left: BTreeMap<String, u32>,
        pub unavailable: bool,
        pub log: Vec<String>,
    }

    /// systemd's stand-in: units kept in memory, started units hold the running lock.
    pub(crate) struct Fake {
        services: PathBuf,
        pub units: Arc<Mutex<Units>>,
    }

    impl Manager for Fake {
        fn available(&self) -> Result<(), String> {
            if self.units.lock().unwrap().unavailable { Err("no service manager here".into()) } else { Ok(()) }
        }
        fn write_unit(&self, unit: &str, text: Option<&str>) -> Result<bool, String> {
            let mut u = self.units.lock().unwrap();
            u.log.push(format!("write {unit} {}", text.is_some()));
            Ok(match text {
                Some(t) => u.text.insert(unit.into(), t.into()).as_deref() != Some(t),
                None => u.text.remove(unit).is_some(),
            })
        }
        fn units(&self) -> Vec<String> {
            self.units.lock().unwrap().text.keys().cloned().collect()
        }
        fn read_unit(&self, unit: &str) -> Option<String> {
            self.units.lock().unwrap().text.get(unit).cloned()
        }
        fn reload(&self) -> Result<(), String> {
            self.units.lock().unwrap().log.push("reload".into());
            Ok(())
        }
        fn start(&self, unit: &str, wait: bool) -> Result<(), String> {
            let mut u = self.units.lock().unwrap();
            u.log.push(format!("start {unit}{}", if wait { "" } else { " --no-block" }));
            if !u.text.contains_key(unit) {
                return Err(format!("Unit {unit} not found."));
            }
            u.failed.remove(unit);
            if u.crash_on_start.remove(unit) {
                u.failed.insert(unit.into());
                return Ok(());
            }
            if u.exit_on_start.remove(unit) {
                return Ok(());
            }
            if u.active.insert(unit.into()) {
                let id = unit.strip_prefix(UNIT_PREFIX).unwrap().strip_suffix(".service").unwrap();
                let file = File::options().create(true).truncate(false).write(true).open(self.services.join(format!(".running-{id}"))).unwrap();
                file.lock_shared().unwrap();
                u.held.insert(unit.into(), file);
            }
            Ok(())
        }
        fn stop(&self, unit: &str) -> Result<(), String> {
            let mut u = self.units.lock().unwrap();
            u.log.push(format!("stop {unit}"));
            if let Some(left) = u.stops_left.get_mut(unit) {
                if *left == 0 {
                    return Err(format!("Job for {unit} failed."));
                }
                *left -= 1;
            }
            if u.stuck.contains(unit) {
                return Err(format!("Job for {unit} failed."));
            }
            u.active.remove(unit);
            u.held.remove(unit);
            Ok(())
        }
        fn reset_failed(&self, unit: &str) {
            self.units.lock().unwrap().failed.remove(unit);
        }
        fn states(&self, units: &[String]) -> Result<Vec<UnitState>, String> {
            let u = self.units.lock().unwrap();
            Ok(units
                .iter()
                .map(|unit| {
                    let load = if u.text.contains_key(unit) { "loaded" } else { "not-found" };
                    let (active, sub, result) = if u.active.contains(unit) {
                        ("active", "running", "success")
                    } else if u.failed.contains(unit) {
                        ("failed", "failed", "exit-code")
                    } else {
                        ("inactive", "dead", "success")
                    };
                    UnitState { load: load.into(), active: active.into(), sub: sub.into(), result: result.into(), ..UnitState::default() }
                })
                .collect())
        }
        fn probation(&self) -> Duration {
            Duration::ZERO
        }
    }

    /// A library in `dir/prefix` whose services run under a fresh [`Fake`]:
    /// a new one is what a reboot leaves, with the same files.
    pub(crate) fn library(dir: &Path) -> (Library, Arc<Mutex<Units>>) {
        let prefix = dir.join("prefix");
        let units = Arc::new(Mutex::new(Units::default()));
        let fake = Fake { services: std::path::absolute(prefix.join("services")).unwrap(), units: units.clone() };
        (Library::with_manager(prefix, Box::new(fake), dir.join("run")), units)
    }

    fn service(dir: &Path, id: &str, version: &str) -> PathBuf {
        let m = manifest_v(id, Some(2), Some("service")).replace("\"0.1.0\"", &format!("{version:?}"));
        package(dir, &m, &[])
    }

    fn app(dir: &Path, id: &str) -> PathBuf {
        package(dir, &manifest_v(id, Some(2), Some("app")), &[])
    }

    const A: &str = "dev.example.clock";
    const B: &str = "dev.example.sync";

    fn unit(id: &str) -> String {
        unit_name(id)
    }

    #[test]
    fn installs_and_controls_a_service() {
        let dir = temp("svc-cycle");
        let (lib, units) = library(&dir);
        let installed = lib.install(&service(&dir, A, "0.1.0")).unwrap();
        assert_eq!(installed.dir, lib.services_dir().join(A));
        assert!(lib.scan().is_empty(), "a service is not on the shelf");
        assert!(lib.open_for_run(A).unwrap_err().contains("is a service, not an app"));
        assert!(!lib.apps_dir().join(format!(".running-{A}")).exists(), "refusing to run it leaves nothing behind");
        let text = units.lock().unwrap().text[&unit(A)].clone();
        for line in [
            "Type=exec",
            &format!(" service exec {A}\n"),
            &format!("RuntimeDirectory=gizlet-services/{A}\n"),
            "KillMode=control-group",
            "Restart=on-failure",
            "PrivateTmp=yes",
            &format!("SyslogIdentifier={A}\n"),
        ] {
            assert!(text.contains(line), "{line:?} in\n{text}");
        }
        // A new service runs and starts at boot.
        let s = lib.service(A).unwrap();
        assert_eq!((s.state.clone(), s.boot), (State::Running, true));
        assert_eq!(lib.services().unwrap().len(), 1);
        assert_eq!(lib.stop_service(A).unwrap().state, State::Stopped);
        assert_eq!(lib.start_service(A).unwrap().state, State::Running);
        assert!(!lib.set_service_boot(A, false).unwrap().boot);
        assert_eq!(lib.restart_service(A).unwrap().state, State::Running);
        // After a reboot nothing is registered: boot registers it and starts only what starts at boot.
        drop((lib, units));
        let (lib, units) = library(&dir);
        assert_eq!(lib.boot_services().unwrap(), Vec::<String>::new());
        assert!(units.lock().unwrap().text.contains_key(&unit(A)));
        assert_eq!(lib.service(A).unwrap().state, State::Stopped);
        lib.set_service_boot(A, true).unwrap();
        drop((lib, units));
        let (lib, units) = library(&dir);
        assert_eq!(lib.boot_services().unwrap(), [A]);
        assert!(units.lock().unwrap().log.contains(&format!("start {} --no-block", unit(A))));
        assert_eq!(leftovers(&lib), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_keeps_state_and_running() {
        let dir = temp("svc-update");
        let (lib, units) = library(&dir);
        lib.install(&service(&dir, A, "0.1.0")).unwrap();
        fs::create_dir_all(lib.service_data(A)).unwrap();
        fs::write(lib.service_data(A).join("state"), "kept").unwrap();
        lib.set_service_boot(A, false).unwrap();
        units.lock().unwrap().log.clear();
        let updated = lib.install(&service(&dir, A, "0.2.0")).unwrap();
        assert_eq!(updated.manifest.version, "0.2.0");
        let log = units.lock().unwrap().log.clone();
        let (stop, start) = (format!("stop {}", unit(A)), format!("start {}", unit(A)));
        assert!(log.iter().position(|l| *l == stop) < log.iter().position(|l| *l == start), "{log:?}");
        let s = lib.service(A).unwrap();
        assert_eq!((s.state, s.boot), (State::Running, false), "it runs again and keeps its boot choice");
        assert_eq!(fs::read_to_string(lib.service_data(A).join("state")).unwrap(), "kept");
        // A stopped service stays stopped through an update.
        lib.stop_service(A).unwrap();
        lib.install(&service(&dir, A, "0.3.0")).unwrap();
        assert_eq!(lib.service(A).unwrap().state, State::Stopped);
        assert_eq!(leftovers(&lib), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_update_brings_back_the_previous_version() {
        let dir = temp("svc-rollback");
        let (lib, units) = library(&dir);
        lib.install(&service(&dir, A, "0.1.0")).unwrap();
        units.lock().unwrap().crash_on_start.insert(unit(A));
        let err = lib.install(&service(&dir, A, "0.2.0")).unwrap_err();
        assert!(err.contains("0.2.0 did not start") && err.contains("0.1.0 is back and running"), "{err}");
        let s = lib.service(A).unwrap();
        assert_eq!((s.package.manifest.version.as_str(), s.state), ("0.1.0", State::Running));
        // Exiting at once, even successfully, is not starting.
        units.lock().unwrap().exit_on_start.insert(unit(A));
        let err = lib.install(&service(&dir, A, "0.2.0")).unwrap_err();
        assert!(err.contains("it exited (success)") && err.contains("0.1.0 is back and running"), "{err}");
        assert_eq!(lib.service(A).unwrap().package.manifest.version, "0.1.0");
        assert_eq!(leftovers(&lib), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_update_cut_short_before_it_started_is_undone() {
        let dir = temp("svc-power-loss");
        let (lib, units) = library(&dir);
        lib.install(&service(&dir, A, "0.1.0")).unwrap();
        // Power is lost after 0.2.0 took its place, before it was confirmed to start:
        // the backup and the new version are both there.
        lib.stop_service(A).unwrap();
        let (dest, backup) = (lib.services_dir().join(A), lib.backup_in(&lib.services_dir(), A));
        fs::rename(&dest, &backup).unwrap();
        fs::create_dir_all(dest.join("bin")).unwrap();
        let newer = manifest_v(A, Some(2), Some("service")).replace("\"0.1.0\"", "\"0.2.0\"");
        fs::write(dest.join(crate::apps::MANIFEST), newer).unwrap();
        fs::write(dest.join("bin/hello"), "#!/bin/sh\n").unwrap();
        drop((lib, units));
        let (lib, _units) = library(&dir);
        assert_eq!(lib.boot_services().unwrap(), [A]);
        let s = lib.service(A).unwrap();
        assert_eq!((s.package.manifest.version.as_str(), s.state), ("0.1.0", State::Running));
        assert_eq!(leftovers(&lib), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rollback_cut_short_is_finished_and_runs_the_previous_version() {
        let dir = temp("svc-rollback-cut");
        let (lib, units) = library(&dir);
        lib.install(&service(&dir, A, "0.1.0")).unwrap();
        // 0.2.0 crashes, and the rollback cannot stop it: the update's own stop is the last that works.
        units.lock().unwrap().crash_on_start.insert(unit(A));
        units.lock().unwrap().stops_left.insert(unit(A), 1);
        let err = lib.install(&service(&dir, A, "0.2.0")).unwrap_err();
        assert!(err.contains("restoring 0.1.0 failed"), "{err}");
        assert!(lib.restart_flag(A).exists(), "the flag stays for recovery");
        // Once systemd can stop it again, the next read finishes the rollback and runs 0.1.0.
        units.lock().unwrap().stops_left.remove(&unit(A));
        let s = lib.services().unwrap().remove(0);
        assert_eq!((s.package.manifest.version.as_str(), s.state), ("0.1.0", State::Running));
        assert!(!lib.restart_flag(A).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_update_that_cannot_be_committed_is_undone() {
        let dir = temp("svc-commit");
        let (lib, _units) = library(&dir);
        lib.install(&service(&dir, A, "0.1.0")).unwrap();
        // A file where the backup would be renamed to: the rename fails, as on a full disk.
        let blocker = lib.services_dir().join(format!(".remove-{A}-{}-old", std::process::id()));
        fs::write(&blocker, "").unwrap();
        let err = lib.install(&service(&dir, A, "0.2.0")).unwrap_err();
        assert!(err.contains("started, but its update cannot be confirmed") && err.contains("0.1.0 is back and running"), "{err}");
        let s = lib.service(A).unwrap();
        assert_eq!((s.package.manifest.version.as_str(), s.state), ("0.1.0", State::Running));
        fs::remove_file(&blocker).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_service_that_does_not_start_is_not_installed() {
        let dir = temp("svc-crash");
        let (lib, units) = library(&dir);
        units.lock().unwrap().crash_on_start.insert(unit(A));
        let err = lib.install(&service(&dir, A, "0.1.0")).unwrap_err();
        assert!(err.contains("did not start") && err.contains("it was not installed"), "{err}");
        assert_nothing_left(&lib, &units, A);
        // Once it starts, it installs.
        lib.install(&service(&dir, A, "0.1.0")).unwrap();
        assert_eq!(lib.service(A).unwrap().state, State::Running);
        let _ = fs::remove_dir_all(&dir);
    }

    fn assert_nothing_left(lib: &Library, units: &Arc<Mutex<Units>>, id: &str) {
        let u = units.lock().unwrap();
        assert!(!u.text.contains_key(&unit(id)) && !u.active.contains(&unit(id)), "{id} is unregistered and stopped");
        assert!(!u.held.contains_key(&unit(id)), "no process of {id} remains");
        for path in [
            lib.services_dir().join(id),
            lib.boot_flag(id),
            lib.running_in(&lib.services_dir(), id),
            lib.service_data(id),
            lib.service_runtime(id),
            lib.restart_flag(id),
        ] {
            assert!(!path.exists(), "{} remains", path.display());
        }
        assert_eq!(leftovers(lib), 0);
        let hidden: Vec<String> = fs::read_dir(lib.services_dir())
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with('.') && n.contains(id))
            .collect();
        assert!(hidden.is_empty(), "{hidden:?} remain");
    }

    #[test]
    fn uninstall_removes_a_running_service_completely() {
        let dir = temp("svc-uninstall");
        let (lib, units) = library(&dir);
        lib.install(&service(&dir, A, "0.1.0")).unwrap();
        lib.install(&service(&dir, B, "0.1.0")).unwrap();
        lib.install(&app(&dir, "dev.example.game")).unwrap();
        for dir in [lib.service_data(A), lib.service_runtime(A)] {
            fs::create_dir_all(dir.join("sub")).unwrap();
            fs::write(dir.join("sub/file"), "x").unwrap();
        }
        fs::create_dir_all(lib.service_data(B)).unwrap();
        assert_eq!(lib.remove(A, false).unwrap(), Kind::Service);
        assert_nothing_left(&lib, &units, A);
        // Unrelated services and apps stay.
        assert_eq!(lib.service(B).unwrap().state, State::Running);
        assert!(lib.service_data(B).is_dir());
        assert_eq!(lib.scan().len(), 1);
        // It stays gone after a reboot, and installs again.
        drop((lib, units));
        let (lib, units) = library(&dir);
        assert_eq!(lib.boot_services().unwrap(), [B]);
        assert!(!units.lock().unwrap().text.contains_key(&unit(A)));
        lib.install(&service(&dir, A, "0.1.0")).unwrap();
        assert_eq!(lib.service(A).unwrap().state, State::Running);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn uninstall_says_what_to_do_when_it_cannot_finish() {
        let dir = temp("svc-stuck");
        let (lib, units) = library(&dir);
        lib.install(&service(&dir, A, "0.1.0")).unwrap();
        units.lock().unwrap().stuck.insert(unit(A));
        let err = lib.remove(A, false).unwrap_err();
        assert!(err.contains("cannot stop") && err.contains("restart the device") && err.contains(&format!("gizlet uninstall {A}")), "{err}");
        assert!(lib.services_dir().join(A).is_dir(), "nothing was removed");
        assert!(!lib.service(A).unwrap().boot, "it no longer starts at boot");
        // After the restart it does not start, and uninstalls.
        drop((lib, units));
        let (lib, units) = library(&dir);
        assert!(lib.boot_services().unwrap().is_empty());
        lib.remove(A, false).unwrap();
        assert_nothing_left(&lib, &units, A);

        // A copy started by hand, outside its unit, is reported too.
        lib.install(&service(&dir, A, "0.1.0")).unwrap();
        lib.stop_service(A).unwrap();
        let by_hand = lib.hold_running(&lib.services_dir(), A).unwrap();
        let err = lib.remove(A, false).unwrap_err();
        assert!(err.contains("outside") && err.contains(&format!("gizlet service exec {A}")), "{err}");
        drop(by_hand);
        lib.remove(A, false).unwrap();

        // A state folder that cannot be deleted keeps the package, so uninstalling again finishes.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            lib.install(&service(&dir, A, "0.1.0")).unwrap();
            let locked = lib.service_data(A).join("locked");
            fs::create_dir_all(locked.join("inner")).unwrap();
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();
            let err = lib.remove(A, false).unwrap_err();
            assert!(err.contains("stopped and unregistered") && err.contains("cannot be deleted"), "{err}");
            // It is no longer installed, and the uninstall stays marked until it finishes.
            assert!(lib.uninstalling(A).is_dir() && lib.services().unwrap().is_empty());
            assert!(lib.install(&service(&dir, A, "0.1.0")).unwrap_err().contains("did not finish"));
            assert!(lib.remove(A, false).unwrap_err().contains("cannot be deleted"), "uninstalling again says why it still cannot");
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(lib.remove(A, false).unwrap(), Kind::Service);
            assert_nothing_left(&lib, &units, A);
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_uninstall_cut_short_is_finished() {
        let dir = temp("svc-uninstall-cut");
        let (lib, units) = library(&dir);
        lib.install(&service(&dir, A, "0.1.0")).unwrap();
        fs::create_dir_all(lib.service_data(A).join("deep")).unwrap();
        // Power is lost once it is stopped and marked, before its state is deleted.
        lib.stop_service(A).unwrap();
        fs::rename(lib.services_dir().join(A), lib.uninstalling(A)).unwrap();
        drop((lib, units));
        let (lib, units) = library(&dir);
        let text = lib.unit_text(A).unwrap();
        units.lock().unwrap().text.insert(unit(A), text);
        assert!(lib.boot_services().unwrap().is_empty(), "it does not start again");
        assert_nothing_left(&lib, &units, A);
        // Uninstalling it again once recovery has finished says it is gone.
        assert!(lib.remove(A, false).unwrap_err().contains("not installed"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_confirmed_update_keeps_its_version_whatever_cuts_short_the_cleanup() {
        let dir = temp("svc-cleanup-cut");
        let (lib, _units) = library(&dir);
        lib.install(&service(&dir, A, "0.1.0")).unwrap();
        lib.install(&service(&dir, A, "0.2.0")).unwrap();
        // The deletion of the committed backup is cut short partway.
        let half = lib.services_dir().join(format!(".remove-{A}-1"));
        fs::create_dir_all(half.join("bin")).unwrap();
        lib.scan();
        assert!(!half.exists());
        assert_eq!(lib.service(A).unwrap().package.manifest.version, "0.2.0");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn listing_services_recovers_an_interrupted_update() {
        let dir = temp("svc-list-recovers");
        let (lib, _units) = library(&dir);
        lib.install(&service(&dir, A, "0.1.0")).unwrap();
        // The installer is killed after moving the running version aside, before the new one is in place.
        fs::create_dir_all(lib.run_dir()).unwrap();
        fs::write(lib.restart_flag(A), "").unwrap();
        lib.manager().stop(&unit(A)).unwrap();
        fs::rename(lib.services_dir().join(A), lib.backup_in(&lib.services_dir(), A)).unwrap();
        let listed = lib.services().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].id(), &listed[0].state), (A, &State::Running));
        // While another process holds the library, reading neither waits nor recovers.
        let held = lib.lock_waiting().unwrap();
        fs::rename(lib.services_dir().join(A), lib.backup_in(&lib.services_dir(), A)).unwrap();
        assert!(lib.services().unwrap().is_empty());
        drop(held);
        assert_eq!(lib.service(A).unwrap().package.manifest.version, "0.1.0");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_update_cut_short_while_stopping_runs_the_service_again() {
        let dir = temp("svc-killed-stop");
        let (lib, _units) = library(&dir);
        lib.install(&service(&dir, A, "0.1.0")).unwrap();
        // The installer is killed once it has stopped the service, before making a backup.
        fs::create_dir_all(lib.run_dir()).unwrap();
        fs::write(lib.restart_flag(A), "").unwrap();
        lib.manager().stop(&unit(A)).unwrap();
        let s = lib.services().unwrap().remove(0);
        assert_eq!((s.package.manifest.version.as_str(), s.state), ("0.1.0", State::Running));
        assert!(!lib.restart_flag(A).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_update_cut_short_in_this_boot_runs_the_previous_version() {
        let dir = temp("svc-killed-update");
        let (lib, units) = library(&dir);
        lib.install(&service(&dir, A, "0.1.0")).unwrap();
        // The installer is killed while 0.2.0 is on probation: the flag that 0.1.0 ran is still there.
        fs::create_dir_all(lib.run_dir()).unwrap();
        fs::write(lib.restart_flag(A), "").unwrap();
        lib.manager().stop(&unit(A)).unwrap();
        let (dest, backup) = (lib.services_dir().join(A), lib.backup_in(&lib.services_dir(), A));
        fs::rename(&dest, &backup).unwrap();
        fs::create_dir_all(dest.join("bin")).unwrap();
        let newer = manifest_v(A, Some(2), Some("service")).replace("\"0.1.0\"", "\"0.2.0\"");
        fs::write(dest.join(crate::apps::MANIFEST), newer).unwrap();
        fs::write(dest.join("bin/hello"), "#!/bin/sh\n").unwrap();
        lib.manager().start(&unit(A), true).unwrap();
        // The launcher's next scan recovers it: 0.1.0 is back and running.
        lib.scan();
        let s = lib.service(A).unwrap();
        assert_eq!((s.package.manifest.version.as_str(), s.state), ("0.1.0", State::Running));
        assert!(!lib.restart_flag(A).exists() && !backup.exists());
        assert!(units.lock().unwrap().log.contains(&format!("start {} --no-block", unit(A))));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn another_prefix_leaves_running_services_alone() {
        let dir = temp("svc-two-prefixes");
        let (lib, _units) = library(&dir);
        lib.install(&service(&dir, A, "0.1.0")).unwrap();
        fs::create_dir_all(lib.service_runtime(A)).unwrap();
        fs::write(lib.service_runtime(A).join("control.sock"), "").unwrap();
        // A second library, as with GIZLET_PREFIX, shares the runtime folder and the service manager.
        let other_prefix = std::path::absolute(dir.join("other")).unwrap();
        let fake = Fake { services: other_prefix.join("services"), units: _units.clone() };
        let other = Library::with_manager(other_prefix, Box::new(fake), dir.join("run"));
        other.install(&app(&dir, "dev.example.game")).unwrap();
        assert!(other.boot_services().unwrap().is_empty());
        assert!(lib.service_runtime(A).join("control.sock").exists());
        assert!(_units.lock().unwrap().text.contains_key(&unit(A)), "the other library's units stay registered");
        // Nor can it take over the unit with a service of the same ID.
        let err = other.install(&service(&dir, A, "0.2.0")).unwrap_err();
        assert!(err.contains("it is left alone"), "{err}");
        assert!(!other.services_dir().join(A).exists());
        let s = lib.service(A).unwrap();
        assert_eq!((s.package.manifest.version.as_str(), s.state), ("0.1.0", State::Running));
        // Finishing an old uninstall of that ID there leaves this prefix's runtime folder alone.
        fs::create_dir_all(other.uninstalling(A)).unwrap();
        other.scan();
        assert!(!other.uninstalling(A).exists());
        assert!(lib.service_runtime(A).join("control.sock").exists());
        // After a reboot the other prefix installs the same ID first; booting this one leaves it alone.
        drop((lib, other, _units));
        let shared: Arc<Mutex<Units>> = Default::default();
        let prefix = std::path::absolute(dir.join("prefix")).unwrap();
        let lib = Library::with_manager(prefix.clone(), Box::new(Fake { services: prefix.join("services"), units: shared.clone() }), dir.join("run"));
        let other_prefix = std::path::absolute(dir.join("other")).unwrap();
        let other = Library::with_manager(other_prefix.clone(), Box::new(Fake { services: other_prefix.join("services"), units: shared.clone() }), dir.join("run"));
        other.install(&service(&dir, A, "0.2.0")).unwrap();
        other.stop_service(A).unwrap();
        assert!(lib.boot_services().unwrap_err().contains("it is left alone"));
        assert!(!shared.lock().unwrap().active.contains(&unit(A)), "the other prefix's stopped service stays stopped");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn apps_and_services_do_not_share_ids() {
        let dir = temp("svc-kinds");
        let (lib, _) = library(&dir);
        lib.install(&app(&dir, A)).unwrap();
        assert!(lib.install(&service(&dir, A, "0.1.0")).unwrap_err().contains("installed as an app"));
        assert!(lib.start_service(A).unwrap_err().contains("is an app, not a service"));
        lib.remove(A, false).unwrap();
        lib.install(&service(&dir, A, "0.1.0")).unwrap();
        assert!(lib.install(&app(&dir, A)).unwrap_err().contains("installed as a service"));
        assert_eq!(leftovers(&lib), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recovers_interrupted_updates_and_uninstalls() {
        let dir = temp("svc-recover");
        let (lib, units) = library(&dir);
        lib.install(&service(&dir, A, "0.1.0")).unwrap();
        lib.stop_service(A).unwrap();
        // Cut short after the installed copy became the backup.
        fs::rename(lib.services_dir().join(A), lib.backup_in(&lib.services_dir(), A)).unwrap();
        assert_eq!(lib.services().unwrap().len(), 1, "reading the services puts it back");
        assert_eq!(lib.boot_services().unwrap(), [A]);
        // What an uninstall cut short leaves of a service no longer installed.
        let gone = "dev.example.gone";
        fs::create_dir_all(lib.service_data(gone)).unwrap();
        fs::write(lib.boot_flag(gone), "").unwrap();
        fs::write(lib.running_in(&lib.services_dir(), gone), "").unwrap();
        fs::create_dir_all(lib.services_dir().join(format!(".remove-{gone}-1"))).unwrap();
        let text = lib.unit_text(gone).unwrap();
        units.lock().unwrap().text.insert(unit(gone), text);
        lib.boot_services().unwrap();
        assert_nothing_left(&lib, &units, gone);
        assert!(lib.services_dir().join(A).is_dir());
        // A backup that cannot be put back yet keeps the service's state and boot choice.
        let held = "dev.example.held";
        let backup = lib.backup_in(&lib.services_dir(), held);
        fs::create_dir_all(&backup).unwrap();
        fs::create_dir_all(lib.service_data(held)).unwrap();
        fs::write(lib.boot_flag(held), "").unwrap();
        lib.sweep_services();
        assert!(lib.service_data(held).is_dir() && lib.boot_flag(held).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn services_need_a_service_manager() {
        let dir = temp("svc-none");
        let (lib, units) = library(&dir);
        units.lock().unwrap().unavailable = true;
        let err = lib.install(&service(&dir, A, "0.1.0")).unwrap_err();
        assert!(err.contains("cannot install the service") && err.contains("no service manager"), "{err}");
        assert!(!lib.services_dir().join(A).exists());
        assert_eq!(leftovers(&lib), 0);
        // With nothing installed, boot needs no manager.
        assert!(lib.boot_services().unwrap().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_systemctl_show() {
        let state = UnitState { active: "activating".into(), sub: "auto-restart".into(), result: "exit-code".into(), restarts: 1, ..UnitState::default() };
        assert_eq!(State::of(&state), State::Restarting("exit-code".into()));
        assert!(State::of(&state).on(), "a restarting service shows as on, so its switch stops it");
        assert!(state.start_problem().unwrap().contains("restarted"));
        let clean = UnitState { active: "inactive".into(), sub: "dead".into(), result: "success".into(), ..UnitState::default() };
        assert_eq!((State::of(&clean), clean.running()), (State::Stopped, false));
        assert_eq!(clean.start_problem().as_deref(), Some("it exited (success)"));
        let up = UnitState { active: "active".into(), sub: "running".into(), result: "success".into(), ..UnitState::default() };
        assert_eq!(up.start_problem(), None);
        let left = UnitState { cgroup: "/system.slice/x.service".into(), ..clean };
        assert!(left.running(), "processes left in its control group still run");
    }
}
