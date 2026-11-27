use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

const ORIGIN: &str = "https://dl.gizlet.io/releases";
const STORE: &str = "/opt/gizlet/ota";
const SIGNERS: &str = "/usr/lib/gizlet/launcher/allowed_signers";
const RUNTIME_LIMIT: u64 = 512 * 1024 * 1024;
const LAUNCHER_LIMIT: u64 = 64 * 1024 * 1024;

fn factory_version() -> Option<&'static str> {
    option_env!("GIZLET_RELEASE_VERSION").filter(|text| version(text).is_some())
}

#[derive(Clone, Debug, Deserialize)]
struct Artifact {
    path: String,
    sha256: String,
    bytes: u64,
}

#[derive(Debug, Deserialize)]
struct Metadata {
    protocol: u32,
    arch: String,
    runtime_abi: u32,
    runtime: Artifact,
    launcher: Artifact,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    schema_version: u32,
    tag: String,
    ota: Option<Metadata>,
}

#[derive(Clone, Debug)]
pub struct Offer {
    pub tag: String,
    pub runtime_changed: bool,
    pub launcher_changed: bool,
    runtime: Artifact,
    launcher: Artifact,
}

impl Offer {
    pub fn bytes(&self) -> u64 {
        self.launcher.bytes + if self.runtime_changed { self.runtime.bytes } else { 0 }
    }
}

#[derive(Clone, Debug)]
pub struct Installed {
    pub launcher: String,
    pub runtime: Option<String>,
    pub runtime_tag: Option<String>,
    pub pending: Option<String>,
    pub failed: Option<String>,
}

impl Installed {
    pub fn launcher_tag(&self) -> Option<String> {
        tag_from_version(&self.launcher)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Progress {
    Runtime(u8),
    Launcher(u8),
    Verify,
}

fn version(text: &str) -> Option<[u32; 3]> {
    let mut fields = text.split('.');
    let mut parts = [0; 3];
    for part in &mut parts {
        let field = fields.next()?;
        if field.len() > 9 || (field.len() > 1 && field.starts_with('0')) || !field.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        *part = field.parse().ok()?;
    }
    fields.next().is_none().then_some(parts)
}

fn valid_tag(tag: &str) -> bool {
    tag.strip_prefix('v').and_then(version).is_some()
}

fn tag_from_version(release: &str) -> Option<String> {
    version(release).map(|_| format!("v{release}"))
}

fn launcher_bundle_version(root: &Path) -> Option<String> {
    let manifest = fs::read_to_string(root.join("manifest")).ok()?;
    let mut fields = manifest.split_whitespace();
    if fields.next()? != "gizlet-launcher-1" {
        return None;
    }
    let release = fields.next()?;
    version(release).map(|_| release.to_owned())
}

pub fn launcher_release_tag() -> Option<String> {
    let bundle_version = |exe: &Path| {
        let exe = exe.canonicalize().ok()?;
        launcher_bundle_version(exe.parent()?.parent()?)
    };
    std::env::args_os().next()
        .and_then(|exe| bundle_version(Path::new(&exe)))
        .or_else(|| std::env::current_exe().ok().and_then(|exe| bundle_version(&exe)))
        .and_then(|release| tag_from_version(&release))
        .or_else(|| factory_version().and_then(tag_from_version))
}

fn runtime_release_tag(root: &Path) -> Option<String> {
    let tag = fs::read_to_string(root.join("release-tag")).ok()?;
    let tag = tag.trim();
    valid_tag(tag).then(|| tag.to_owned())
}

fn pointer(name: &str) -> Option<String> {
    let target = fs::read_link(Path::new(STORE).join(name)).ok()?;
    let generation = target.to_str()?;
    valid_generation(generation).then(|| generation.to_owned())
}

fn valid_generation(generation: &str) -> bool {
    generation.rsplit_once('.').is_some_and(|(tag, id)| {
        valid_tag(tag) && id.len() == 6 && id.bytes().all(|byte| byte.is_ascii_alphanumeric())
    })
}

fn hash(path: &Path) -> io::Result<String> {
    use std::fmt::Write;

    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut block = vec![0u8; 128 * 1024];
    loop {
        let size = file.read(&mut block)?;
        if size == 0 {
            break;
        }
        digest.update(&block[..size]);
    }
    let mut hex = String::with_capacity(64);
    for byte in digest.finalize() {
        write!(hex, "{byte:02x}").unwrap();
    }
    Ok(hex)
}

fn runtime_root() -> PathBuf {
    if let Some(path) = std::env::var_os("GIZLET_RUNNING_RUNTIME") {
        return PathBuf::from(path);
    }
    ["/run/gizlet-ota-runtime", "/opt/gizlet/runtime/1-current", "/opt/gizlet/runtime/1"]
        .into_iter()
        .find_map(|path| {
            let root = Path::new(path);
            root.join("bin/gizlet-run").is_file().then(|| fs::canonicalize(root).ok()).flatten()
        })
        .unwrap_or_else(|| PathBuf::from("/opt/gizlet/runtime/1"))
}

pub fn installed() -> Installed {
    let running = std::env::var("GIZLET_RUNNING_LAUNCHER").ok();
    let active = launcher_bundle_version(Path::new("/run/gizlet-ota-launcher"));
    let selected = fs::read_to_string("/run/gizlet-launcher-release").ok().map(|text| text.trim().to_owned());
    let launcher = running.or(active).or(selected).filter(|text| version(text).is_some())
        .or_else(|| factory_version().map(str::to_owned))
        .unwrap_or_else(|| "factory".into());
    let failed = fs::read_to_string(Path::new(STORE).join("failed")).ok()
        .map(|text| text.trim().to_owned()).filter(|tag| valid_tag(tag));
    let pending = pointer("pending").and_then(|generation| generation.rsplit_once('.').map(|(tag, _)| tag.to_owned()));
    let root = runtime_root();
    Installed {
        launcher,
        runtime: hash(&runtime_image_path(&root)).ok(),
        runtime_tag: runtime_release_tag(&root),
        pending,
        failed,
    }
}

fn run(command: &mut Command, error: &str) -> Result<(), String> {
    if !command.status().map_err(|err| format!("{error}: {err}"))?.success() {
        return Err(error.into());
    }
    Ok(())
}

struct Temp(PathBuf);

impl Temp {
    fn new(template: &Path) -> Result<Self, String> {
        let output = Command::new("mktemp").arg("-d").arg(template).output().map_err(error)?;
        if !output.status.success() {
            return Err("Cannot create update staging directory".into());
        }
        let path = String::from_utf8(output.stdout).map_err(error)?;
        Ok(Self(PathBuf::from(path.trim_end_matches('\n'))))
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn error(err: impl std::fmt::Display) -> String {
    err.to_string()
}

fn download(
    url: &str,
    destination: &Path,
    max_bytes: u64,
    redirect: bool,
    progress: Option<&mut dyn FnMut(u8)>,
) -> Result<String, String> {
    let mut curl = Command::new("curl");
    curl.args(["-q", "-fsS", "--proto", "=https", "--proto-redir", "=https", "--connect-timeout", "15",
        "--max-time", if max_bytes <= 64 * 1024 { "30" } else { "1200" }, "--speed-limit", "1024", "--speed-time", "30",
        "--max-filesize", &max_bytes.to_string(), "--write-out", "%{url_effective}"]);
    if redirect {
        curl.args(["--location", "--max-redirs", "1"]);
    }
    curl.arg("--output").arg(destination).arg("--").arg(url);
    let output = if let Some(progress) = progress {
        let mut child = curl.stdout(std::process::Stdio::piped()).spawn().map_err(error)?;
        let mut last = 0;
        while child.try_wait().map_err(error)?.is_none() {
            let bytes = fs::metadata(destination).map_or(0, |metadata| metadata.len());
            let percent = (bytes.min(max_bytes) * 100 / max_bytes) as u8;
            if percent != last {
                progress(percent);
                last = percent;
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        child.wait_with_output().map_err(error)?
    } else {
        curl.output().map_err(error)?
    };
    if !output.status.success() {
        eprintln!("Gizlet OTA: {}", String::from_utf8_lossy(&output.stderr).trim());
        return Err("Download failed".into());
    }
    String::from_utf8(output.stdout).map_err(error)
}

fn verify_manifest(manifest: &Path, signature: &Path) -> Result<(), String> {
    run(
        Command::new("ssh-keygen")
            .args(["-Y", "verify", "-f", SIGNERS, "-I", "launcher", "-n", "gizlet-launcher", "-s"])
            .arg(signature)
            .stdin(File::open(manifest).map_err(error)?)
            .stdout(std::process::Stdio::null()),
        "Release signature verification failed",
    )
}

fn artifact_valid(artifact: &Artifact, path: &str, limit: u64) -> bool {
    artifact.path == path && (1..=limit).contains(&artifact.bytes) && artifact.sha256.len() == 64
        && artifact.sha256.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn offer(manifest: Manifest, tag: &str, current: &Installed) -> Result<Option<Offer>, String> {
    if manifest.schema_version != 1 || manifest.tag != tag || !valid_tag(tag) {
        return Err("Unexpected release manifest".into());
    }
    let Some(ota) = manifest.ota else { return Ok(None) };
    let launcher_version = &tag[1..];
    if ota.protocol != 1 || ota.arch != "aarch64" || ota.runtime_abi != 1 ||
        !artifact_valid(&ota.runtime, "runtime/gizlet-runtime-1.sqfs", RUNTIME_LIMIT) ||
        !artifact_valid(&ota.launcher, &format!("launcher/gizlet-launcher-{launcher_version}.gizlet"), LAUNCHER_LIMIT)
    {
        return Err("Release does not support this device or OTA protocol".into());
    }
    let installed_version = version(&current.launcher);
    let candidate_version = version(launcher_version);
    if candidate_version < installed_version {
        return Ok(None);
    }
    let runtime_changed = current.runtime.as_ref() != Some(&ota.runtime.sha256);
    let launcher_changed = candidate_version > installed_version;
    Ok((runtime_changed || launcher_changed).then(|| Offer {
        tag: tag.into(),
        runtime_changed,
        launcher_changed,
        runtime: ota.runtime,
        launcher: ota.launcher,
    }))
}

pub fn check(current: &Installed) -> Result<Option<Offer>, String> {
    let work = Temp::new(Path::new("/tmp/gizlet-ota.XXXXXX"))?;
    let manifest_file = work.0.join("manifest.json");
    let effective = download(&format!("{ORIGIN}/latest/manifest.json"), &manifest_file, 64 * 1024, true, None)?;
    let tag = effective.strip_prefix(&format!("{ORIGIN}/")).and_then(|path| path.strip_suffix("/manifest.json"))
        .filter(|tag| valid_tag(tag)).ok_or("Invalid release URL")?;
    let manifest: Manifest = serde_json::from_reader(File::open(&manifest_file).map_err(error)?).map_err(error)?;
    if manifest.ota.is_some() {
        let signature = work.0.join("manifest.json.sig");
        download(&format!("{ORIGIN}/{tag}/manifest.json.sig"), &signature, 4096, false, None)?;
        verify_manifest(&manifest_file, &signature)?;
    }
    offer(manifest, tag, current)
}

#[cfg(any(target_os = "linux", test))]
fn verify_file(artifact: &Artifact, path: &Path) -> Result<(), String> {
    if fs::metadata(path).map_err(error)?.len() != artifact.bytes || hash(path).map_err(error)? != artifact.sha256 {
        return Err("Update file checksum mismatch".into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn fetch_artifact(tag: &str, artifact: &Artifact, path: &Path, mut progress: impl FnMut(u8)) -> Result<(), String> {
    download(&format!("{ORIGIN}/{tag}/{}", artifact.path), path, artifact.bytes, false, Some(&mut progress))?;
    verify_file(artifact, path)
}

#[cfg(target_os = "linux")]
fn persist(path: &Path) -> Result<(), String> {
    run(Command::new("sync").arg("-f").arg(path), "Cannot save update to the card")
}

#[cfg(target_os = "linux")]
pub fn install(offer: &Offer, mut progress: impl FnMut(Progress)) -> Result<(), String> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::symlink;

    if !Path::new("/run/gizlet-data.ready").is_file() {
        return Err("The data partition is not mounted".into());
    }
    let guard = File::create("/run/gizlet-ota.lock").map_err(error)?;
    if unsafe { libc::flock(guard.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("An update is already in progress".into());
    }
    if pointer("pending").is_some() {
        return Err("Restart to finish the pending update".into());
    }
    let store = Path::new(STORE);
    fs::create_dir_all(store).map_err(error)?;
    let current = pointer("current");
    let previous = pointer("previous");
    for entry in fs::read_dir(store).map_err(error)? {
        let entry = entry.map_err(error)?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if (valid_generation(name) || name.starts_with(".stage.")) && entry.file_type().map_err(error)?.is_dir()
            && Some(name) != current.as_deref() && Some(name) != previous.as_deref()
            && !Command::new("mountpoint").arg("-q").arg(entry.path().join("root"))
                .status().map_err(error)?.success()
        {
            fs::remove_dir_all(entry.path()).map_err(error)?;
        }
    }
    let work = Temp::new(&store.join(".stage.XXXXXX"))?;
    let image = work.0.join("image.sqfs");
    let bundle = work.0.join("launcher.gizlet");
    progress(Progress::Runtime(0));
    if !offer.runtime_changed && fs::hard_link(runtime_image_path(&runtime_root()), &image).is_ok() {
        verify_file(&offer.runtime, &image)?;
    } else {
        fetch_artifact(&offer.tag, &offer.runtime, &image, |percent| progress(Progress::Runtime(percent)))?;
    }
    progress(Progress::Launcher(0));
    fetch_artifact(&offer.tag, &offer.launcher, &bundle, |percent| progress(Progress::Launcher(percent)))?;
    progress(Progress::Verify);
    run(
        Command::new("/usr/lib/gizlet/launcher-release")
            .arg("stage").arg(&bundle).arg(work.0.join("launcher")).arg(&offer.tag[1..]),
        "Launcher bundle verification failed",
    )?;
    fs::remove_file(bundle).map_err(error)?;
    fs::create_dir(work.0.join("root")).map_err(error)?;
    let legacy_runtime = fs::read_link("/opt/gizlet/runtime/1-current").unwrap_or_default();
    fs::write(work.0.join("legacy-runtime"), legacy_runtime.as_os_str().as_encoded_bytes()).map_err(error)?;
    run(
        Command::new("/usr/lib/gizlet/gizlet-runtime").arg("trial").arg(&work.0),
        "Runtime and Launcher compatibility check failed",
    )?;
    run(Command::new("umount").arg(work.0.join("root")), "Cannot unmount the staged Runtime")?;
    let staging = work.0.file_name().and_then(|name| name.to_str()).ok_or("Invalid staging path")?;
    let suffix = staging.strip_prefix(".stage.").ok_or("Invalid staging directory")?;
    let generation = format!("{}.{suffix}", offer.tag);
    persist(&work.0)?;
    fs::rename(&work.0, store.join(&generation)).map_err(error)?;
    let next = store.join("pending.new");
    if let Err(err) = fs::remove_file(&next) {
        if err.kind() != io::ErrorKind::NotFound {
            return Err(error(err));
        }
    }
    symlink(&generation, &next).map_err(error)?;
    fs::rename(next, store.join("pending")).map_err(error)?;
    persist(store)
}

fn runtime_image_path(root: &Path) -> PathBuf {
    if root.ends_with("root") { root.with_file_name("image.sqfs") } else { root.with_extension("sqfs") }
}

#[cfg(not(target_os = "linux"))]
pub fn install(_: &Offer, _: impl FnMut(Progress)) -> Result<(), String> {
    Err("OTA requires Gizlet OS".into())
}

pub fn command(args: &[String]) -> Result<(), String> {
    if args == ["factory-version"] {
        println!("{}", factory_version().unwrap_or("factory"));
        return Ok(());
    }
    let current = installed();
    match args {
        [action] if action == "status" => {
            let launcher = current.launcher_tag();
            println!("Launcher: {}\nRuntime 1: {}\nRuntime release: {}",
                launcher.as_deref().unwrap_or(&current.launcher),
                current.runtime.as_deref().unwrap_or("unknown"),
                current.runtime_tag.as_deref().unwrap_or("unknown"));
            if let Some(tag) = current.pending { println!("Pending restart: {tag}"); }
            if let Some(tag) = current.failed { println!("Rolled back: {tag}"); }
        }
        [action] if action == "check" => {
            match check(&current)? {
                Some(offer) => println!("Available: {} (Runtime: {}, Launcher: {}, {} bytes)", offer.tag, offer.runtime_changed, offer.launcher_changed, offer.bytes()),
                None => println!("No update available"),
            }
        }
        [action, tag] if action == "install" => {
            let offer = check(&current)?.filter(|offer| &offer.tag == tag).ok_or("That release is no longer offered. Check for updates again.")?;
            install(&offer, |phase| match phase {
                Progress::Runtime(0) => println!("Preparing Runtime"),
                Progress::Launcher(0) => println!("Downloading Launcher"),
                Progress::Verify => println!("Verifying and staging"),
                _ => {}
            })?;
            println!("Restart to finish installing {tag}");
        }
        _ => return Err("usage: gizlet system ota status|check|install TAG".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        Manifest {
            schema_version: 1,
            tag: "v1.0.10".into(),
            ota: Some(Metadata {
                protocol: 1,
                arch: "aarch64".into(),
                runtime_abi: 1,
                runtime: Artifact {
                    path: "runtime/gizlet-runtime-1.sqfs".into(),
                    sha256: "a".repeat(64),
                    bytes: 1,
                },
                launcher: Artifact {
                    path: "launcher/gizlet-launcher-1.0.10.gizlet".into(),
                    sha256: "b".repeat(64),
                    bytes: 1,
                },
            }),
        }
    }

    fn current() -> Installed {
        Installed { launcher: "1.0.9".into(), runtime: Some("a".repeat(64)), runtime_tag: None, pending: None, failed: None }
    }

    #[test]
    fn reads_release_tags_from_artifact_metadata() {
        let work = Temp::new(&std::env::temp_dir().join("gizlet-tags.XXXXXX")).unwrap();
        let runtime_tag = work.0.join("release-tag");
        assert_eq!(runtime_release_tag(&work.0), None);
        fs::write(&runtime_tag, "1\n").unwrap();
        assert_eq!(runtime_release_tag(&work.0), None);
        fs::write(&runtime_tag, "v2.0.1\n").unwrap();
        assert_eq!(runtime_release_tag(&work.0).as_deref(), Some("v2.0.1"));
        fs::write(work.0.join("manifest"), "gizlet-launcher-1 2.0.2 aarch64-unknown-linux-gnu runtime-1\n").unwrap();
        assert_eq!(launcher_bundle_version(&work.0).and_then(|release| tag_from_version(&release)).as_deref(), Some("v2.0.2"));
    }

    #[test]
    fn compares_launcher_versions_and_runtime_builds_independently() {
        let mut installed = current();
        let update = offer(manifest(), "v1.0.10", &installed).unwrap().unwrap();
        assert!(update.launcher_changed);
        assert!(!update.runtime_changed);
        installed.launcher = "1.0.10".into();
        assert!(offer(manifest(), "v1.0.10", &installed).unwrap().is_none());
        installed.runtime = Some("c".repeat(64));
        let update = offer(manifest(), "v1.0.10", &installed).unwrap().unwrap();
        assert!(!update.launcher_changed);
        assert!(update.runtime_changed);
        installed.launcher = "1.0.11".into();
        assert!(offer(manifest(), "v1.0.10", &installed).unwrap().is_none());
    }

    #[test]
    fn refuses_unexpected_release_path_architecture_and_abi() {
        let mut wrong = manifest();
        wrong.ota.as_mut().unwrap().runtime.path = "../gizlet-runtime-1.sqfs".into();
        assert!(offer(wrong, "v1.0.10", &current()).is_err());
        let mut wrong = manifest();
        wrong.ota.as_mut().unwrap().arch = "x86_64".into();
        assert!(offer(wrong, "v1.0.10", &current()).is_err());
        let mut wrong = manifest();
        wrong.ota.as_mut().unwrap().runtime_abi = 2;
        assert!(offer(wrong, "v1.0.10", &current()).is_err());
        assert!(offer(manifest(), "v1.0.11", &current()).is_err());
    }

    #[test]
    fn rejects_truncated_and_corrupt_downloads() {
        let work = Temp::new(&std::env::temp_dir().join("gizlet-ota-test.XXXXXX")).unwrap();
        let path = work.0.join("image.sqfs");
        fs::write(&path, b"complete").unwrap();
        let artifact = Artifact { path: String::new(), sha256: hash(&path).unwrap(), bytes: 8 };
        assert!(verify_file(&artifact, &path).is_ok());
        fs::write(&path, b"partial").unwrap();
        assert!(verify_file(&artifact, &path).is_err());
        fs::write(&path, b"corrupt!").unwrap();
        assert!(verify_file(&artifact, &path).is_err());
    }
}
