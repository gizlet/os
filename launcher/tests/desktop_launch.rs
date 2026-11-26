#![cfg(all(unix, not(feature = "gizlet-os")))]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

struct TestDir(PathBuf);

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn shelf_worker_runs_app() {
    let root = std::env::temp_dir().join(format!("gizlet-desktop-launch-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let root = TestDir(root);
    let id = "dev.gizlet.desktop-test";
    let app = root.0.join("apps").join(id);
    fs::create_dir_all(&app).unwrap();
    let target = gizlet_launcher::apps::device_target();
    let manifest = format!(
        "id = \"{id}\"\nname = \"Desktop Test\"\nversion = \"1.0.0\"\ntier = \"lite\"\ntarget = \"{target}\"\nentry = \"hello\"\n",
    );
    fs::write(app.join("manifest.toml"), manifest).unwrap();
    let entry = app.join("hello");
    fs::write(&entry, "#!/bin/sh\nprintf '%s\\n' \"$GIZLET_APP_ID\"\n").unwrap();
    fs::set_permissions(&entry, fs::Permissions::from_mode(0o755)).unwrap();

    // The desktop shelf re-execs gizlet-launcher with these arguments.
    let child = Command::new(env!("CARGO_BIN_EXE_gizlet-launcher"))
        .args(["run", id])
        .env("GIZLET_PREFIX", &root.0)
        .env("GIZLET_LAUNCHER_DATA", &root.0)
        .output()
        .unwrap();
    assert!(child.status.success(), "{child:?}");
    assert_eq!(String::from_utf8(child.stdout).unwrap().trim(), id);
}
