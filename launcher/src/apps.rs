//! Installed apps: `.gzl` packages and the folders they install into.
//!
//! A `.gzl` is the package of the Gizlet store contract: a zip with
//! `manifest.json` at its root and `CHECKSUMS`, the SHA-256 of every other file
//! (`<64 hex>  <path>` per line). Installing extracts it into
//! `<prefix>/apps/<id>/`, where the prefix is `/opt/gizlet` on Gizlet OS. Each
//! app keeps its own files in `<prefix>/saves/<id>/`, which outlive updates.
//! A service package installs into `<prefix>/services/<id>/` instead and runs
//! in the background (see [`crate::services`]).
//!
//! The reader accepts only what the store accepts: stored or deflated entries
//! with canonical relative names, no symlinks, devices, encryption or ZIP64,
//! and no two entries landing on the same path.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const MANIFEST: &str = "manifest.json";
pub const CHECKSUMS: &str = "CHECKSUMS";
const MAX_ENTRIES: usize = 20_000;
const MAX_TOTAL_BYTES: u64 = 8 << 30;
const MAX_MANIFEST_BYTES: u64 = 64 << 10;
/// CHECKSUMS is read whole: one line per file is far below this.
const MAX_CHECKSUMS_BYTES: u64 = 4 << 20;
/// The zip directory is read whole: 20,000 entries with long names fit well below this.
const MAX_DIRECTORY_BYTES: u64 = 16 << 20;
/// Asks the launcher on the screen to start an app: it writes the app's ID here
/// and quits, and its start script runs the app, then the launcher again.
pub const LAUNCH_REQUEST: &str = "/run/gizlet-launch";
/// The running launcher's process ID, for `gizlet start`.
pub const LAUNCHER_PID: &str = "/run/gizlet-launcher.pid";

/// Writes the request the launcher's start script reads. Each request is
/// written to its own file, then renamed over the request, so two at once
/// never publish a partial or each other's ID.
pub fn request_launch(id: &str) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let tmp = format!("{LAUNCH_REQUEST}.{}-{}.new", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed));
    let result = fs::write(&tmp, format!("{id}\n")).and_then(|()| fs::rename(&tmp, LAUNCH_REQUEST));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// `/opt/gizlet` on Gizlet OS, the launcher's data folder elsewhere, or `GIZLET_PREFIX`.
pub fn prefix() -> PathBuf {
    if let Some(dir) = std::env::var_os("GIZLET_PREFIX") {
        return PathBuf::from(dir);
    }
    if cfg!(feature = "gizlet-os") {
        PathBuf::from("/opt/gizlet")
    } else {
        crate::settings::data_dir()
    }
}

/// The target triple packages are built for here, or `GIZLET_TARGET`.
pub fn device_target() -> String {
    if let Ok(t) = std::env::var("GIZLET_TARGET") {
        return t;
    }
    let arch = std::env::consts::ARCH;
    match std::env::consts::OS {
        "linux" => format!("{arch}-unknown-linux-gnu"),
        "macos" => format!("{arch}-apple-darwin"),
        "windows" => format!("{arch}-pc-windows-msvc"),
        os => format!("{arch}-unknown-{os}"),
    }
}

/// `lite` on handhelds, or `GIZLET_TIER`: `lite` packages run on `lite` and `full`, `full` only on `full`.
pub fn device_tier() -> String {
    std::env::var("GIZLET_TIER").unwrap_or_else(|_| "lite".into())
}

/// Linux runs packages for its C library or static musl ones of the same CPU.
fn runs_here(target: &str, device: &str) -> bool {
    if target == device {
        return true;
    }
    let arch = |t: &str| t.split('-').next().unwrap_or("").to_string();
    device.contains("-linux-") && target.ends_with("-unknown-linux-musl") && arch(target) == arch(device)
}

/// What a package is, from its manifest: schema 1 (or no `schema`) is always
/// an app; schema 2 names `app` or `service` in `kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A foreground program, games included: it is shown on the shelf and takes the screen.
    App,
    /// A background program with no screen: installed under `services/` and
    /// run by the system's service manager, never from the shelf.
    Service,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Manifest {
    schema: Option<u32>,
    /// As written; [`Manifest::kind`] reads it after validation.
    #[serde(rename = "kind")]
    declared_kind: Option<String>,
    runtime: Option<u32>,
    pub id: String,
    pub name: String,
    pub version: String,
    pub tier: String,
    pub target: String,
    pub entry: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub names: BTreeMap<String, String>,
    #[serde(default)]
    pub descriptions: BTreeMap<String, String>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, String> {
        if !text.trim_start().starts_with('{') {
            return Err(format!("{MANIFEST} must be a JSON object"));
        }
        let manifest: Manifest = serde_json::from_str(text).map_err(|e| format!("{MANIFEST}: {e}"))?;
        manifest.validate()
    }

    fn validate(self) -> Result<Manifest, String> {
        match (self.schema.unwrap_or(1), self.declared_kind.as_deref()) {
            (1, None | Some("app")) | (2, Some("app" | "service")) => {}
            (1, Some(kind)) => return Err(format!("manifest schema 1 is always an app, but it has kind {kind:?}")),
            (2, None) => return Err("manifest schema 2 must name its kind (app or service)".into()),
            (2, Some(kind)) => return Err(format!("unsupported package kind {kind:?}")),
            (schema, _) => return Err(format!("unsupported manifest schema {schema}")),
        }
        if let Some(runtime) = self.runtime.filter(|&runtime| runtime != 1) {
            return Err(format!("unsupported Gizlet runtime {runtime}"));
        }
        for (field, value) in [("name", &self.name), ("version", &self.version), ("entry", &self.entry)] {
            if value.trim().is_empty() {
                return Err(format!("manifest has no {field}"));
            }
        }
        if !is_valid_id(&self.id) {
            return Err(format!(
                "id {:?} must be reverse-DNS: lowercase letters, digits, '-' and '_' in dot-separated parts",
                self.id
            ));
        }
        if !matches!(self.tier.as_str(), "lite" | "full") {
            return Err(format!("tier {:?} must be lite or full", self.tier));
        }
        if self.target.split('-').count() < 3 {
            return Err(format!("target {:?} is not a target triple", self.target));
        }
        for (field, path) in [("entry", Some(&self.entry)), ("icon", self.icon.as_ref())] {
            if let Some(path) = path {
                if let Some(problem) = name_problem(path) {
                    return Err(format!("{field} {path:?} {problem}"));
                }
            }
        }
        Ok(self)
    }

    /// The package's kind; valid once the manifest is parsed.
    pub fn kind(&self) -> Kind {
        match self.declared_kind.as_deref() {
            Some("service") => Kind::Service,
            _ => Kind::App,
        }
    }

    pub fn name_for(&self, lang: &str) -> &str {
        localized(&self.names, lang).unwrap_or(&self.name)
    }

    /// `#RRGGBB` as RGB.
    pub fn rgb(&self) -> Option<[u8; 3]> {
        let hex = self.color.as_deref()?.strip_prefix('#')?;
        if hex.len() != 6 {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        Some([byte(0)?, byte(2)?, byte(4)?])
    }
}

/// Why a manifest's package cannot run as an app.
/// `an app` or `a service`.
pub fn kind_word(kind: Kind) -> &'static str {
    match kind {
        Kind::App => "an app",
        Kind::Service => "a service",
    }
}

pub fn not_an_app(manifest: &Manifest) -> String {
    format!("{0} is a service, not an app; start it with `gizlet service start {0}`", manifest.id)
}

fn localized<'a>(map: &'a BTreeMap<String, String>, lang: &str) -> Option<&'a str> {
    if let Some(v) = map.get(lang) {
        return Some(v);
    }
    let primary = lang.split(['-', '_']).next().unwrap_or(lang);
    map.iter().find(|(k, _)| k.split(['-', '_']).next() == Some(primary)).map(|(_, v)| v.as_str())
}

pub fn is_valid_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    id.len() <= 128
        && parts.len() >= 2
        && parts.iter().all(|p| {
            !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        })
}

/// Why `name` is not a canonical relative path: `/`-separated, no empty, `.` or `..` parts.
/// `:` is refused too, so no name is a Windows drive path (`C:/...`) or alternate stream.
fn name_problem(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("is empty");
    }
    if name.starts_with('/') || name.contains(['\\', '\0', ':']) {
        return Some("is not a relative path");
    }
    if name.split('/').any(|part| part.is_empty() || part == "." || part == "..") {
        return Some("has an empty, . or .. part");
    }
    // Whatever this platform's paths make of it, every part must stay below the folder.
    if !Path::new(name).components().all(|c| matches!(c, std::path::Component::Normal(_))) {
        return Some("is not a relative path");
    }
    None
}

/// An installed package: an app, or a service under `services/`.
#[derive(Clone, Debug)]
pub struct App {
    pub dir: PathBuf,
    pub manifest: Manifest,
    /// When it was installed or updated.
    pub installed: SystemTime,
}

impl App {
    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    pub fn entry(&self) -> PathBuf {
        self.dir.join(&self.manifest.entry)
    }

    pub fn icon(&self) -> Option<PathBuf> {
        self.manifest.icon.as_ref().map(|i| self.dir.join(i)).filter(|p| p.is_file())
    }
}

/// The apps under `<prefix>/apps` and the services under `<prefix>/services`.
pub struct Library {
    prefix: PathBuf,
    manager: Box<dyn crate::services::Manager>,
    run_dir: PathBuf,
}

impl Library {
    /// The prefix is made absolute, since apps run in their own folder.
    pub fn new(prefix: PathBuf) -> Library {
        Library::with_manager(prefix, crate::services::system_manager(), PathBuf::from(crate::services::RUN_DIR))
    }

    /// A library whose services run under `manager`, with their runtime folders in `run_dir`.
    pub fn with_manager(prefix: PathBuf, manager: Box<dyn crate::services::Manager>, run_dir: PathBuf) -> Library {
        let prefix = std::path::absolute(&prefix).unwrap_or(prefix);
        Library { prefix, manager, run_dir }
    }

    pub fn open() -> Library {
        Library::new(prefix())
    }

    pub(crate) fn prefix(&self) -> &Path {
        &self.prefix
    }

    pub(crate) fn manager(&self) -> &dyn crate::services::Manager {
        self.manager.as_ref()
    }

    pub(crate) fn run_dir(&self) -> &Path {
        &self.run_dir
    }

    pub fn apps_dir(&self) -> PathBuf {
        self.prefix.join("apps")
    }

    pub fn saves(&self, id: &str) -> PathBuf {
        self.prefix.join("saves").join(id)
    }

    fn played_path(&self) -> PathBuf {
        self.prefix.join("saves/.played")
    }

    /// Changes when an app is installed or removed.
    pub fn stamp(&self) -> Option<SystemTime> {
        fs::metadata(self.apps_dir()).and_then(|m| m.modified()).ok()
    }

    /// Installed apps, most recently played first, then most recently installed.
    /// Folders without a valid manifest, and services, are skipped.
    pub fn scan(&self) -> Vec<App> {
        // The launcher scans while `gizlet install` may be running: it never
        // waits for the lock, and recovers only when nothing else holds it.
        if let Some(_lock) = self.lock(false) {
            self.recover();
        }
        let played = self.played();
        let mut apps: Vec<App> = fs::read_dir(self.apps_dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .filter_map(|e| self.load(&e.path()).ok())
            .filter(|app| app.manifest.kind() == Kind::App)
            .collect();
        apps.sort_by(|a, b| {
            let pa = played.get(a.id()).copied().unwrap_or(0);
            let pb = played.get(b.id()).copied().unwrap_or(0);
            pb.cmp(&pa).then(b.installed.cmp(&a.installed)).then(a.id().cmp(b.id()))
        });
        apps
    }

    pub(crate) fn load(&self, dir: &Path) -> Result<App, String> {
        let path = dir.join(MANIFEST);
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let manifest = Manifest::parse(&text)?;
        if dir.file_name().is_none_or(|n| n.to_string_lossy() != manifest.id) {
            return Err(format!("{} holds {}", dir.display(), manifest.id));
        }
        let installed = fs::metadata(&path).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
        Ok(App { dir: dir.to_path_buf(), manifest, installed })
    }

    pub fn get(&self, id: &str) -> Result<App, String> {
        if let Some(_lock) = self.lock(true) {
            self.recover();
        }
        self.find(id)
    }

    /// The installed app or service `id`, without recovery or the lock.
    fn find(&self, id: &str) -> Result<App, String> {
        if !is_valid_id(id) {
            return Err(format!("{id:?} is not an app ID"));
        }
        let dir = self.apps_dir().join(id);
        if dir.is_dir() {
            return self.load(&dir);
        }
        if self.services_dir().join(id).is_dir() {
            return self.find_service(id);
        }
        Err(format!("{id} is not installed"))
    }

    /// Installs or updates the app or service in `package`, a `.gzl`. The
    /// previous version stays in place until the new one is complete and checked.
    pub fn install(&self, package: &Path) -> Result<App, String> {
        self.install_checked(package, None)
    }

    /// Installs or updates `id`, an app or a service as `kind` says, from
    /// `package`, and nothing else: a package that holds another ID or kind is
    /// refused once unpacked, before anything installed is replaced or started.
    pub fn install_as(&self, package: &Path, id: &str, kind: Kind) -> Result<App, String> {
        self.install_checked(package, Some((id, kind)))
    }

    fn install_checked(&self, package: &Path, expected: Option<(&str, Kind)>) -> Result<App, String> {
        let _lock = self.lock_waiting()?;
        self.recover();
        let staging = self.apps_dir().join(format!(".install-{}", std::process::id()));
        let _ = fs::remove_dir_all(&staging);
        let result = self.install_staged(package, &staging, expected);
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }

    /// The previous version of `id` in `root`, `apps/` or `services/`, while an update replaces it.
    pub(crate) fn backup_in(&self, root: &Path, id: &str) -> PathBuf {
        root.join(format!(".old-{id}"))
    }

    #[cfg(test)]
    fn backup(&self, id: &str) -> PathBuf {
        self.backup_in(&self.apps_dir(), id)
    }

    /// The library lock, waiting for it.
    pub(crate) fn lock_waiting(&self) -> Result<Lock, String> {
        let apps = self.apps_dir();
        fs::create_dir_all(&apps).map_err(|e| format!("{}: {e}", apps.display()))?;
        self.lock(true).ok_or_else(|| format!("cannot lock {}", apps.display()))
    }

    /// Holds an exclusive lock on `<prefix>/apps/.lock` until dropped
    /// (`flock` on Unix, `LockFileEx` on Windows): installs, removals and
    /// recovery each take it, for services too, so none sees another half
    /// done. Without `wait`, `None` when another process holds it.
    fn lock(&self, wait: bool) -> Option<Lock> {
        let file = File::options().create(true).truncate(false).write(true).open(self.apps_dir().join(".lock")).ok()?;
        let locked = if wait { file.lock().is_ok() } else { file.try_lock().is_ok() };
        locked.then_some(Lock(file))
    }

    /// Recovers unless another process, or this one, holds the library lock.
    pub(crate) fn recover_if_free(&self) {
        if let Some(_lock) = self.lock(false) {
            self.recover();
        }
    }

    /// Finishes what a crash or power loss cut short, with the lock held: a
    /// backup whose app folder is missing goes back in place, one beside a
    /// complete update is deleted, and staging and removal folders, which
    /// only a lock holder uses, are deleted. A service's backup always goes
    /// back (see [`Library::restore_service`]), and the leftovers of an
    /// interrupted service uninstall are deleted.
    pub(crate) fn recover(&self) {
        for root in [self.apps_dir(), self.services_dir()] {
            let Ok(entries) = fs::read_dir(&root) else { continue };
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with(".install-") || name.starts_with(".remove-") {
                    let _ = fs::remove_dir_all(e.path());
                    continue;
                }
                if let Some(id) = name.strip_prefix(".uninstall-").filter(|id| is_valid_id(id)) {
                    let _ = self.finish_uninstall(id, None);
                    continue;
                }
                if let Some(id) = name.strip_prefix(".new-").filter(|id| is_valid_id(id)) {
                    self.drop_unconfirmed(id);
                    continue;
                }
                let Some(id) = name.strip_prefix(".old-").filter(|id| is_valid_id(id)) else { continue };
                if root == self.services_dir() {
                    self.restore_service(id, &e.path());
                    continue;
                }
                let dest = root.join(id);
                if dest.exists() {
                    let _ = fs::remove_dir_all(e.path());
                } else if fs::rename(e.path(), &dest).is_ok() {
                    sync();
                }
            }
        }
        self.sweep_services();
    }

    /// The app `id` for running, with its running lock held shared. The lock
    /// is taken before the app is read, so no install or removal changes it
    /// between reading its manifest and starting it; hand it to the app's
    /// process so it stays held until the app and its children exit.
    pub fn open_for_run(&self, id: &str) -> Result<(App, File), String> {
        if !is_valid_id(id) {
            return Err(format!("{id:?} is not an app ID"));
        }
        if !self.apps_dir().join(id).is_dir() {
            if let Ok(service) = self.find_service(id) {
                return Err(not_an_app(&service.manifest));
            }
        }
        let running = self.hold_running(&self.apps_dir(), id).map_err(|e| format!("cannot mark {id} running: {e}"))?;
        let app = self.get(id)?;
        if app.manifest.kind() != Kind::App {
            return Err(not_an_app(&app.manifest));
        }
        Ok((app, running))
    }

    /// Holds `id`'s running lock in `root` shared, waiting while an update
    /// holds it. An app's lock file stays when it is removed: a run waiting
    /// on it must lock the same file an install will try.
    pub(crate) fn hold_running(&self, root: &Path, id: &str) -> io::Result<File> {
        let file = File::options().create(true).truncate(false).write(true).open(self.running_in(root, id))?;
        file.lock_shared()?;
        Ok(file)
    }

    pub(crate) fn running_in(&self, root: &Path, id: &str) -> PathBuf {
        root.join(format!(".running-{id}"))
    }

    /// Excludes runs of `id` in `root` until dropped, or says it is running:
    /// an update or removal must not change files a running program still reads.
    pub(crate) fn not_running(&self, root: &Path, id: &str) -> Result<File, String> {
        let path = self.running_in(root, id);
        let file = File::options().create(true).truncate(false).write(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        match file.try_lock() {
            Ok(()) => Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => Err(format!("{id} is running; quit it first")),
            Err(std::fs::TryLockError::Error(e)) => Err(format!("{}: {e}", path.display())),
        }
    }

    fn install_staged(&self, package: &Path, staging: &Path, expected: Option<(&str, Kind)>) -> Result<App, String> {
        let manifest = unpack(package, staging)?;
        if let Some((id, kind)) = expected {
            if manifest.id != id {
                return Err(format!("the package holds {}, not {id}", manifest.id));
            }
            if manifest.kind() != kind {
                return Err(format!("{id} is packaged as {}, not {}", kind_word(manifest.kind()), kind_word(kind)));
            }
        }
        let (device, tier) = (device_target(), device_tier());
        if !runs_here(&manifest.target, &device) {
            return Err(format!("{} is built for {}; this device runs {device}", manifest.id, manifest.target));
        }
        if manifest.tier == "full" && tier != "full" {
            return Err(format!("{} needs a full-tier device; this one is {tier}", manifest.id));
        }
        if let Some(problem) = loader_problem(&manifest, staging) {
            return Err(problem);
        }
        if manifest.kind() == Kind::Service {
            return self.install_service(&manifest, staging);
        }
        if self.services_dir().join(&manifest.id).exists() {
            return Err(format!("{} is installed as a service; uninstall it first", manifest.id));
        }
        let _not_running = self.not_running(&self.apps_dir(), &manifest.id)?;
        let dest = self.apps_dir().join(&manifest.id);
        // A fixed name, so the next scan or install restores it if this one is cut short.
        let old = self.backup_in(&self.apps_dir(), &manifest.id);
        let had_old = dest.exists();
        if had_old {
            fs::rename(&dest, &old).map_err(|e| format!("{}: {e}", dest.display()))?;
        }
        if let Err(e) = fs::rename(staging, &dest) {
            if had_old {
                let _ = fs::rename(&old, &dest);
            }
            return Err(format!("{}: {e}", dest.display()));
        }
        sync();
        if had_old {
            let _ = fs::remove_dir_all(&old);
        }
        self.load(&dest)
    }

    /// Removes the app, and its saves only with `saves`; or uninstalls the
    /// service, with everything it keeps. Returns what was removed.
    pub fn remove(&self, id: &str, saves: bool) -> Result<Kind, String> {
        let _lock = self.lock(true);
        // An earlier uninstall that stopped short: recovery finishes it, or this says why it cannot.
        let unfinished = is_valid_id(id) && self.uninstalling(id).exists();
        self.recover();
        if unfinished {
            return self.finish_uninstall(id, None).map(|()| Kind::Service);
        }
        if is_valid_id(id) && !self.apps_dir().join(id).is_dir() && self.services_dir().join(id).is_dir() {
            return self.remove_service(id).map(|()| Kind::Service);
        }
        let app = self.find(id)?;
        let not_running = self.not_running(&self.apps_dir(), id)?;
        let trash = self.apps_dir().join(format!(".remove-{}-{}", id, std::process::id()));
        fs::rename(&app.dir, &trash).map_err(|e| format!("{}: {e}", app.dir.display()))?;
        fs::remove_dir_all(&trash).map_err(|e| format!("{}: {e}", trash.display()))?;
        drop(not_running);
        if saves {
            let dir = self.saves(id);
            if dir.exists() {
                fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
        }
        let mut played = self.played();
        if played.remove(id).is_some() {
            let _ = self.write_played(&played);
        }
        sync();
        Ok(Kind::App)
    }

    pub fn record_played(&self, id: &str) {
        let mut played = self.played();
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        played.insert(id.to_string(), now);
        let _ = self.write_played(&played);
    }

    fn played(&self) -> BTreeMap<String, u64> {
        fs::read_to_string(self.played_path())
            .unwrap_or_default()
            .lines()
            .filter_map(|l| {
                let (id, t) = l.split_once(' ')?;
                Some((id.to_string(), t.trim().parse().ok()?))
            })
            .collect()
    }

    fn write_played(&self, played: &BTreeMap<String, u64>) -> io::Result<()> {
        let path = self.played_path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("new");
        let text: String = played.iter().map(|(id, t)| format!("{id} {t}\n")).collect();
        fs::write(&tmp, text)?;
        fs::rename(&tmp, &path)
    }
}

pub(crate) fn sync() {
    #[cfg(unix)]
    unsafe {
        libc::sync();
    }
}

/// The apps lock; released when dropped.
pub(crate) struct Lock(#[allow(dead_code)] File);

struct Entry {
    name: String,
    method: u16,
    compressed: u64,
    size: u64,
    local_offset: u64,
    dir: bool,
    executable: bool,
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// The central directory of the zip `file`, checked entry by entry.
fn entries(file: &mut File) -> Result<Vec<Entry>, String> {
    let bad = |what: &str| Err(format!("is not a valid package: {what}"));
    let len = file.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
    // The end of central directory record: 22 bytes and a comment of up to 64 KiB.
    let tail_len = len.min(22 + 0xffff);
    let mut tail = vec![0u8; tail_len as usize];
    file.seek(SeekFrom::Start(len - tail_len)).map_err(|e| e.to_string())?;
    file.read_exact(&mut tail).map_err(|e| e.to_string())?;
    let Some(eocd) = (0..tail.len().saturating_sub(21)).rev().find(|&i| u32_at(&tail, i) == 0x0605_4b50) else {
        return bad("no zip directory");
    };
    let e = &tail[eocd..];
    let (disk, cd_disk, count_here, count) = (u16_at(e, 4), u16_at(e, 6), u16_at(e, 8), u16_at(e, 10));
    let (cd_size, cd_offset) = (u32_at(e, 12) as u64, u32_at(e, 16) as u64);
    if disk != 0 || cd_disk != 0 || count_here != count {
        return bad("split archives are not supported");
    }
    if count == 0xffff || cd_size == 0xffff_ffff || cd_offset == 0xffff_ffff {
        return bad("ZIP64 is not supported");
    }
    if count as usize > MAX_ENTRIES {
        return bad("too many files");
    }
    if cd_offset + cd_size > len {
        return bad("a damaged zip directory");
    }
    if cd_size > MAX_DIRECTORY_BYTES {
        return bad("a zip directory larger than 16 MiB");
    }
    let mut cd = vec![0u8; cd_size as usize];
    file.seek(SeekFrom::Start(cd_offset)).map_err(|e| e.to_string())?;
    file.read_exact(&mut cd).map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut p = 0usize;
    for _ in 0..count {
        if p + 46 > cd.len() || u32_at(&cd, p) != 0x0201_4b50 {
            return bad("a damaged zip directory");
        }
        let made_by = u16_at(&cd, p + 4);
        let flags = u16_at(&cd, p + 8);
        let method = u16_at(&cd, p + 10);
        let compressed = u32_at(&cd, p + 20) as u64;
        let size = u32_at(&cd, p + 24) as u64;
        let (name_len, extra_len, comment_len) =
            (u16_at(&cd, p + 28) as usize, u16_at(&cd, p + 30) as usize, u16_at(&cd, p + 32) as usize);
        let external = u32_at(&cd, p + 38);
        let local_offset = u32_at(&cd, p + 42) as u64;
        let end = p + 46 + name_len + extra_len + comment_len;
        if end > cd.len() {
            return bad("a damaged zip directory");
        }
        let raw = &cd[p + 46..p + 46 + name_len];
        let extra = &cd[p + 46 + name_len..p + 46 + name_len + extra_len];
        p = end;

        if flags & 0x41 != 0 {
            return bad("encrypted files");
        }
        if compressed == 0xffff_ffff || size == 0xffff_ffff || local_offset == 0xffff_ffff {
            return bad("ZIP64 is not supported");
        }
        if flags & 0x800 == 0 && !raw.is_ascii() {
            return bad("a file name that is neither ASCII nor UTF-8");
        }
        let Ok(name) = std::str::from_utf8(raw) else {
            return bad("a file name that is not UTF-8");
        };
        let mut x = 0;
        while x + 4 <= extra.len() {
            if u16_at(extra, x) == 0x7075 {
                return bad("a renamed file (Unicode Path extra field)");
            }
            x += 4 + u16_at(extra, x + 2) as usize;
        }
        let dir = name.ends_with('/');
        let path = name.strip_suffix('/').unwrap_or(name);
        if let Some(problem) = name_problem(path) {
            return Err(format!("is not a valid package: {path:?} {problem}"));
        }
        let mode = if made_by >> 8 == 3 { external >> 16 } else { 0 };
        let kind = mode & 0o170000;
        if kind != 0 && kind != 0o100000 && kind != 0o040000 {
            return Err(format!("is not a valid package: {path:?} is a link or a device"));
        }
        if mode & 0o6000 != 0 {
            return Err(format!("is not a valid package: {path:?} is setuid or setgid"));
        }
        if !dir && method != 0 && method != 8 {
            return Err(format!("is not a valid package: {path:?} uses compression method {method}"));
        }
        if !seen.insert(path.to_lowercase()) {
            return Err(format!("is not a valid package: {path:?} appears twice"));
        }
        out.push(Entry {
            name: path.to_string(),
            method,
            compressed,
            size,
            local_offset,
            dir: dir || kind == 0o040000,
            executable: mode & 0o111 != 0,
        });
    }
    // A file may not also be a folder of another entry.
    for e in &out {
        let mut parent = e.name.as_str();
        while let Some((up, _)) = parent.rsplit_once('/') {
            if out.iter().any(|o| !o.dir && o.name.eq_ignore_ascii_case(up)) {
                return Err(format!("is not a valid package: {up:?} is both a file and a folder"));
            }
            parent = up;
        }
    }
    Ok(out)
}

/// Hashes what passes through.
struct Hashing<W> {
    inner: W,
    hash: Sha256,
}

impl<W: Write> Write for Hashing<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hash.update(&buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Extracts `package` into the new folder `dest`, checks every file against
/// `CHECKSUMS` and returns its manifest.
pub fn unpack(package: &Path, dest: &Path) -> Result<Manifest, String> {
    let what = package.display();
    let mut file = File::open(package).map_err(|e| format!("{what}: {e}"))?;
    let list = entries(&mut file).map_err(|e| format!("{what} {e}"))?;
    let total: u64 = list.iter().map(|e| e.size).sum();
    if total > MAX_TOTAL_BYTES {
        return Err(format!("{what} is larger than 8 GiB unpacked"));
    }
    let manifest_entry = list
        .iter()
        .find(|e| e.name == MANIFEST && !e.dir)
        .ok_or_else(|| format!("{what} has no {MANIFEST}"))?;
    if manifest_entry.size > MAX_MANIFEST_BYTES {
        return Err(format!("{what} has a {MANIFEST} larger than 64 KiB"));
    }
    match list.iter().find(|e| e.name == CHECKSUMS && !e.dir) {
        None => return Err(format!("{what} has no CHECKSUMS")),
        Some(e) if e.size > MAX_CHECKSUMS_BYTES => return Err(format!("{what} has a CHECKSUMS larger than 4 MiB")),
        Some(_) => {}
    }

    fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let mut sums = BTreeMap::new();
    for e in &list {
        let path = dest.join(&e.name);
        if e.dir {
            fs::create_dir_all(&path).map_err(|err| format!("{}: {err}", path.display()))?;
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
        }
        // The local header: its own name and extra field lengths lead to the data.
        let mut local = [0u8; 30];
        file.seek(SeekFrom::Start(e.local_offset)).map_err(|err| err.to_string())?;
        file.read_exact(&mut local).map_err(|_| format!("{what} has a damaged entry for {}", e.name))?;
        if u32_at(&local, 0) != 0x0403_4b50 {
            return Err(format!("{what} has a damaged entry for {}", e.name));
        }
        let mut local_name = vec![0u8; u16_at(&local, 26) as usize];
        file.read_exact(&mut local_name).map_err(|_| format!("{what} has a damaged entry for {}", e.name))?;
        let local_name = String::from_utf8_lossy(&local_name);
        if local_name.trim_end_matches('/') != e.name {
            return Err(format!("{what} names {} differently in its local header", e.name));
        }
        file.seek(SeekFrom::Current(u16_at(&local, 28) as i64)).map_err(|err| err.to_string())?;

        let out = File::create(&path).map_err(|err| format!("{}: {err}", path.display()))?;
        let mut out = Hashing { inner: out, hash: Sha256::new() };
        let data = (&mut file).take(e.compressed);
        let written = if e.method == 8 {
            io::copy(&mut flate2::read::DeflateDecoder::new(data).take(e.size + 1), &mut out)
        } else {
            io::copy(&mut data.take(e.size + 1), &mut out)
        }
        .map_err(|err| format!("{what}: {}: {err}", e.name))?;
        if written != e.size {
            return Err(format!("{what}: {} has the wrong size", e.name));
        }
        out.inner.sync_all().map_err(|err| format!("{}: {err}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if e.executable { 0o755 } else { 0o644 };
            fs::set_permissions(&path, fs::Permissions::from_mode(mode))
                .map_err(|err| format!("{}: {err}", path.display()))?;
        }
        sums.insert(e.name.clone(), hex(&out.hash.finalize()));
    }

    let listed = fs::read_to_string(dest.join(CHECKSUMS)).map_err(|e| format!("{what}: CHECKSUMS: {e}"))?;
    sums.remove(CHECKSUMS);
    let mut unchecked = sums.clone();
    for line in listed.lines().filter(|l| !l.trim().is_empty()) {
        let (sum, rel) = line.split_once("  ").ok_or_else(|| format!("{what}: bad CHECKSUMS line {line:?}"))?;
        match unchecked.remove(rel) {
            Some(actual) if actual == sum.to_ascii_lowercase() => {}
            Some(_) => return Err(format!("{what}: {rel} does not match its checksum")),
            None if sums.contains_key(rel) => return Err(format!("{what}: {rel} is listed twice in CHECKSUMS")),
            None => return Err(format!("{what}: {rel} is listed in CHECKSUMS but missing")),
        }
    }
    if let Some(rel) = unchecked.keys().next() {
        return Err(format!("{what}: {rel} is not listed in CHECKSUMS"));
    }

    let text = fs::read_to_string(dest.join(MANIFEST)).map_err(|e| format!("{what}: {MANIFEST}: {e}"))?;
    let manifest = Manifest::parse(&text).map_err(|e| format!("{what}: {e}"))?;
    let entry = dest.join(&manifest.entry);
    if !entry.is_file() {
        return Err(format!("{what} has no entry file {:?}", manifest.entry));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&entry, fs::Permissions::from_mode(0o755)).map_err(|e| format!("{}: {e}", entry.display()))?;
    }
    Ok(manifest)
}

/// Gizlet runtime 1 brings glibc's loader only, so a musl package's entry
/// in `dir` must be static.
fn loader_problem(m: &Manifest, dir: &Path) -> Option<String> {
    (m.target.ends_with("-musl") && needs_loader(&dir.join(&m.entry)))
        .then(|| format!("{} is a dynamically linked musl program; musl packages must be static", m.id))
}

/// Whether `path` is an ELF executable that names a program interpreter, so
/// it needs a C library's loader: Gizlet runtime 1's on Gizlet OS. Scripts and
/// static executables run as they are.
pub fn needs_loader(path: &Path) -> bool {
    let Ok(mut f) = File::open(path) else { return false };
    let mut h = [0u8; 64];
    if f.read_exact(&mut h).is_err() || &h[..4] != b"\x7fELF" || h[4] != 2 || h[5] != 1 {
        return false;
    }
    let u64_at = |b: &[u8], o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    let (phoff, phentsize, phnum) = (u64_at(&h, 32), u16_at(&h, 54) as u64, u16_at(&h, 56) as u64);
    if phentsize < 4 || phnum > 512 {
        return false;
    }
    let mut table = vec![0u8; (phentsize * phnum) as usize];
    if f.seek(SeekFrom::Start(phoff)).is_err() || f.read_exact(&mut table).is_err() {
        return false;
    }
    // PT_INTERP is program header type 3.
    table.chunks(phentsize as usize).any(|ph| u32_at(ph, 0) == 3)
}

/// The command that runs `app` with the Gizlet app environment: its folder as
/// the working directory, `GIZLET_APP_DIR`, `GIZLET_SAVE_DIR`, `GIZLET_LANG`
/// and `GIZLET_SWAP_AB`.
pub fn command(library: &Library, app: &App, lang: &str, swap_ab: bool, args: &[String]) -> io::Result<std::process::Command> {
    let saves = library.saves(app.id());
    fs::create_dir_all(&saves)?;
    let mut cmd = entry_command(library, &app.entry())?;
    cmd.args(args)
        .current_dir(&app.dir)
        .env("GIZLET_APP_ID", app.id())
        .env("GIZLET_APP_DIR", &app.dir)
        .env("GIZLET_SAVE_DIR", &saves)
        .env("GIZLET_LANG", lang)
        .env("GIZLET_SWAP_AB", if swap_ab { "1" } else { "0" });
    Ok(cmd)
}

/// The command that runs a package's entry: through Gizlet runtime 1's
/// `gizlet-run` when it is dynamically linked, as it is otherwise.
pub(crate) fn entry_command(library: &Library, entry: &Path) -> io::Result<std::process::Command> {
    let mut cmd = std::process::Command::new(entry);
    if cfg!(target_os = "linux") && needs_loader(entry) {
        let loader = if cfg!(feature = "gizlet-os") {
            let runtime = Path::new("/opt/gizlet/runtime");
            fs::canonicalize("/run/gizlet-ota-runtime/bin/gizlet-run")
                .or_else(|_| fs::canonicalize(runtime.join("1-current/bin/gizlet-run")))
                .unwrap_or_else(|_| runtime.join("1/bin/gizlet-run"))
        } else {
            library.prefix.join("runtime/1/bin/gizlet-run")
        };
        if loader.is_file() {
            cmd = std::process::Command::new(loader);
            cmd.arg(entry);
        } else if cfg!(feature = "gizlet-os") {
            return Err(io::Error::new(io::ErrorKind::NotFound, "Gizlet runtime 1 is not mounted"));
        }
    }
    Ok(cmd)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minimal zip writer for the tests: stored entries with Unix modes.
    pub(crate) fn zip(files: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let (mut out, mut cd) = (Vec::new(), Vec::new());
        for (name, data, mode) in files {
            let offset = out.len() as u32;
            let crc = 0u32;
            let local = |v: &mut Vec<u8>, sig: u32| {
                v.extend_from_slice(&sig.to_le_bytes());
            };
            local(&mut out, 0x0403_4b50);
            out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);
            local(&mut cd, 0x0201_4b50);
            cd.extend_from_slice(&(0x0314u16).to_le_bytes());
            cd.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            cd.extend_from_slice(&crc.to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
            cd.extend_from_slice(&(mode << 16).to_le_bytes());
            cd.extend_from_slice(&offset.to_le_bytes());
            cd.extend_from_slice(name.as_bytes());
        }
        let cd_offset = out.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    pub(crate) fn sum(data: &[u8]) -> String {
        hex(&Sha256::digest(data))
    }

    pub(crate) fn manifest(id: &str, target: &str) -> String {
        serde_json::json!({
            "schema": 1,
            "id": id,
            "name": "Hello",
            "names": {"zh-CN": "你好"},
            "version": "0.1.0",
            "description": null,
            "descriptions": {"zh-CN": "一个测试应用"},
            "color": "#7b4dff",
            "runtime": 1,
            "target": target,
            "tier": "lite",
            "entry": "bin/hello",
            "icon": "icon.png",
        }).to_string()
    }

    pub(crate) fn package(dir: &Path, manifest: &str, extra: &[(&str, &[u8], u32)]) -> PathBuf {
        let exe: &[u8] = b"#!/bin/sh\necho hello\n";
        let mut files: Vec<(&str, &[u8], u32)> =
            vec![(MANIFEST, manifest.as_bytes(), 0o100644), ("bin/hello", exe, 0o100755)];
        files.extend_from_slice(extra);
        let sums: String = files.iter().map(|(n, d, _)| format!("{}  {n}\n", sum(d))).collect();
        files.push(("CHECKSUMS", sums.as_bytes(), 0o100644));
        let path = dir.join("hello.gzl");
        fs::write(&path, zip(&files)).unwrap();
        path
    }

    pub(crate) fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gizlet-apps-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn install_as_refuses_another_id_or_kind_before_replacing() {
        let dir = temp("expected");
        let lib = Library::new(dir.join("prefix"));
        for d in ["mine", "other"] {
            fs::create_dir_all(dir.join(d)).unwrap();
        }
        let mine = package(&dir.join("mine"), &manifest("dev.example.mine", &device_target()), &[]);
        lib.install(&mine).unwrap();
        let other = package(&dir.join("other"), &manifest("dev.example.other", &device_target()), &[]);
        let err = lib.install_as(&other, "dev.example.mine", Kind::App).unwrap_err();
        assert!(err.contains("holds dev.example.other"), "{err}");
        assert!(!lib.apps_dir().join("dev.example.other").exists());
        let err = lib.install_as(&mine, "dev.example.mine", Kind::Service).unwrap_err();
        assert!(err.contains("packaged as an app, not a service"), "{err}");
        assert_eq!(lib.scan().iter().map(|a| a.id().to_string()).collect::<Vec<_>>(), ["dev.example.mine"]);
        lib.install_as(&mine, "dev.example.mine", Kind::App).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_scan_update_remove() {
        let dir = temp("cycle");
        let lib = Library::new(dir.join("prefix"));
        let pkg = package(&dir, &manifest("dev.example.hello", &device_target()), &[("icon.png", b"icon", 0o100644)]);
        let app = lib.install(&pkg).unwrap();
        assert_eq!(fs::read_to_string(app.entry()).unwrap(), "#!/bin/sh\necho hello\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(app.entry()).unwrap().permissions().mode() & 0o777, 0o755);
        }
        // An update replaces the folder; saves stay.
        fs::create_dir_all(lib.saves("dev.example.hello")).unwrap();
        lib.install(&pkg).unwrap();
        let apps = lib.scan();
        assert_eq!(apps.len(), 1);
        let app = &apps[0];
        assert_eq!(app.manifest.name_for("zh-CN"), "你好");
        assert_eq!(app.manifest.name_for("en"), "Hello");
        assert_eq!(app.manifest.description, None);
        assert_eq!(app.manifest.descriptions["zh-CN"], "一个测试应用");
        assert_eq!(app.manifest.rgb(), Some([0x7b, 0x4d, 0xff]));
        assert_eq!(app.icon(), Some(app.dir.join("icon.png")));
        assert_eq!(leftovers(&lib), 0, "no staging folders are left");
        lib.remove("dev.example.hello", false).unwrap();
        assert!(lib.scan().is_empty());
        assert!(lib.saves("dev.example.hello").is_dir());
        assert!(lib.remove("dev.example.hello", false).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn requires_manifest() {
        let dir = temp("missing-manifest");
        let lib = Library::new(dir.join("prefix"));
        let id = "dev.example.hello";
        let path = dir.join("no-manifest.gzl");
        let exe: &[u8] = b"#!/bin/sh\necho hello\n";
        let sums = format!("{}  bin/hello\n", sum(exe));
        fs::write(&path, zip(&[("bin/hello", exe, 0o100755), (CHECKSUMS, sums.as_bytes(), 0o100644)])).unwrap();
        assert!(lib.install(&path).unwrap_err().contains("has no manifest.json"));

        let app = lib.install(&package(&dir, &manifest(id, &device_target()), &[])).unwrap();
        fs::remove_file(app.dir.join(MANIFEST)).unwrap();
        assert!(lib.get(id).unwrap_err().contains(MANIFEST));
        assert!(lib.scan().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_bad_packages() {
        let dir = temp("bad");
        let lib = Library::new(dir.join("prefix"));
        let target = device_target();
        let err = |m: &str, extra: &[(&str, &[u8], u32)]| lib.install(&package(&dir, m, extra)).unwrap_err();
        assert!(err(&manifest("dev.example.hello", "riscv64gc-unknown-linux-gnu"), &[]).contains("built for"));
        assert!(err(&manifest("Bad", &target), &[]).contains("reverse-DNS"));
        assert!(err("[]", &[]).contains("must be a JSON object"));
        assert!(err(&manifest("dev.example.hello", &target).replace("\"schema\":1", "\"schema\":3"), &[]).contains("unsupported manifest schema 3"));
        assert!(err(&manifest("dev.example.hello", &target).replace("\"schema\":1", "\"schema\":2"), &[]).contains("must name its kind"));
        assert!(err(&manifest("dev.example.hello", &target).replace("\"runtime\":1", "\"runtime\":2"), &[]).contains("unsupported Gizlet runtime"));
        assert!(err(&manifest("dev.example.hello", &target).replace("bin/hello", "../x"), &[]).contains(".."));
        assert!(err(&manifest("dev.example.hello", &target), &[("../evil", b"x", 0o100644)]).contains("evil"));
        assert!(err(&manifest("dev.example.hello", &target), &[("link", b"bin/hello", 0o120777)]).contains("link"));
        assert!(err(&manifest("dev.example.hello", &target), &[("BIN/HELLO", b"x", 0o100644)]).contains("twice"));
        // A file missing from CHECKSUMS, or one that does not match it.
        let exe: &[u8] = b"#!/bin/sh\necho hello\n";
        let m = manifest("dev.example.hello", &target);
        let sums = format!("{}  {MANIFEST}\n", sum(m.as_bytes()));
        let path = dir.join("unlisted.gzl");
        fs::write(&path, zip(&[(MANIFEST, m.as_bytes(), 0o100644), ("bin/hello", exe, 0o100755), ("CHECKSUMS", sums.as_bytes(), 0o100644)])).unwrap();
        assert!(lib.install(&path).unwrap_err().contains("not listed"));
        let sums = format!("{}  {MANIFEST}\n{}  bin/hello\n", sum(m.as_bytes()), sum(b"other"));
        fs::write(&path, zip(&[(MANIFEST, m.as_bytes(), 0o100644), ("bin/hello", exe, 0o100755), ("CHECKSUMS", sums.as_bytes(), 0o100644)])).unwrap();
        assert!(lib.install(&path).unwrap_err().contains("checksum"));
        let huge = vec![b' '; (MAX_CHECKSUMS_BYTES + 1) as usize];
        fs::write(&path, zip(&[(MANIFEST, m.as_bytes(), 0o100644), ("bin/hello", exe, 0o100755), ("CHECKSUMS", &huge, 0o100644)])).unwrap();
        assert!(lib.install(&path).unwrap_err().contains("larger than 4 MiB"));
        assert!(lib.scan().is_empty());
        assert_eq!(leftovers(&lib), 0, "failed installs leave nothing behind");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The test manifest with `schema` (left out when `None`) and `kind` (left out when `None`).
    pub(crate) fn manifest_v(id: &str, schema: Option<u32>, kind: Option<&str>) -> String {
        let mut m: serde_json::Value = serde_json::from_str(&manifest(id, &device_target())).unwrap();
        let o = m.as_object_mut().unwrap();
        o.remove("schema");
        if let Some(schema) = schema {
            o.insert("schema".into(), schema.into());
        }
        if let Some(kind) = kind {
            o.insert("kind".into(), kind.into());
        }
        m.to_string()
    }

    #[test]
    fn manifest_schemas_and_kinds() {
        let id = "dev.example.hello";
        let kind = |schema, k| Manifest::parse(&manifest_v(id, schema, k)).map(|m| m.kind());
        assert_eq!(kind(None, None), Ok(Kind::App), "no schema is schema 1");
        assert_eq!(kind(Some(1), None), Ok(Kind::App));
        assert_eq!(kind(Some(1), Some("app")), Ok(Kind::App));
        assert_eq!(kind(Some(2), Some("app")), Ok(Kind::App));
        assert_eq!(kind(Some(2), Some("service")), Ok(Kind::Service));
        assert!(kind(Some(1), Some("service")).unwrap_err().contains("schema 1 is always an app"));
        assert!(kind(Some(2), None).unwrap_err().contains("must name its kind"));
        assert!(kind(Some(2), Some("rom")).unwrap_err().contains("unsupported package kind \"rom\""));
        assert!(kind(Some(3), Some("app")).unwrap_err().contains("unsupported manifest schema 3"));
    }

    #[test]
    fn installs_and_runs_v1_and_v2_apps() {
        let dir = temp("schemas");
        let lib = Library::new(dir.join("prefix"));
        for (id, schema, kind) in [
            ("dev.example.v1", Some(1), None),
            ("dev.example.noschema", None, None),
            ("dev.example.v2", Some(2), Some("app")),
        ] {
            let app = lib.install(&package(&dir, &manifest_v(id, schema, kind), &[])).unwrap();
            assert_eq!(app.manifest.kind(), Kind::App);
            let (app, running) = lib.open_for_run(id).unwrap();
            assert!(app.entry().is_file());
            drop(running);
        }
        let mut ids: Vec<String> = lib.scan().iter().map(|a| a.id().to_string()).collect();
        ids.sort();
        assert_eq!(ids, ["dev.example.noschema", "dev.example.v1", "dev.example.v2"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn services_are_not_apps() {
        let dir = temp("service");
        let (lib, _units) = crate::services::tests::library(&dir);
        let id = "dev.example.clock";
        let service = manifest_v(id, Some(2), Some("service"));
        // A service installs beside the apps, never among them.
        let installed = lib.install(&package(&dir, &service, &[])).unwrap();
        assert_eq!(installed.manifest.kind(), Kind::Service);
        assert!(!lib.apps_dir().join(id).exists());
        assert!(lib.scan().is_empty());
        assert_eq!(lib.remove(id, false).unwrap(), Kind::Service);
        assert_eq!(leftovers(&lib), 0);
        // One placed among the apps by hand is neither shown on the shelf nor started, but can be removed.
        let placed = lib.apps_dir().join(id);
        fs::create_dir_all(placed.join("bin")).unwrap();
        fs::write(placed.join(MANIFEST), &service).unwrap();
        fs::write(placed.join("bin/hello"), "#!/bin/sh\n").unwrap();
        assert!(lib.scan().is_empty());
        assert!(lib.open_for_run(id).unwrap_err().contains("is a service, not an app"));
        assert_eq!(lib.get(id).unwrap().manifest.kind(), Kind::Service);
        lib.remove(id, false).unwrap();
        assert!(!placed.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Hidden folders under apps/: staging, removals and backups.
    pub(crate) fn leftovers(lib: &Library) -> usize {
        fs::read_dir(lib.apps_dir()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with('.') && e.path().is_dir()).count()
    }

    #[test]
    fn recovery_waits_for_the_lock() {
        let dir = temp("lock");
        let lib = Library::new(dir.join("prefix"));
        let pkg = package(&dir, &manifest("dev.example.hello", &device_target()), &[]);
        lib.install(&pkg).unwrap();
        // An update between its two renames, in another process: the launcher's scan must not undo it.
        let held = lib.lock(true).unwrap();
        fs::rename(lib.apps_dir().join("dev.example.hello"), lib.backup("dev.example.hello")).unwrap();
        fs::create_dir_all(lib.apps_dir().join(".install-1")).unwrap();
        assert!(lib.scan().is_empty());
        assert!(lib.backup("dev.example.hello").exists() && lib.apps_dir().join(".install-1").exists());
        drop(held);
        // Once it is gone, what it left is recovered or reclaimed.
        fs::create_dir_all(lib.apps_dir().join(".remove-dev.example.hello-1")).unwrap();
        // Another test starting a process can hold a copy of the lock's descriptor
        // until that process execs, so give it a moment.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while lib.lock(false).is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(lib.scan().len(), 1);
        assert_eq!(leftovers(&lib), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn interrupted_update_is_recovered() {
        let dir = temp("recover");
        let lib = Library::new(dir.join("prefix"));
        let pkg = package(&dir, &manifest("dev.example.hello", &device_target()), &[]);
        lib.install(&pkg).unwrap();
        // Cut short after the installed copy became the backup.
        fs::rename(lib.apps_dir().join("dev.example.hello"), lib.backup("dev.example.hello")).unwrap();
        assert_eq!(lib.scan().len(), 1);
        assert!(!lib.backup("dev.example.hello").exists());
        // Cut short after the new copy was in place.
        fs::create_dir_all(lib.backup("dev.example.hello")).unwrap();
        lib.install(&pkg).unwrap();
        assert!(!lib.backup("dev.example.hello").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn relative_prefix_is_absolute() {
        let lib = Library::new(PathBuf::from("./data"));
        assert!(lib.apps_dir().is_absolute());
    }

    #[test]
    fn names_stay_inside() {
        for bad in ["C:/Users/x.txt", "c:x", "bin/a.txt:stream", "/abs", "a//b", "a/../b", "a\\b"] {
            assert!(name_problem(bad).is_some(), "{bad}");
        }
        assert!(name_problem("bin/hello").is_none());
    }

    #[test]
    fn rejects_huge_directory() {
        let dir = temp("huge-dir");
        let lib = Library::new(dir.join("prefix"));
        // A sparse file whose end record declares a directory just over the limit.
        let path = dir.join("huge.gzl");
        let cd = MAX_DIRECTORY_BYTES + 1;
        let mut f = File::create(&path).unwrap();
        f.set_len(cd).unwrap();
        f.seek(SeekFrom::End(0)).unwrap();
        let mut eocd = 0x0605_4b50u32.to_le_bytes().to_vec();
        eocd.extend_from_slice(&[0, 0, 0, 0, 1, 0, 1, 0]);
        eocd.extend_from_slice(&(cd as u32).to_le_bytes());
        eocd.extend_from_slice(&0u32.to_le_bytes());
        eocd.extend_from_slice(&[0, 0]);
        f.write_all(&eocd).unwrap();
        drop(f);
        assert!(lib.install(&path).unwrap_err().contains("larger than 16 MiB"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn running_apps_are_not_replaced() {
        let dir = temp("running");
        let lib = Library::new(dir.join("prefix"));
        let pkg = package(&dir, &manifest("dev.example.hello", &device_target()), &[]);
        lib.install(&pkg).unwrap();
        // Starting holds the lock before reading the app, then until it exits.
        let (app, running) = lib.open_for_run("dev.example.hello").unwrap();
        assert!(lib.install(&pkg).unwrap_err().contains("is running"));
        assert!(lib.remove("dev.example.hello", false).unwrap_err().contains("is running"));
        assert!(app.entry().is_file(), "the running copy stays in place");
        assert_eq!(leftovers(&lib), 0);
        drop(running);
        lib.install(&pkg).unwrap();
        lib.remove("dev.example.hello", false).unwrap();
        assert!(lib.open_for_run("dev.example.hello").unwrap_err().contains("not installed"));
        // The lock file outlives removal, so a run already waiting on it locks
        // the file the next install checks.
        let waiting = lib.hold_running(&lib.apps_dir(), "dev.example.hello").unwrap();
        lib.install(&pkg).unwrap_err();
        drop(waiting);
        lib.install(&pkg).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_dynamic_musl() {
        let dir = temp("musl");
        // A minimal 64-bit little-endian ELF header with one PT_INTERP program header.
        let mut elf = vec![0u8; 64 + 56];
        elf[..6].copy_from_slice(b"\x7fELF\x02\x01");
        elf[32..40].copy_from_slice(&64u64.to_le_bytes());
        elf[54..56].copy_from_slice(&56u16.to_le_bytes());
        elf[56..58].copy_from_slice(&1u16.to_le_bytes());
        elf[64..68].copy_from_slice(&3u32.to_le_bytes());
        fs::create_dir_all(dir.join("bin")).unwrap();
        fs::write(dir.join("bin/hello"), &elf).unwrap();
        let musl = Manifest::parse(&manifest("dev.example.hello", "aarch64-unknown-linux-musl")).unwrap();
        let gnu = Manifest::parse(&manifest("dev.example.hello", "aarch64-unknown-linux-gnu")).unwrap();
        assert!(loader_problem(&musl, &dir).unwrap().contains("must be static"));
        assert!(loader_problem(&gnu, &dir).is_none(), "glibc programs run through gizlet-run");
        // A script, like a static program, needs no loader.
        fs::write(dir.join("bin/hello"), b"#!/bin/sh\n").unwrap();
        assert!(loader_problem(&musl, &dir).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn musl_runs_on_gnu() {
        assert!(runs_here("aarch64-unknown-linux-musl", "aarch64-unknown-linux-gnu"));
        assert!(!runs_here("x86_64-unknown-linux-musl", "aarch64-unknown-linux-gnu"));
        assert!(!runs_here("aarch64-unknown-linux-gnu", "aarch64-apple-darwin"));
    }
}
