//! `gizlet`: installs, lists, runs and removes apps from the shell. On Gizlet
//! OS `/usr/bin/gizlet` points to the CLI installed alongside the launcher.

use crate::apps::{self, Library};
use crate::settings::{self, Settings};
use std::path::Path;

const USAGE: &str = "\
usage: gizlet <command>

  install FILE.gzl       install or update an app
  uninstall ID [--saves] remove an app; its saves too with --saves
  list                   list installed apps
  run ID [ARGS]          run an app here and wait for it; holding Home
                         for 1 s or Select+Start for 2 s ends it
  start ID               hand the screen from the launcher to an app
  target                 print this device's target triple and tier

Apps live in <prefix>/apps/<ID>, their saves in <prefix>/saves/<ID>; the
prefix is /opt/gizlet on Gizlet OS, or GIZLET_PREFIX.";

/// Runs a subcommand and returns its exit status.
pub fn main(args: &[String]) -> i32 {
    let (command, rest) = args.split_first().map_or(("help", &[][..]), |(command, rest)| (command.as_str(), rest));
    if cfg!(feature = "gizlet-os") && command == "ota" {
        return match crate::ota::command(rest) {
            Ok(()) => 0,
            Err(message) => {
                eprintln!("gizlet: {message}");
                1
            }
        };
    }
    let library = Library::open();
    let status = match (command, rest) {
        ("help" | "--help" | "-h", _) => {
            println!("{USAGE}");
            if cfg!(feature = "gizlet-os") {
                println!("\nOn Gizlet OS:\n  update FILE|HTTPS_URL  install a signed Launcher and CLI update\n  system reload-runtime FILE.sqfs\n                         activate a local runtime image for new launches\n  system ota status|check|install TAG\n                         check or stage an official device update");
            }
            Ok(())
        }
        ("install", [file]) => install(&library, Path::new(file)),
        ("uninstall" | "remove", [id]) => uninstall(&library, id, false),
        ("uninstall" | "remove", [id, flag]) | ("uninstall" | "remove", [flag, id]) if flag == "--saves" => {
            uninstall(&library, id, true)
        }
        ("list", []) => {
            list(&library);
            Ok(())
        }
        ("run", [id, args @ ..]) => run(&library, id, args),
        ("start", [id]) => start(&library, id),
        ("target", []) => {
            println!("{} {}", apps::device_target(), apps::device_tier());
            Ok(())
        }
        ("install" | "uninstall" | "remove" | "list" | "run" | "start" | "target", _) => {
            Err(format!("wrong arguments for {command}\n{USAGE}"))
        }
        _ => {
            eprintln!("gizlet: unknown command {command}\n{USAGE}");
            return 2;
        }
    };
    match status {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("gizlet: {e}");
            1
        }
    }
}

fn install(library: &Library, file: &Path) -> Result<(), String> {
    if file.extension().is_none_or(|e| e != "gzl") {
        return Err(format!("{} is not a .gzl package", file.display()));
    }
    let app = library.install(file)?;
    let m = &app.manifest;
    println!("installed {} {} ({}) in {}", m.id, m.version, m.name, app.dir.display());
    Ok(())
}

fn uninstall(library: &Library, id: &str, saves: bool) -> Result<(), String> {
    library.remove(id, saves)?;
    if saves {
        println!("removed {id} and its saves");
    } else {
        println!("removed {id}; its saves stay in {}", library.saves(id).display());
    }
    Ok(())
}

fn list(library: &Library) {
    for app in library.scan() {
        let m = &app.manifest;
        println!("{}\t{}\t{}", m.id, m.version, m.name);
    }
}

/// Runs the app in the foreground with the launcher's language and A/B choice.
fn run(library: &Library, id: &str, args: &[String]) -> Result<(), String> {
    // Held until the app exits, so `gizlet install` and `uninstall` refuse meanwhile.
    let (app, running) = library.open_for_run(id)?;
    let settings = Settings::load(&settings::data_dir().join("settings.ini"));
    let mut cmd = apps::command(library, &app, settings.language.app_code(), settings.swap_ab, args)
        .map_err(|e| format!("cannot prepare {id}: {e}"))?;
    library.record_played(id);
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        // The app inherits the lock too, so anything it leaves running keeps it.
        unsafe { libc::fcntl(running.as_raw_fd(), libc::F_SETFD, 0) };
    }
    // Holding Home, or Select and Start, ends the app; the launcher comes back.
    let status = crate::supervise::run(&mut cmd).map_err(|e| format!("cannot start {}: {e}", app.entry().display()))?;
    drop(running);
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            std::process::exit(128 + signal);
        }
    }
    std::process::exit(status.code().unwrap_or(1));
}

/// Asks the launcher on the screen to quit and start the app; it comes back
/// when the app exits.
fn start(library: &Library, id: &str) -> Result<(), String> {
    let app = library.get(id)?;
    if app.manifest.kind() != apps::Kind::App {
        return Err(apps::not_an_app(&app.manifest));
    }
    if !cfg!(feature = "gizlet-os") {
        return run(library, id, &[]);
    }
    if !launcher_running() {
        return Err(format!("no launcher is on the screen; use gizlet run {id}"));
    }
    request_launch(id).map_err(|e| format!("{}: {e}", apps::LAUNCH_REQUEST))?;
    println!("the launcher hands the screen to {id}");
    Ok(())
}

/// Writes the request the launcher's start script reads.
/// Each request is written to its own file, then renamed over the request,
/// so two at once never publish a partial or each other's ID.
pub fn request_launch(id: &str) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let tmp = format!("{}.{}-{}.new", apps::LAUNCH_REQUEST, std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed));
    let result = std::fs::write(&tmp, format!("{id}\n")).and_then(|()| std::fs::rename(&tmp, apps::LAUNCH_REQUEST));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn launcher_running() -> bool {
    let Some(pid) = std::fs::read_to_string(apps::LAUNCHER_PID).ok().and_then(|p| p.trim().parse::<u32>().ok()) else {
        return false;
    };
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .is_ok_and(|c| String::from_utf8_lossy(&c).contains("gizlet-launcher"))
}
