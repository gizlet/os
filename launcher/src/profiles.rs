//! Local profiles: each person on the handheld keeps their own Launcher
//! settings, app data, saves and service state, while the installed packages
//! are shared.
//!
//! The data partition is mounted at `/data` on Gizlet OS:
//!
//! | Path | Holds |
//! | --- | --- |
//! | `/data/device/` | What every profile shares: packages, runtimes, Launcher releases and device settings |
//! | `/data/device/boot-profile` | The ID of the profile the next boot mounts |
//! | `/data/profiles/<id>/` | One profile: `profile.ini`, `launcher/`, `apps/`, `services/`, `enabled-services` and `home/` |
//!
//! At boot `gizlet-mount-data` mounts the chosen profile at `/run/gizlet/profile`
//! and its `home/` at `/home`, and writes its ID to `/run/gizlet/profile.id`.
//! Only that profile is current until the next boot: switching stops its
//! services, names the new profile in `boot-profile` and restarts the device.
//! Elsewhere, without those mounts, the current profile is the one
//! `boot-profile` names under the launcher's data folder.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

/// Where Gizlet OS mounts the data partition.
pub const DATA_ROOT: &str = "/data";
/// Where Gizlet OS mounts the current profile.
pub const MOUNT: &str = "/run/gizlet/profile";
/// The current profile's ID, written by `gizlet-mount-data` as it mounts it.
pub const MOUNTED_ID: &str = "/run/gizlet/profile.id";
/// Profile IDs are this many lowercase hexadecimal digits.
pub const ID_LEN: usize = 16;
/// The longest name, in characters.
pub const MAX_NAME_CHARS: usize = 24;
/// What a profile without a name is called, as `gizlet-mount-data` creates the first one.
pub const DEFAULT_NAME: &str = "Default";

/// A profile ID: [`ID_LEN`] lowercase hexadecimal digits, chosen at random when it is created.
pub fn is_valid_id(id: &str) -> bool {
    id.len() == ID_LEN && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Why `name` cannot name a profile.
pub fn name_problem(name: &str) -> Option<&'static str> {
    if name.trim().is_empty() {
        return Some("a profile needs a name");
    }
    if name.trim() != name {
        return Some("a profile name cannot start or end with a space");
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Some("a profile name has at most 24 characters");
    }
    if name.chars().any(char::is_control) {
        return Some("a profile name cannot hold control characters");
    }
    None
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub id: String,
    /// Empty for the profile the first boot creates.
    pub name: String,
    /// Seconds since 1970 when it was created; profiles are listed in this order.
    pub created: u64,
}

impl Profile {
    /// Its name, or [`DEFAULT_NAME`] without one.
    pub fn label(&self) -> &str {
        if self.name.is_empty() {
            DEFAULT_NAME
        } else {
            &self.name
        }
    }

    fn parse(id: &str, text: &str) -> Profile {
        let mut profile = Profile { id: id.into(), name: String::new(), created: 0 };
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            match key.trim() {
                "name" if name_problem(value.trim()).is_none() => profile.name = value.trim().into(),
                "created" => profile.created = value.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
        profile
    }

    fn render(&self) -> String {
        format!("name = {}\ncreated = {}\n", self.name, self.created)
    }
}

/// The profiles under a data root: `/data` on Gizlet OS.
pub struct Profiles {
    root: PathBuf,
}

impl Profiles {
    pub fn new(root: PathBuf) -> Profiles {
        let root = std::path::absolute(&root).unwrap_or(root);
        Profiles { root }
    }

    /// The profiles of [`crate::apps::prefix`].
    pub fn open() -> Profiles {
        Profiles::new(crate::apps::prefix())
    }

    /// `<root>/profiles`.
    pub fn dir(&self) -> PathBuf {
        self.root.join("profiles")
    }

    fn boot_file(&self) -> PathBuf {
        self.root.join("device/boot-profile")
    }

    /// Whether the current profile is the one Gizlet OS mounted at boot.
    fn mounted(&self) -> bool {
        cfg!(feature = "gizlet-os") && self.root == Path::new(DATA_ROOT)
    }

    /// Every profile, oldest first.
    pub fn list(&self) -> Vec<Profile> {
        let mut found: Vec<Profile> = fs::read_dir(self.dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| self.load(&e.file_name().to_string_lossy()))
            .collect();
        found.sort_by(|a, b| a.created.cmp(&b.created).then(a.id.cmp(&b.id)));
        found
    }

    fn load(&self, id: &str) -> Option<Profile> {
        if !is_valid_id(id) {
            return None;
        }
        let text = fs::read_to_string(self.dir().join(id).join("profile.ini")).ok()?;
        Some(Profile::parse(id, &text))
    }

    /// The profile `key` names: its ID, or its name in any case.
    pub fn find(&self, key: &str) -> Result<Profile, String> {
        if let Some(profile) = self.load(key) {
            return Ok(profile);
        }
        let all = self.list();
        let key = key.trim().to_lowercase();
        all.into_iter().find(|p| p.label().to_lowercase() == key).ok_or_else(|| format!("no profile is called {key:?}"))
    }

    /// The ID of the profile in use. On Gizlet OS that is the one mounted at
    /// boot; elsewhere the one `boot-profile` names, chosen or created as the
    /// boot does when it names none.
    pub fn current_id(&self) -> Result<String, String> {
        if self.mounted() {
            let id = fs::read_to_string(MOUNTED_ID).map_err(|e| format!("no profile is mounted ({MOUNTED_ID}: {e})"))?;
            let id = id.trim();
            return if is_valid_id(id) { Ok(id.into()) } else { Err(format!("{MOUNTED_ID} holds no profile ID")) };
        }
        self.choose_for_boot()
    }

    pub fn current(&self) -> Result<Profile, String> {
        let id = self.current_id()?;
        self.load(&id).ok_or_else(|| format!("the current profile {id} has no profile.ini"))
    }

    /// The current profile's folder: `/run/gizlet/profile` on Gizlet OS.
    pub fn current_dir(&self) -> Result<PathBuf, String> {
        if self.mounted() {
            self.current_id()?;
            return Ok(PathBuf::from(MOUNT));
        }
        let id = self.current_id()?;
        Ok(self.dir().join(id))
    }

    /// The profile the next boot mounts, if `boot-profile` names one that exists.
    pub fn next_id(&self) -> Option<String> {
        let id = fs::read_to_string(self.boot_file()).ok()?;
        let id = id.trim();
        self.load(id).map(|p| p.id)
    }

    /// What `gizlet-mount-data` does at boot: the profile `boot-profile`
    /// names, else the oldest one, else a new one without a name.
    fn choose_for_boot(&self) -> Result<String, String> {
        if let Some(id) = self.next_id() {
            return Ok(id);
        }
        let _lock = self.lock()?;
        if let Some(id) = self.next_id() {
            return Ok(id);
        }
        let id = match self.list().into_iter().next() {
            Some(profile) => profile.id,
            None => self.make("")?.id,
        };
        self.write_next(&id)?;
        Ok(id)
    }

    /// Creates a profile with empty folders and the name `name`.
    pub fn create(&self, name: &str) -> Result<Profile, String> {
        if let Some(problem) = name_problem(name) {
            return Err(problem.into());
        }
        let _lock = self.lock()?;
        self.check_unique(name, None)?;
        self.make(name)
    }

    fn make(&self, name: &str) -> Result<Profile, String> {
        let dir = self.dir();
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        self.sweep();
        let id = loop {
            let id = random_id();
            if !dir.join(&id).exists() {
                break id;
            }
        };
        // Later than every other profile, so the list keeps the order they were made in.
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let created = self.list().iter().map(|p| p.created + 1).max().unwrap_or(0).max(now);
        let profile = Profile { id: id.clone(), name: name.into(), created };
        let staging = dir.join(format!(".new-{id}"));
        let made = (|| -> io::Result<()> {
            for sub in ["launcher", "apps", "services", "home"] {
                fs::create_dir_all(staging.join(sub))?;
            }
            fs::write(staging.join("profile.ini"), profile.render())?;
            crate::apps::sync();
            fs::rename(&staging, dir.join(&id))
        })();
        if let Err(e) = made {
            let _ = fs::remove_dir_all(&staging);
            return Err(format!("cannot create the profile in {}: {e}", dir.display()));
        }
        crate::apps::sync();
        Ok(profile)
    }

    pub fn rename(&self, key: &str, name: &str) -> Result<Profile, String> {
        if let Some(problem) = name_problem(name) {
            return Err(problem.into());
        }
        let _lock = self.lock()?;
        let mut profile = self.find(key)?;
        self.check_unique(name, Some(&profile.id))?;
        profile.name = name.into();
        let path = self.dir().join(&profile.id).join("profile.ini");
        write_atomic(&path, &profile.render()).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(profile)
    }

    fn check_unique(&self, name: &str, except: Option<&str>) -> Result<(), String> {
        let taken = self.list().into_iter().any(|p| Some(p.id.as_str()) != except && p.label().to_lowercase() == name.to_lowercase());
        if taken {
            Err(format!("another profile is already called {name:?}"))
        } else {
            Ok(())
        }
    }

    /// Deletes a profile that is not in use, with everything it keeps: its
    /// Launcher settings, app data, service state and home folder.
    pub fn delete(&self, key: &str) -> Result<Profile, String> {
        let current = self.current_id().ok();
        let _lock = self.lock()?;
        let profile = self.find(key)?;
        if current.as_deref() == Some(profile.id.as_str()) {
            return Err(format!("{} is the current profile; switch to another one first", profile.label()));
        }
        if self.next_id().as_deref() == Some(profile.id.as_str()) {
            return Err(format!("{} is the profile the next boot uses; switch to another one first", profile.label()));
        }
        let dir = self.dir().join(&profile.id);
        let trash = self.dir().join(format!(".delete-{}", profile.id));
        fs::rename(&dir, &trash).map_err(|e| format!("{}: {e}", dir.display()))?;
        crate::apps::sync();
        fs::remove_dir_all(&trash).map_err(|e| format!("{} is no longer listed, but {} cannot be deleted: {e}", profile.label(), trash.display()))?;
        crate::apps::sync();
        Ok(profile)
    }

    /// Names `id` in `boot-profile`, so the next boot mounts it.
    pub fn set_next(&self, id: &str) -> Result<(), String> {
        let _lock = self.lock()?;
        self.load(id).ok_or_else(|| format!("{id} is not a profile"))?;
        self.write_next(id)
    }

    fn write_next(&self, id: &str) -> Result<(), String> {
        let path = self.boot_file();
        write_atomic(&path, &format!("{id}\n")).map_err(|e| format!("{}: {e}", path.display()))?;
        crate::apps::sync();
        Ok(())
    }

    /// Removes what a create or delete cut short left behind.
    fn sweep(&self) {
        for e in fs::read_dir(self.dir()).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with(".new-") || name.starts_with(".delete-") {
                let _ = fs::remove_dir_all(e.path());
            }
        }
    }

    /// Holds `<root>/profiles/.lock` until dropped, so two changes never interleave.
    fn lock(&self) -> Result<File, String> {
        let dir = self.dir();
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = dir.join(".lock");
        let file = File::options().create(true).truncate(false).write(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        file.lock().map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(file)
    }
}

/// Prepares a switch to the profile `key` names: stops every service of the
/// current profile and waits until each has exited, then names the new
/// profile for the next boot. The caller restarts the device ([`restart`]);
/// until then the current profile stays mounted.
pub fn prepare_switch(library: &crate::apps::Library, profiles: &Profiles, key: &str) -> Result<Profile, String> {
    let target = profiles.find(key)?;
    if profiles.current_id()? == target.id {
        return Err(format!("{} is already the current profile", target.label()));
    }
    library.stop_services().map_err(|e| format!("cannot switch to {}: {e}", target.label()))?;
    profiles.set_next(&target.id)?;
    Ok(target)
}

/// Whether a switch takes a restart here: on Gizlet OS, where the boot mounts the profile.
pub fn switch_restarts() -> bool {
    cfg!(feature = "gizlet-os")
}

/// Restarts the device so the boot mounts the profile [`prepare_switch`] chose.
pub fn restart() -> Result<(), String> {
    let status = std::process::Command::new("systemctl")
        .arg("reboot")
        .stdin(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("cannot run systemctl: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("systemctl reboot failed ({status})"))
    }
}

fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("new");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

/// [`ID_LEN`] random lowercase hexadecimal digits.
fn random_id() -> String {
    let mut bytes = [0u8; ID_LEN / 2];
    let random = File::open("/dev/urandom").and_then(|mut f| io::Read::read_exact(&mut f, &mut bytes));
    if random.is_err() {
        use sha2::{Digest, Sha256};
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let seed = format!("{now} {} {}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed));
        bytes.copy_from_slice(&Sha256::digest(seed.as_bytes())[..ID_LEN / 2]);
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gizlet-profiles-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn checks_ids_and_names() {
        assert!(is_valid_id("0123456789abcdef"));
        assert!(is_valid_id(&random_id()));
        assert_ne!(random_id(), random_id());
        for bad in ["", "0123456789abcde", "0123456789abcdef0", "0123456789ABCDEF", "../3456789abcdef", "0123456789abcdeg"] {
            assert!(!is_valid_id(bad), "{bad}");
        }
        assert_eq!(name_problem("小王"), None);
        assert_eq!(name_problem(&"x".repeat(24)), None);
        assert!(name_problem(&"x".repeat(25)).is_some());
        for bad in ["", "  ", " Ann", "Ann ", "A\nB", "A\tB"] {
            assert!(name_problem(bad).is_some(), "{bad:?}");
        }
    }

    #[test]
    fn reads_and_writes_profile_ini() {
        let p = Profile { id: "0123456789abcdef".into(), name: "Ann".into(), created: 42 };
        assert_eq!(Profile::parse(&p.id, &p.render()), p);
        let unnamed = Profile::parse(&p.id, "created = 7\nnoise\n");
        assert_eq!((unnamed.name.as_str(), unnamed.label(), unnamed.created), ("", DEFAULT_NAME, 7));
        assert_eq!(Profile::parse(&p.id, "name = a\u{7}b\n").name, "");
    }

    #[test]
    fn chooses_creates_renames_and_deletes() {
        let root = temp("lifecycle");
        let profiles = Profiles::new(root.clone());
        // The first use creates a profile without a name, as the first boot does.
        let first = profiles.current().unwrap();
        assert_eq!(first.label(), DEFAULT_NAME);
        assert_eq!(profiles.current_dir().unwrap(), root.join("profiles").join(&first.id));
        for sub in ["launcher", "apps", "services", "home"] {
            assert!(root.join("profiles").join(&first.id).join(sub).is_dir(), "{sub}");
        }
        assert_eq!(fs::read_to_string(root.join("device/boot-profile")).unwrap(), format!("{}\n", first.id));

        let ann = profiles.create("Ann").unwrap();
        assert!(profiles.create("ann").unwrap_err().contains("already"));
        assert!(profiles.create("default").unwrap_err().contains("already"));
        assert_eq!(profiles.list().iter().map(|p| p.id.clone()).collect::<Vec<_>>(), [first.id.clone(), ann.id.clone()]);
        assert_eq!(profiles.find("ANN").unwrap().id, ann.id);
        assert_eq!(profiles.find(&ann.id).unwrap().name, "Ann");
        assert!(profiles.find("Bob").is_err());

        let renamed = profiles.rename("Ann", "Bob").unwrap();
        assert_eq!(profiles.find(&ann.id).unwrap().name, "Bob");
        assert_eq!(renamed.id, ann.id);
        assert!(profiles.rename(&ann.id, " ").is_err());

        // The profile in use cannot be deleted; another one can, with everything in it.
        assert!(profiles.delete(&first.id).unwrap_err().contains("current"));
        fs::write(root.join("profiles").join(&ann.id).join("home/notes.txt"), "hi").unwrap();
        profiles.delete("Bob").unwrap();
        assert!(!root.join("profiles").join(&ann.id).exists());
        assert_eq!(profiles.list().len(), 1);

        // A boot-profile naming a missing profile falls back to the oldest.
        let carl = profiles.create("Carl").unwrap();
        profiles.set_next(&carl.id).unwrap();
        assert_eq!(profiles.current_id().unwrap(), carl.id);
        assert!(profiles.set_next("ffffffffffffffff").is_err());
        fs::write(root.join("device/boot-profile"), "ffffffffffffffff\n").unwrap();
        assert_eq!(profiles.current_id().unwrap(), first.id);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sweeps_what_was_cut_short() {
        let root = temp("sweep");
        let profiles = Profiles::new(root.clone());
        fs::create_dir_all(root.join("profiles/.new-0123456789abcdef/home")).unwrap();
        fs::create_dir_all(root.join("profiles/.delete-0123456789abcdee/home")).unwrap();
        profiles.create("Ann").unwrap();
        assert!(!root.join("profiles/.new-0123456789abcdef").exists());
        assert!(!root.join("profiles/.delete-0123456789abcdee").exists());
        assert_eq!(profiles.list().len(), 1);
        let _ = fs::remove_dir_all(&root);
    }
}
