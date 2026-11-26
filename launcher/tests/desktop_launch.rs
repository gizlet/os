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

/// Places a package as `gizlet install` leaves it: a manifest with `extra`
/// fields and an entry that prints the app's ID.
fn place(root: &std::path::Path, id: &str, extra: serde_json::Value) {
    let app = root.join("apps").join(id);
    fs::create_dir_all(&app).unwrap();
    let mut manifest = serde_json::json!({
        "id": id,
        "name": "Desktop Test",
        "version": "1.0.0",
        "tier": "lite",
        "target": gizlet_launcher::apps::device_target(),
        "entry": "hello",
    });
    manifest.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    fs::write(app.join(gizlet_launcher::apps::MANIFEST), manifest.to_string()).unwrap();
    let entry = app.join("hello");
    fs::write(&entry, "#!/bin/sh\nprintf '%s\\n' \"$GIZLET_APP_ID\"\n").unwrap();
    fs::set_permissions(&entry, fs::Permissions::from_mode(0o755)).unwrap();
}

/// What the desktop shelf runs: gizlet-launcher re-execs itself with these arguments.
fn run(root: &std::path::Path, id: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_gizlet-launcher"))
        .args(["run", id])
        .env("GIZLET_PREFIX", root)
        .env("GIZLET_LAUNCHER_DATA", root)
        .output()
        .unwrap()
}

#[test]
fn shelf_worker_runs_app() {
    let root = std::env::temp_dir().join(format!("gizlet-desktop-launch-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let root = TestDir(root);
    // Schema 1, schema 1 left out, and schema 2 apps all launch.
    for (id, extra) in [
        ("dev.gizlet.desktop-test", serde_json::json!({ "schema": 1 })),
        ("dev.gizlet.desktop-noschema", serde_json::json!({})),
        ("dev.gizlet.desktop-v2", serde_json::json!({ "schema": 2, "kind": "app" })),
    ] {
        place(&root.0, id, extra);
        let child = run(&root.0, id);
        assert!(child.status.success(), "{id}: {child:?}");
        assert_eq!(String::from_utf8(child.stdout).unwrap().trim(), id);
    }
    // A service is not started as an app.
    let id = "dev.gizlet.desktop-service";
    place(&root.0, id, serde_json::json!({ "schema": 2, "kind": "service" }));
    let child = run(&root.0, id);
    assert!(!child.status.success(), "{child:?}");
    assert!(child.stdout.is_empty());
    assert!(String::from_utf8_lossy(&child.stderr).contains("is a service, not an app"), "{child:?}");
}

/// What a service's unit runs: `gizlet service exec ID` prepares the
/// service's folders and replaces itself with its entry.
#[test]
fn service_exec_runs_the_entry_with_its_folders() {
    let root = std::env::temp_dir().join(format!("gizlet-desktop-service-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let root = TestDir(root);
    let id = "dev.gizlet.desktop-service";
    let dir = root.0.join("services").join(id);
    fs::create_dir_all(&dir).unwrap();
    let manifest = serde_json::json!({
        "schema": 2,
        "kind": "service",
        "id": id,
        "name": "Desktop Service",
        "version": "1.0.0",
        "tier": "lite",
        "target": gizlet_launcher::apps::device_target(),
        "entry": "serve",
    });
    fs::write(dir.join(gizlet_launcher::apps::MANIFEST), manifest.to_string()).unwrap();
    let entry = dir.join("serve");
    fs::write(&entry, "#!/bin/sh\nprintf '%s\\n' \"$GIZLET_SERVICE_ID\" \"$GIZLET_SERVICE_DIR\" \"$GIZLET_STATE_DIR\" \"$GIZLET_RUNTIME_DIR\" \"$HOME\" \"$PWD\"\n").unwrap();
    fs::set_permissions(&entry, fs::Permissions::from_mode(0o755)).unwrap();
    let runtime = root.0.join("run").join(id);
    let child = Command::new(env!("CARGO_BIN_EXE_gizlet-cli"))
        .args(["service", "exec", id])
        .env("GIZLET_PREFIX", &root.0)
        .env("RUNTIME_DIRECTORY", &runtime)
        .output()
        .unwrap();
    assert!(child.status.success(), "{child:?}");
    let dir = fs::canonicalize(&dir).unwrap();
    let state = fs::canonicalize(root.0.join("service-data").join(id)).unwrap();
    let runtime = fs::canonicalize(&runtime).unwrap();
    let lines: Vec<PathBuf> = String::from_utf8(child.stdout).unwrap().lines().map(|l| fs::canonicalize(l).unwrap_or_else(|_| PathBuf::from(l))).collect();
    assert_eq!(lines, [PathBuf::from(id), dir.clone(), state.clone(), runtime.clone(), state.clone(), dir]);
    for folder in [&state, &runtime] {
        assert_eq!(fs::metadata(folder).unwrap().permissions().mode() & 0o777, 0o700, "{}", folder.display());
    }
    // Neither a service nor an app runs the other way.
    let child = run(&root.0, id);
    assert!(String::from_utf8_lossy(&child.stderr).contains("is a service, not an app"), "{child:?}");
    let child = Command::new(env!("CARGO_BIN_EXE_gizlet-cli")).args(["service", "exec", "dev.gizlet.none"]).env("GIZLET_PREFIX", &root.0).output().unwrap();
    assert!(!child.status.success() && String::from_utf8_lossy(&child.stderr).contains("not installed"), "{child:?}");
}
