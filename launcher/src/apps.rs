//! Installed apps: `.gzl` packages and the folders they install into.
//!
//! A `.gzl` is the package of the Gizlet store contract: a zip with
//! `manifest.toml` at its root and `CHECKSUMS`, the SHA-256 of every other file
//! (`<64 hex>  <path>` per line). Installing extracts it into
//! `<prefix>/apps/<id>/`, where the prefix is `/opt/gizlet` on Gizlet OS. Each
//! app keeps its own files in `<prefix>/saves/<id>/`, which outlive updates.
//!
//! The reader accepts only what the store accepts: stored or deflated entries
//! with canonical relative names, no symlinks, devices, encryption or ZIP64,
//! and no two entries landing on the same path.

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const MANIFEST: &str = "manifest.toml";
pub const CHECKSUMS: &str = "CHECKSUMS";
const MAX_ENTRIES: usize = 20_000;
const MAX_TOTAL_BYTES: u64 = 8 << 30;
const MAX_MANIFEST_BYTES: u64 = 64 << 10;
/// Asks the launcher on the screen to start an app: it writes the app's ID here
/// and quits, and its start script runs the app, then the launcher again.
pub const LAUNCH_REQUEST: &str = "/run/gizlet-launch";
/// The running launcher's process ID, for `gizlet start`.
pub const LAUNCHER_PID: &str = "/run/gizlet-launcher.pid";

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

#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub tier: String,
    pub target: String,
    pub entry: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub description: Option<String>,
    pub names: BTreeMap<String, String>,
    pub descriptions: BTreeMap<String, String>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, String> {
        let table: toml::Table = text.parse().map_err(|e: toml::de::Error| {
            format!("manifest.toml is not valid TOML: {}", e.message())
        })?;
        let string = |key: &str| -> Result<Option<String>, String> {
            match table.get(key) {
                None => Ok(None),
                Some(toml::Value::String(s)) => Ok(Some(s.clone())),
                Some(_) => Err(format!("manifest.toml: {key} must be a string")),
            }
        };
        let required = |key: &str| -> Result<String, String> {
            string(key)?.filter(|s| !s.trim().is_empty()).ok_or_else(|| format!("manifest.toml has no {key}"))
        };
        let map = |key: &str| -> Result<BTreeMap<String, String>, String> {
            match table.get(key) {
                None => Ok(BTreeMap::new()),
                Some(toml::Value::Table(t)) => t
                    .iter()
                    .map(|(k, v)| match v {
                        toml::Value::String(s) => Ok((k.clone(), s.clone())),
                        _ => Err(format!("manifest.toml: {key}.{k} must be a string")),
                    })
                    .collect(),
                Some(_) => Err(format!("manifest.toml: {key} must be a table")),
            }
        };
        let m = Manifest {
            id: required("id")?,
            name: required("name")?,
            version: required("version")?,
            tier: required("tier")?,
            target: required("target")?,
            entry: required("entry")?,
            icon: string("icon")?,
            color: string("color")?,
            description: string("description")?,
            names: map("names")?,
            descriptions: map("descriptions")?,
        };
        if !is_valid_id(&m.id) {
            return Err(format!(
                "id {:?} must be reverse-DNS: lowercase letters, digits, '-' and '_' in dot-separated parts",
                m.id
            ));
        }
        if !matches!(m.tier.as_str(), "lite" | "full") {
            return Err(format!("tier {:?} must be lite or full", m.tier));
        }
        if m.target.split('-').count() < 3 {
            return Err(format!("target {:?} is not a target triple", m.target));
        }
        for (field, path) in [("entry", Some(&m.entry)), ("icon", m.icon.as_ref())] {
            if let Some(path) = path {
                if let Some(problem) = name_problem(path) {
                    return Err(format!("{field} {path:?} {problem}"));
                }
            }
        }
        Ok(m)
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

/// An installed app.
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

/// The apps under `<prefix>/apps`.
pub struct Library {
    prefix: PathBuf,
}

impl Library {
    /// The prefix is made absolute, since apps run in their own folder.
    pub fn new(prefix: PathBuf) -> Library {
        let prefix = std::path::absolute(&prefix).unwrap_or(prefix);
        Library { prefix }
    }

    pub fn open() -> Library {
        Library::new(prefix())
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
    /// Folders without a valid manifest are skipped.
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
            .collect();
        apps.sort_by(|a, b| {
            let pa = played.get(a.id()).copied().unwrap_or(0);
            let pb = played.get(b.id()).copied().unwrap_or(0);
            pb.cmp(&pa).then(b.installed.cmp(&a.installed)).then(a.id().cmp(b.id()))
        });
        apps
    }

    fn load(&self, dir: &Path) -> Result<App, String> {
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

    /// The installed app `id`, without recovery or the lock.
    fn find(&self, id: &str) -> Result<App, String> {
        if !is_valid_id(id) {
            return Err(format!("{id:?} is not an app ID"));
        }
        let dir = self.apps_dir().join(id);
        if !dir.is_dir() {
            return Err(format!("{id} is not installed"));
        }
        self.load(&dir)
    }

    /// Installs or updates the app in `package`, a `.gzl`. The previous
    /// version stays in place until the new one is complete and checked.
    pub fn install(&self, package: &Path) -> Result<App, String> {
        let apps = self.apps_dir();
        fs::create_dir_all(&apps).map_err(|e| format!("{}: {e}", apps.display()))?;
        let _lock = self.lock(true).ok_or_else(|| format!("cannot lock {}", apps.display()))?;
        self.recover();
        let staging = apps.join(format!(".install-{}", std::process::id()));
        let _ = fs::remove_dir_all(&staging);
        let result = self.install_staged(package, &staging);
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }

    /// The previous version of `id` while an update replaces it.
    fn backup(&self, id: &str) -> PathBuf {
        self.apps_dir().join(format!(".old-{id}"))
    }

    /// Holds `<prefix>/apps/.lock` until dropped: installs, removals and
    /// recovery each take it, so none sees another half done. Without
    /// `wait`, `None` when another process holds it.
    fn lock(&self, wait: bool) -> Option<Lock> {
        let file = File::options().create(true).truncate(false).write(true).open(self.apps_dir().join(".lock")).ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let op = libc::LOCK_EX | if wait { 0 } else { libc::LOCK_NB };
            while unsafe { libc::flock(file.as_raw_fd(), op) } != 0 {
                if !wait || io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                    return None;
                }
            }
        }
        Some(Lock(file))
    }

    /// Finishes what a crash or power loss cut short, with the lock held: a
    /// backup whose app folder is missing goes back in place, one beside a
    /// complete update is deleted, and staging and removal folders, which
    /// only a lock holder uses, are deleted.
    fn recover(&self) {
        let Ok(entries) = fs::read_dir(self.apps_dir()) else { return };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with(".install-") || name.starts_with(".remove-") {
                let _ = fs::remove_dir_all(e.path());
                continue;
            }
            let Some(id) = name.strip_prefix(".old-").filter(|id| is_valid_id(id)) else { continue };
            let dest = self.apps_dir().join(id);
            if dest.exists() {
                let _ = fs::remove_dir_all(e.path());
            } else if fs::rename(e.path(), &dest).is_ok() {
                sync();
            }
        }
    }

    fn install_staged(&self, package: &Path, staging: &Path) -> Result<App, String> {
        let manifest = unpack(package, staging)?;
        let (device, tier) = (device_target(), device_tier());
        if !runs_here(&manifest.target, &device) {
            return Err(format!("{} is built for {}; this device runs {device}", manifest.id, manifest.target));
        }
        if manifest.tier == "full" && tier != "full" {
            return Err(format!("{} needs a full-tier device; this one is {tier}", manifest.id));
        }
        let dest = self.apps_dir().join(&manifest.id);
        // A fixed name, so the next scan or install restores it if this one is cut short.
        let old = self.backup(&manifest.id);
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

    /// Removes the app; its saves only with `saves`.
    pub fn remove(&self, id: &str, saves: bool) -> Result<(), String> {
        let _lock = self.lock(true);
        self.recover();
        let app = self.find(id)?;
        let trash = self.apps_dir().join(format!(".remove-{}-{}", id, std::process::id()));
        fs::rename(&app.dir, &trash).map_err(|e| format!("{}: {e}", app.dir.display()))?;
        fs::remove_dir_all(&trash).map_err(|e| format!("{}: {e}", trash.display()))?;
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
        Ok(())
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

fn sync() {
    #[cfg(unix)]
    unsafe {
        libc::sync();
    }
}

/// The apps lock; released when dropped.
struct Lock(#[allow(dead_code)] File);

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
    let manifest_entry = list.iter().find(|e| e.name == MANIFEST && !e.dir);
    match manifest_entry {
        None => return Err(format!("{what} has no manifest.toml")),
        Some(e) if e.size > MAX_MANIFEST_BYTES => return Err(format!("{what} has a manifest.toml larger than 64 KiB")),
        Some(_) => {}
    }
    if !list.iter().any(|e| e.name == CHECKSUMS && !e.dir) {
        return Err(format!("{what} has no CHECKSUMS"));
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

    let text = fs::read_to_string(dest.join(MANIFEST)).map_err(|e| format!("{what}: manifest.toml: {e}"))?;
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
    let entry = app.entry();
    let loader = library.prefix.join("runtime/1/bin/gizlet-run");
    let mut cmd = if cfg!(target_os = "linux") && needs_loader(&entry) && loader.is_file() {
        let mut c = std::process::Command::new(loader);
        c.arg(&entry);
        c
    } else {
        std::process::Command::new(&entry)
    };
    cmd.args(args)
        .current_dir(&app.dir)
        .env("GIZLET_APP_ID", app.id())
        .env("GIZLET_APP_DIR", &app.dir)
        .env("GIZLET_SAVE_DIR", &saves)
        .env("GIZLET_LANG", lang)
        .env("GIZLET_SWAP_AB", if swap_ab { "1" } else { "0" });
    Ok(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal zip writer for the tests: stored entries with Unix modes.
    fn zip(files: &[(&str, &[u8], u32)]) -> Vec<u8> {
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

    fn sum(data: &[u8]) -> String {
        hex(&Sha256::digest(data))
    }

    fn manifest(id: &str, target: &str) -> String {
        format!(
            "id = \"{id}\"\nname = \"Hello\"\nversion = \"0.1.0\"\ntier = \"lite\"\ntarget = \"{target}\"\nentry = \"bin/hello\"\n\n[names]\nzh-CN = \"你好\"\n"
        )
    }

    fn package(dir: &Path, manifest: &str, extra: &[(&str, &[u8], u32)]) -> PathBuf {
        let exe: &[u8] = b"#!/bin/sh\necho hello\n";
        let mut files: Vec<(&str, &[u8], u32)> =
            vec![("manifest.toml", manifest.as_bytes(), 0o100644), ("bin/hello", exe, 0o100755)];
        files.extend_from_slice(extra);
        let sums: String = files.iter().map(|(n, d, _)| format!("{}  {n}\n", sum(d))).collect();
        files.push(("CHECKSUMS", sums.as_bytes(), 0o100644));
        let path = dir.join("hello.gzl");
        fs::write(&path, zip(&files)).unwrap();
        path
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gizlet-apps-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn install_scan_update_remove() {
        let dir = temp("cycle");
        let lib = Library::new(dir.join("prefix"));
        let pkg = package(&dir, &manifest("dev.example.hello", &device_target()), &[]);
        let app = lib.install(&pkg).unwrap();
        assert_eq!(app.manifest.name_for("zh-CN"), "你好");
        assert_eq!(app.manifest.name_for("en"), "Hello");
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
        assert_eq!(leftovers(&lib), 0, "no staging folders are left");
        lib.remove("dev.example.hello", false).unwrap();
        assert!(lib.scan().is_empty());
        assert!(lib.saves("dev.example.hello").is_dir());
        assert!(lib.remove("dev.example.hello", false).is_err());
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
        assert!(err(&manifest("dev.example.hello", &target).replace("bin/hello", "../x"), &[]).contains(".."));
        assert!(err(&manifest("dev.example.hello", &target), &[("../evil", b"x", 0o100644)]).contains("evil"));
        assert!(err(&manifest("dev.example.hello", &target), &[("link", b"bin/hello", 0o120777)]).contains("link"));
        assert!(err(&manifest("dev.example.hello", &target), &[("BIN/HELLO", b"x", 0o100644)]).contains("twice"));
        // A file missing from CHECKSUMS, or one that does not match it.
        let exe: &[u8] = b"#!/bin/sh\necho hello\n";
        let m = manifest("dev.example.hello", &target);
        let sums = format!("{}  manifest.toml\n", sum(m.as_bytes()));
        let path = dir.join("unlisted.gzl");
        fs::write(&path, zip(&[("manifest.toml", m.as_bytes(), 0o100644), ("bin/hello", exe, 0o100755), ("CHECKSUMS", sums.as_bytes(), 0o100644)])).unwrap();
        assert!(lib.install(&path).unwrap_err().contains("not listed"));
        let sums = format!("{}  manifest.toml\n{}  bin/hello\n", sum(m.as_bytes()), sum(b"other"));
        fs::write(&path, zip(&[("manifest.toml", m.as_bytes(), 0o100644), ("bin/hello", exe, 0o100755), ("CHECKSUMS", sums.as_bytes(), 0o100644)])).unwrap();
        assert!(lib.install(&path).unwrap_err().contains("checksum"));
        assert!(lib.scan().is_empty());
        assert_eq!(leftovers(&lib), 0, "failed installs leave nothing behind");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Hidden folders under apps/: staging, removals and backups.
    fn leftovers(lib: &Library) -> usize {
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
    fn musl_runs_on_gnu() {
        assert!(runs_here("aarch64-unknown-linux-musl", "aarch64-unknown-linux-gnu"));
        assert!(!runs_here("x86_64-unknown-linux-musl", "aarch64-unknown-linux-gnu"));
        assert!(!runs_here("aarch64-unknown-linux-gnu", "aarch64-apple-darwin"));
    }
}
