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
/// fields and an entry that prints the app's ID and save folder.
fn place(root: &std::path::Path, id: &str, extra: serde_json::Value) {
    let app = root.join("device/packages/apps").join(id);
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
    fs::write(&entry, "#!/bin/sh\nprintf '%s\\n' \"$GIZLET_APP_ID\" \"$GIZLET_SAVE_DIR\"\n").unwrap();
    fs::set_permissions(&entry, fs::Permissions::from_mode(0o755)).unwrap();
}

/// What the desktop shelf runs: gizlet-launcher re-execs itself with these arguments.
fn run(root: &std::path::Path, id: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_gizlet-launcher"))
        .args(["run", id])
        .env("GIZLET_PREFIX", root)
        .output()
        .unwrap()
}

fn cli(root: &std::path::Path, args: &[&str]) -> String {
    let child = Command::new(env!("CARGO_BIN_EXE_gizlet-cli")).args(args).env("GIZLET_PREFIX", root).output().unwrap();
    assert!(child.status.success(), "{args:?}: {child:?}");
    String::from_utf8(child.stdout).unwrap()
}

fn current_profile(root: &std::path::Path) -> PathBuf {
    gizlet_launcher::profiles::Profiles::new(root.to_path_buf()).current_dir().unwrap()
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
        let saves = current_profile(&root.0).join("apps").join(id);
        assert_eq!(String::from_utf8(child.stdout).unwrap(), format!("{id}\n{}\n", saves.display()));
        assert!(saves.is_dir());
    }
    // Another profile runs the same installed app with its own save folder.
    let first = current_profile(&root.0);
    cli(&root.0, &["profile", "create", "Ann"]);
    assert!(cli(&root.0, &["profile", "switch", "Ann"]).contains("switched to Ann"));
    let ann = current_profile(&root.0);
    assert_ne!(ann, first);
    let id = "dev.gizlet.desktop-test";
    let child = run(&root.0, id);
    assert_eq!(String::from_utf8(child.stdout).unwrap(), format!("{id}\n{}\n", ann.join("apps").join(id).display()));
    assert!(cli(&root.0, &["profile", "list"]).lines().any(|l| l.ends_with("\tAnn\tcurrent")));
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
    let dir = root.0.join("device/packages/services").join(id);
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
    // The CLI runs it, and so does the launcher, whose Settings may register it.
    for program in [env!("CARGO_BIN_EXE_gizlet-cli"), env!("CARGO_BIN_EXE_gizlet-launcher")] {
        let child = Command::new(program)
            .args(["service", "exec", id])
            .env("GIZLET_PREFIX", &root.0)
            .env("RUNTIME_DIRECTORY", &runtime)
            .output()
            .unwrap();
        assert!(child.status.success(), "{program}: {child:?}");
        let dir = fs::canonicalize(&dir).unwrap();
        let state = fs::canonicalize(current_profile(&root.0).join("services").join(id)).unwrap();
        let runtime = fs::canonicalize(&runtime).unwrap();
        let lines: Vec<PathBuf> = String::from_utf8(child.stdout).unwrap().lines().map(|l| fs::canonicalize(l).unwrap_or_else(|_| PathBuf::from(l))).collect();
        assert_eq!(lines, [PathBuf::from(id), dir.clone(), state.clone(), runtime.clone(), state.clone(), dir], "{program}");
        for folder in [&state, &runtime] {
            assert_eq!(fs::metadata(folder).unwrap().permissions().mode() & 0o777, 0o700, "{}", folder.display());
        }
    }
    // Neither a service nor an app runs the other way.
    let child = run(&root.0, id);
    assert!(String::from_utf8_lossy(&child.stderr).contains("is a service, not an app"), "{child:?}");
    let child = Command::new(env!("CARGO_BIN_EXE_gizlet-cli")).args(["service", "exec", "dev.gizlet.none"]).env("GIZLET_PREFIX", &root.0).output().unwrap();
    assert!(!child.status.success() && String::from_utf8_lossy(&child.stderr).contains("not installed"), "{child:?}");
}
