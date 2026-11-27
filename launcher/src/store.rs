//! The Gizlet Store on the device: browsing gizlet.io's public API, and
//! installing, updating and removing apps from it. Shared by the launcher's
//! Store page and `gizlet store`.
//!
//! Nobody signs in: every request is to the public API. Requests and
//! downloads go through `curl`, like system updates. A package is downloaded
//! beside the library, resumed after a dropped connection, checked against
//! the sha256 the API named, and installed by [`Library::install`]. The
//! repository it came from is recorded in `<prefix>/store/origins.json`, and
//! only that repository, with the same app ID, may update it later.

use crate::apps::{self, App, Kind, Library};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

const API: &str = "https://api.gizlet.io";
const JSON_LIMIT: u64 = 4 * 1024 * 1024;
const IMAGE_LIMIT: u64 = 4 * 1024 * 1024;
const PACKAGE_LIMIT: u64 = 1024 * 1024 * 1024;
/// Store art kept on the card; the least recently used goes first.
const CACHE_LIMIT: u64 = 32 * 1024 * 1024;

/// Why a store request failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// No network, or the server could not be reached.
    Offline,
    /// The API answered with an error: its code and message.
    Api { status: u16, code: String, message: String },
    /// The gizlet is a background service, which the Store does not install yet.
    Service,
    /// The app is installed from another repository or as a file.
    OtherSource { id: String, from: Option<String> },
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Offline => f.write_str("cannot reach gizlet.io; check the network"),
            Error::Api { message, .. } => f.write_str(message),
            Error::Service => f.write_str("services cannot be installed from the Store yet"),
            Error::OtherSource { id, from: Some(repo) } => write!(f, "{id} is installed from {repo}"),
            Error::OtherSource { id, from: None } => write!(f, "{id} is installed from a file; uninstall it first"),
            Error::Other(message) => f.write_str(message),
        }
    }
}

impl Error {
    /// The API's error code, when it answered with one.
    pub fn code(&self) -> Option<&str> {
        match self {
            Error::Api { code, .. } => Some(code),
            _ => None,
        }
    }
}

fn other(e: impl std::fmt::Display) -> Error {
    Error::Other(e.to_string())
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Rating {
    pub average: Option<f32>,
    #[serde(default)]
    pub count: u32,
}

/// A gizlet as lists show it.
#[derive(Clone, Debug, Deserialize)]
pub struct Summary {
    pub repo: String,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub names: BTreeMap<String, String>,
    pub description: Option<String>,
    #[serde(default)]
    pub descriptions: BTreeMap<String, String>,
    #[serde(default = "app_kind")]
    pub kind: String,
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub color: Option<String>,
    pub license: Option<String>,
    pub device_icon_url: Option<String>,
    pub device_cover_url: Option<String>,
    pub version: Option<String>,
    pub updated_at: Option<String>,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub marks: Vec<String>,
    #[serde(default)]
    pub rating: Rating,
    #[serde(default = "listed")]
    pub visibility: String,
}

fn app_kind() -> String {
    "app".into()
}

fn listed() -> String {
    "listed".into()
}

impl Summary {
    pub fn name_for(&self, lang: &str) -> &str {
        localized(&self.names, lang).unwrap_or(&self.name)
    }

    pub fn description_for(&self, lang: &str) -> Option<&str> {
        localized(&self.descriptions, lang).or(self.description.as_deref())
    }

    pub fn is_service(&self) -> bool {
        self.kind == "service"
    }

    /// Installable by link, but not approved for the lists.
    pub fn is_unlisted(&self) -> bool {
        self.visibility != "listed"
    }

    pub fn rgb(&self) -> Option<[u8; 3]> {
        self.color.as_deref().and_then(parse_rgb)
    }
}

fn localized<'a>(map: &'a BTreeMap<String, String>, lang: &str) -> Option<&'a str> {
    let base = lang.split('-').next().unwrap_or(lang);
    map.get(lang).or_else(|| map.iter().find(|(k, _)| k.split('-').next() == Some(base)).map(|(_, v)| v)).map(String::as_str)
}

pub fn parse_rgb(hex: &str) -> Option<[u8; 3]> {
    let hex = hex.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(hex, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

#[derive(Clone, Debug, Deserialize)]
pub struct Control {
    pub button: String,
    pub action: String,
    #[serde(default)]
    pub actions: BTreeMap<String, String>,
}

impl Control {
    pub fn action_for(&self, lang: &str) -> &str {
        localized(&self.actions, lang).unwrap_or(&self.action)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Package {
    pub file: String,
    pub target: String,
    pub tier: String,
    pub size: Option<u64>,
    pub sha256: Option<String>,
    pub url: Option<String>,
}

/// A gizlet's page.
#[derive(Clone, Debug, Deserialize)]
pub struct Detail {
    #[serde(flatten)]
    pub summary: Summary,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub device_screenshots: Vec<String>,
    #[serde(default)]
    pub controls: Vec<Control>,
    #[serde(default)]
    pub packages: Vec<Package>,
}

impl Detail {
    /// The package this device would install from the shown release, if any.
    pub fn package_here(&self) -> Option<&Package> {
        let (target, tier) = (apps::device_target(), apps::device_tier());
        let fits = |p: &&Package| p.target == target && (p.tier == "lite" || tier == "full");
        self.packages.iter().filter(fits).find(|p| p.tier == tier).or_else(|| self.packages.iter().find(fits))
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Section {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub titles: BTreeMap<String, String>,
    #[serde(default)]
    pub layout: String,
    pub gizlets: Vec<Summary>,
}

impl Section {
    pub fn title_for(&self, lang: &str) -> &str {
        localized(&self.titles, lang).unwrap_or(&self.title)
    }
}

/// The Store's home: a hero row and the sections below it.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Featured {
    #[serde(default)]
    pub hero: Vec<Summary>,
    #[serde(default)]
    pub sections: Vec<Section>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Offer {
    pub repo: String,
    /// GitHub's id for the repository, which a later owner of its path never has.
    pub repository_id: Option<u64>,
    pub id: String,
    pub name: String,
    pub kind: String,
    pub version: String,
    pub tag: String,
    pub package: Package,
}

/// What `/v1/updates` says about one installed app.
#[derive(Clone, Debug, Deserialize)]
pub struct UpdateStatus {
    pub repo: String,
    pub repository_id: Option<u64>,
    pub id: String,
    pub installed: String,
    pub status: String,
    pub version: Option<String>,
    pub tag: Option<String>,
    pub kind: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub names: BTreeMap<String, String>,
    pub color: Option<String>,
    pub device_icon_url: Option<String>,
    pub package: Option<Package>,
}

impl UpdateStatus {
    pub fn has_update(&self) -> bool {
        self.status == "update" && self.package.is_some() && self.kind.as_deref().is_none_or(|k| k == "app")
    }

    pub fn name_for(&self, lang: &str) -> &str {
        localized(&self.names, lang).or(self.name.as_deref()).unwrap_or(&self.id)
    }
}

/// Where an installed app came from.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Origin {
    pub repo: String,
    /// GitHub's id for `repo` when installed; updates must come from the same repository.
    #[serde(default)]
    pub repository_id: Option<u64>,
    pub version: String,
    pub tag: String,
    pub sha256: String,
}

/// Where a package comes from, as the API named it.
#[derive(Clone, Copy)]
struct Source<'a> {
    repo: &'a str,
    repository_id: Option<u64>,
    id: &'a str,
    version: &'a str,
    tag: &'a str,
}

/// Progress of an install: downloading (bytes so far, of total), then installing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Downloading(u64, u64),
    Installing,
}

/// The Store of one library: its API and its files under `<prefix>/store`.
pub struct Store {
    api: String,
    dir: PathBuf,
}

impl Store {
    /// The Store for `library`; `GIZLET_STORE_API` points it at another API.
    pub fn new(library: &Library) -> Store {
        let api = std::env::var("GIZLET_STORE_API").ok().filter(|a| !a.is_empty()).unwrap_or_else(|| API.into());
        Store { api: api.trim_end_matches('/').into(), dir: library.prefix().join("store") }
    }

    pub fn with_api(library: &Library, api: &str) -> Store {
        Store { api: api.trim_end_matches('/').into(), dir: library.prefix().join("store") }
    }

    fn query(&self) -> String {
        format!("kind=app&target={}&tier={}", encode(&apps::device_target()), encode(&apps::device_tier()))
    }

    /// The home lists, saved for when the device is offline.
    pub fn featured(&self) -> Result<Featured, Error> {
        let body = get(&format!("{}/v1/featured?{}", self.api, self.query()), JSON_LIMIT)?;
        let featured: Featured = serde_json::from_slice(&body).map_err(other)?;
        let _ = write_atomic(&self.dir.join("featured.json"), &body);
        Ok(featured)
    }

    /// The home lists last fetched.
    pub fn featured_saved(&self) -> Option<Featured> {
        serde_json::from_slice(&fs::read(self.dir.join("featured.json")).ok()?).ok()
    }

    /// Listed apps this device can install that match `q`.
    pub fn search(&self, q: &str) -> Result<Vec<Summary>, Error> {
        #[derive(Deserialize)]
        struct Page {
            gizlets: Vec<Summary>,
        }
        let url = format!("{}/v1/gizlets?{}&sort=score&limit=48&q={}", self.api, self.query(), encode(q.trim()));
        let page: Page = serde_json::from_slice(&get(&url, JSON_LIMIT)?).map_err(other)?;
        Ok(page.gizlets)
    }

    /// The repository a link names: a short link (`rocket`, `gizlet.io/rocket`)
    /// or any way of writing a GitHub repository.
    pub fn resolve(&self, link: &str) -> Result<String, Error> {
        #[derive(Deserialize)]
        struct Resolved {
            repo: String,
        }
        let link = link.trim();
        if link.is_empty() {
            return Err(Error::Other("enter a link".into()));
        }
        let short = !link.contains('/') && !link.contains('.');
        let url = if short { format!("gizlet.io/{}", link.to_ascii_lowercase()) } else { link.to_string() };
        let resolved: Resolved = serde_json::from_slice(&get(&format!("{}/v1/resolve?url={}", self.api, encode(&url)), JSON_LIMIT)?).map_err(other)?;
        Ok(resolved.repo)
    }

    /// A gizlet's page, whatever its visibility but blocked.
    pub fn detail(&self, repo: &str) -> Result<Detail, Error> {
        serde_json::from_slice(&get(&format!("{}/v1/gizlets/{}", self.api, repo_path(repo)?), JSON_LIMIT)?).map_err(other)
    }

    /// The package this device would install.
    pub fn offer(&self, repo: &str) -> Result<Offer, Error> {
        let url = format!("{}/v1/gizlets/{}/install?target={}&tier={}", self.api, repo_path(repo)?, encode(&apps::device_target()), encode(&apps::device_tier()));
        serde_json::from_slice(&get(&url, JSON_LIMIT)?).map_err(other)
    }

    /// Installs or updates the app from `repo`.
    pub fn install(&self, library: &Library, repo: &str, progress: &mut dyn FnMut(Progress)) -> Result<App, Error> {
        let offer = self.offer(repo)?;
        if offer.kind != "app" {
            return Err(Error::Service);
        }
        let source = Source { repo: &offer.repo, repository_id: offer.repository_id, id: &offer.id, version: &offer.version, tag: &offer.tag };
        self.install_package(library, source, &offer.package, progress)
    }

    /// Installs the update `/v1/updates` offered.
    pub fn upgrade(&self, library: &Library, update: &UpdateStatus, progress: &mut dyn FnMut(Progress)) -> Result<App, Error> {
        let (Some(package), Some(version), Some(tag)) = (&update.package, &update.version, &update.tag) else {
            return Err(Error::Other(format!("{} has no update", update.id)));
        };
        if !update.has_update() {
            return Err(Error::Service);
        }
        // The repository it came from, by GitHub's id when the API knows it now and did not then.
        let repository_id = update.repository_id.or_else(|| self.origin(&update.id).and_then(|o| o.repository_id));
        let source = Source { repo: &update.repo, repository_id, id: &update.id, version, tag };
        self.install_package(library, source, package, progress)
    }

    fn install_package(&self, library: &Library, source: Source<'_>, package: &Package, progress: &mut dyn FnMut(Progress)) -> Result<App, Error> {
        let Source { repo, id, .. } = source;
        if !apps::is_valid_id(id) {
            return Err(Error::Other(format!("{id:?} is not an app ID")));
        }
        // An installed app is only replaced by the repository it came from.
        if let Ok(installed) = library.get(id) {
            if installed.manifest.kind() != Kind::App {
                return Err(Error::Service);
            }
            match self.origin(id) {
                Some(origin) if origin.repo != repo => return Err(Error::OtherSource { id: id.into(), from: Some(origin.repo) }),
                None => return Err(Error::OtherSource { id: id.into(), from: None }),
                Some(_) => {}
            }
        }
        let (Some(url), Some(sha256), Some(size)) = (&package.url, &package.sha256, package.size) else {
            return Err(Error::Other("the package is still being checked; try again in a minute".into()));
        };
        if !is_sha256(sha256) || size == 0 || size > PACKAGE_LIMIT {
            return Err(Error::Other("the Store sent an invalid package".into()));
        }
        let file = self.download(url, sha256, size, progress)?;
        progress(Progress::Installing);
        let installed = library.install(&file).map_err(Error::Other);
        let _ = fs::remove_file(&file);
        let app = installed?;
        if app.manifest.id != id || app.manifest.kind() != Kind::App {
            let _ = library.remove(&app.manifest.id, false);
            return Err(Error::Other(format!("the package holds {}, not {id}", app.manifest.id)));
        }
        let origin = Origin { repo: repo.into(), repository_id: source.repository_id, version: source.version.into(), tag: source.tag.into(), sha256: sha256.clone() };
        self.set_origin(id, Some(origin)).map_err(other)?;
        Ok(app)
    }

    /// Downloads a package into `store/downloads`, resuming a partial file
    /// of the same bytes, and checks its size and sha256.
    fn download(&self, url: &str, sha256: &str, size: u64, progress: &mut dyn FnMut(Progress)) -> Result<PathBuf, Error> {
        let dir = self.dir.join("downloads");
        fs::create_dir_all(&dir).map_err(other)?;
        // Other packages' leftovers: a store install runs one download at a time.
        for e in fs::read_dir(&dir).into_iter().flatten().flatten() {
            if !e.file_name().to_string_lossy().starts_with(sha256) {
                let _ = fs::remove_file(e.path());
            }
        }
        let part = dir.join(format!("{sha256}.part"));
        let done = dir.join(format!("{sha256}.gzl"));
        if fs::metadata(&part).is_ok_and(|m| m.len() > size) {
            let _ = fs::remove_file(&part);
        }
        let mut tries = 0;
        loop {
            let have = fs::metadata(&part).map_or(0, |m| m.len());
            if have == size {
                break;
            }
            if have > size || tries >= 5 {
                let _ = fs::remove_file(&part);
                return Err(Error::Other("the download did not match its size; try again".into()));
            }
            tries += 1;
            let mut curl = curl(url);
            curl.args(["-f", "--max-time", "3600", "--speed-limit", "1024", "--speed-time", "30", "--max-filesize", &size.to_string(), "-o"]).arg(&part);
            if have > 0 {
                curl.args(["-C", "-"]);
            }
            curl.arg("--").arg(url);
            let mut child = curl.stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| Error::Other(format!("cannot run curl: {e}")))?;
            loop {
                progress(Progress::Downloading(fs::metadata(&part).map_or(0, |m| m.len()).min(size), size));
                if child.try_wait().map_err(other)?.is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            let output = child.wait_with_output().map_err(other)?;
            if output.status.success() {
                continue;
            }
            let code = output.status.code().unwrap_or(-1);
            eprintln!("gizlet store: {}", String::from_utf8_lossy(&output.stderr).trim());
            // Some bytes arrived: try again from where it stopped, a few times.
            let grew = fs::metadata(&part).map_or(0, |m| m.len()) > have;
            if tries >= 5 || !grew {
                return Err(if offline(code) { Error::Offline } else { Error::Other(format!("download failed (curl {code})")) });
            }
        }
        progress(Progress::Downloading(size, size));
        if sha256_file(&part).map_err(other)? != sha256 {
            let _ = fs::remove_file(&part);
            return Err(Error::Other("the download is damaged (sha256 mismatch); try again".into()));
        }
        fs::rename(&part, &done).map_err(other)?;
        Ok(done)
    }

    /// Removes an app and forgets its origin; its saves stay unless `saves`.
    pub fn uninstall(&self, library: &Library, id: &str, saves: bool) -> Result<(), Error> {
        let app = library.get(id).map_err(Error::Other)?;
        if app.manifest.kind() != Kind::App {
            return Err(Error::Service);
        }
        library.remove(id, saves).map_err(Error::Other)?;
        self.set_origin(id, None).map_err(other)
    }

    /// Updates for the installed apps that came from the Store.
    pub fn updates(&self, library: &Library) -> Result<Vec<UpdateStatus>, Error> {
        let origins = self.origins();
        let installed: Vec<serde_json::Value> = library
            .scan()
            .iter()
            .filter_map(|app| {
                let origin = origins.get(app.id())?;
                Some(serde_json::json!({ "repo": origin.repo, "repository_id": origin.repository_id, "id": app.id(), "version": app.manifest.version }))
            })
            .collect();
        if installed.is_empty() {
            return Ok(Vec::new());
        }
        #[derive(Deserialize)]
        struct Answer {
            gizlets: Vec<UpdateStatus>,
        }
        let body = serde_json::json!({ "target": apps::device_target(), "tier": apps::device_tier(), "installed": installed });
        let answer: Answer = serde_json::from_slice(&post(&format!("{}/v1/updates", self.api), &body.to_string(), JSON_LIMIT)?).map_err(other)?;
        Ok(answer.gizlets)
    }

    /// Origins of the apps installed from the Store, by app ID.
    pub fn origins(&self) -> BTreeMap<String, Origin> {
        fs::read(self.dir.join("origins.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn origin(&self, id: &str) -> Option<Origin> {
        self.origins().remove(id)
    }

    /// Forgets where `id` came from: it was installed from a file or removed.
    pub fn forget(&self, id: &str) {
        if self.origins().contains_key(id) {
            let _ = self.set_origin(id, None);
        }
    }

    fn set_origin(&self, id: &str, origin: Option<Origin>) -> std::io::Result<()> {
        let mut origins = self.origins();
        match origin {
            Some(origin) => origins.insert(id.into(), origin),
            None => origins.remove(id),
        };
        write_atomic(&self.dir.join("origins.json"), &serde_json::to_vec_pretty(&origins).unwrap_or_default())
    }

    /// A PNG of store art on the card, downloading it the first time; the
    /// cache keeps the most recently used up to its limit.
    pub fn image(&self, url: &str) -> Result<PathBuf, Error> {
        let dir = self.dir.join("cache");
        let path = dir.join(format!("{}.png", &sha256_hex(url.as_bytes())[..32]));
        if let Ok(file) = File::options().write(true).open(&path) {
            let _ = file.set_modified(SystemTime::now());
            return Ok(path);
        }
        fs::create_dir_all(&dir).map_err(other)?;
        let tmp = dir.join(format!(".{}-{:?}.tmp", std::process::id(), std::thread::current().id()).replace(['(', ')'], ""));
        let mut curl = curl(url);
        curl.args(["--max-time", "60", "--max-filesize", &IMAGE_LIMIT.to_string(), "-f", "-o"]).arg(&tmp).arg("--").arg(url);
        let output = curl.stdout(Stdio::null()).output().map_err(|e| Error::Other(format!("cannot run curl: {e}")))?;
        if !output.status.success() {
            let _ = fs::remove_file(&tmp);
            let code = output.status.code().unwrap_or(-1);
            return Err(if offline(code) { Error::Offline } else { Error::Other(format!("image download failed (curl {code})")) });
        }
        let png = fs::read(&tmp).is_ok_and(|b| b.starts_with(b"\x89PNG\r\n\x1a\n"));
        if !png {
            let _ = fs::remove_file(&tmp);
            return Err(Error::Other("not a PNG".into()));
        }
        fs::rename(&tmp, &path).map_err(other)?;
        prune(&dir, CACHE_LIMIT);
        Ok(path)
    }
}

/// Deletes the least recently used files until the folder fits in `limit`.
fn prune(dir: &Path, limit: u64) {
    let mut files: Vec<(SystemTime, u64, PathBuf)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".png"))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort();
    for (_, len, path) in files {
        if total <= limit {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

fn repo_path(repo: &str) -> Result<&str, Error> {
    let ok = repo.starts_with("github.com/")
        && repo.split('/').count() == 3
        && repo.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b));
    if ok { Ok(repo) } else { Err(Error::Other(format!("{repo:?} is not a repository"))) }
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Percent-encodes a query parameter.
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// curl's exit codes for a network that is down or unreachable.
fn offline(code: i32) -> bool {
    matches!(code, 5 | 6 | 7 | 28 | 35 | 52 | 55 | 56)
}

/// HTTPS only, unless the API was pointed at a plain HTTP test server.
fn curl(url: &str) -> Command {
    let mut curl = Command::new("curl");
    let proto = if url.starts_with("http://") && std::env::var_os("GIZLET_STORE_API").is_some() { "=http,https" } else { "=https" };
    curl.args(["-q", "-sS", "-L", "--max-redirs", "3", "--proto", proto, "--proto-redir", proto, "--connect-timeout", "10", "-A", "gizlet-launcher"]);
    curl
}

fn get(url: &str, limit: u64) -> Result<Vec<u8>, Error> {
    request(url, curl(url), None, limit)
}

fn post(url: &str, body: &str, limit: u64) -> Result<Vec<u8>, Error> {
    let mut c = curl(url);
    c.args(["-H", "content-type: application/json", "--data-binary", "@-"]);
    request(url, c, Some(body), limit)
}

/// Runs a JSON request; the status code follows the body on its own line.
fn request(url: &str, mut curl: Command, body: Option<&str>, limit: u64) -> Result<Vec<u8>, Error> {
    curl.args(["--max-time", "30", "--max-filesize", &limit.to_string(), "-H", "accept: application/json", "-w", "\n%{http_code}", "--", url]);
    let mut child = curl
        .stdin(if body.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Other(format!("cannot run curl: {e}")))?;
    if let (Some(body), Some(mut stdin)) = (body, child.stdin.take()) {
        stdin.write_all(body.as_bytes()).map_err(other)?;
    }
    let output = child.wait_with_output().map_err(other)?;
    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        eprintln!("gizlet store: {}", String::from_utf8_lossy(&output.stderr).trim());
        return Err(if offline(code) { Error::Offline } else { Error::Other(format!("request failed (curl {code})")) });
    }
    let mut out = output.stdout;
    let split = out.iter().rposition(|&b| b == b'\n').ok_or_else(|| Error::Other("no answer".into()))?;
    let status: u16 = std::str::from_utf8(&out[split + 1..]).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    out.truncate(split);
    if (200..300).contains(&status) {
        return Ok(out);
    }
    #[derive(Deserialize)]
    struct Problem {
        code: String,
        message: String,
    }
    #[derive(Deserialize)]
    struct Wrapped {
        error: Problem,
    }
    match serde_json::from_slice::<Wrapped>(&out) {
        Ok(w) => Err(Error::Api { status, code: w.error.code, message: w.error.message }),
        Err(_) if status >= 500 || status == 0 => Err(Error::Offline),
        Err(_) => Err(Error::Api { status, code: "http".into(), message: format!("gizlet.io answered {status}") }),
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&tmp, path)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut block = vec![0u8; 128 * 1024];
    loop {
        let n = file.read(&mut block)?;
        if n == 0 {
            break;
        }
        digest.update(&block[..n]);
    }
    Ok(digest.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_queries() {
        assert_eq!(encode("rocket snail/火"), "rocket%20snail%2F%E7%81%AB");
        assert_eq!(encode("a-b_c.d~"), "a-b_c.d~");
    }

    #[test]
    fn accepts_only_repositories() {
        assert!(repo_path("github.com/gizlet/overeaten").is_ok());
        assert!(repo_path("github.com/gizlet/../x").is_err());
        assert!(repo_path("github.com/gizlet").is_err());
        assert!(repo_path("github.com/a/b?x=1").is_err());
    }

    #[test]
    fn picks_localized_names() {
        let names = BTreeMap::from([("zh-CN".to_string(), "吃吃吃".to_string())]);
        assert_eq!(localized(&names, "zh-CN"), Some("吃吃吃"));
        assert_eq!(localized(&names, "zh-TW"), Some("吃吃吃"));
        assert_eq!(localized(&names, "en"), None);
    }

    #[test]
    fn reads_api_summaries() {
        let s: Summary = serde_json::from_str(r##"{"repo":"github.com/a/b","id":"dev.a.b","name":"B","names":{"zh-CN":"乙"},"description":null,"kind":"app","category":"game","color":"#c8553d","device_icon_url":"https://dl.gizlet.io/x/~device/icon.png","rating":{"average":4.5,"count":2},"visibility":"unlisted","extra":1}"##).unwrap();
        assert_eq!(s.name_for("zh-CN"), "乙");
        assert_eq!(s.rgb(), Some([0xc8, 0x55, 0x3d]));
        assert!(s.is_unlisted());
        assert!(!s.is_service());
    }

    #[test]
    fn prunes_the_least_recently_used() {
        let dir = std::env::temp_dir().join(format!("gizlet-store-prune-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for (i, name) in ["old.png", "mid.png", "new.png"].iter().enumerate() {
            let path = dir.join(name);
            fs::write(&path, vec![0u8; 100]).unwrap();
            File::options().write(true).open(&path).unwrap().set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1000 + i as u64)).unwrap();
        }
        prune(&dir, 200);
        assert!(!dir.join("old.png").exists());
        assert!(dir.join("mid.png").exists() && dir.join("new.png").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn origins_survive_and_guard_updates() {
        let prefix = std::env::temp_dir().join(format!("gizlet-store-origin-{}", std::process::id()));
        let _ = fs::remove_dir_all(&prefix);
        let library = Library::new(prefix.clone());
        let store = Store::with_api(&library, "http://127.0.0.1:9");
        let origin = Origin { repo: "github.com/a/b".into(), repository_id: Some(42), version: "1.0.0".into(), tag: "v1.0.0".into(), sha256: "0".repeat(64) };
        store.set_origin("dev.a.b", Some(origin.clone())).unwrap();
        assert_eq!(Store::with_api(&library, API).origin("dev.a.b"), Some(origin));
        store.set_origin("dev.a.b", None).unwrap();
        assert!(store.origins().is_empty());
        let _ = fs::remove_dir_all(&prefix);
    }
}
