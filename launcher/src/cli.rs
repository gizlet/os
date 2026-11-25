//! `gizlet`: installs, lists, runs and removes apps from the shell. On Gizlet
//! OS `/usr/bin/gizlet` runs these subcommands of the launcher's program.

use crate::apps::{self, Library};
use crate::settings::{self, Settings};
use std::path::Path;

const USAGE: &str = "\
usage: gizlet <command>

  install FILE.gzl       install or update an app
  uninstall ID [--saves] remove an app; its saves too with --saves
  list                   list installed apps
  run ID [ARGS]          run an app here and wait for it
  start ID               hand the screen from the launcher to an app
  target                 print this device's target triple and tier

Apps live in <prefix>/apps/<ID>, their saves in <prefix>/saves/<ID>; the
prefix is /opt/gizlet on Gizlet OS, or GIZLET_PREFIX.";

/// Runs `args` if they name a subcommand, returning its exit status.
pub fn main(args: &[String]) -> Option<i32> {
    let (command, rest) = args.split_first()?;
    let library = Library::open();
    let status = match (command.as_str(), rest) {
        ("help" | "--help" | "-h", _) => {
            println!("{USAGE}");
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
        _ => return None,
    };
    Some(match status {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("gizlet: {e}");
            1
        }
    })
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
    let app = library.get(id)?;
    let settings = Settings::load(&settings::data_dir().join("settings.ini"));
    let mut cmd = apps::command(library, &app, settings.language.app_code(), settings.swap_ab, args)
        .map_err(|e| format!("cannot prepare {id}: {e}"))?;
    library.record_played(id);
    // Held until the app exits, so `gizlet install` and `uninstall` wait for it.
    let running = library.hold_running(id).map_err(|e| format!("cannot mark {id} running: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        use std::os::unix::process::CommandExt;
        // The app inherits the lock across exec.
        unsafe { libc::fcntl(running.as_raw_fd(), libc::F_SETFD, 0) };
        let e = cmd.exec();
        Err(format!("cannot start {}: {e}", app.entry().display()))
    }
    #[cfg(not(unix))]
    {
        let status = cmd.status().map_err(|e| format!("cannot start {}: {e}", app.entry().display()))?;
        drop(running);
        std::process::exit(status.code().unwrap_or(1));
    }
}

/// Asks the launcher on the screen to quit and start the app; it comes back
/// when the app exits.
fn start(library: &Library, id: &str) -> Result<(), String> {
    library.get(id)?;
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
pub fn request_launch(id: &str) -> std::io::Result<()> {
    let tmp = format!("{}.new", apps::LAUNCH_REQUEST);
    std::fs::write(&tmp, format!("{id}\n"))?;
    std::fs::rename(&tmp, apps::LAUNCH_REQUEST)
}

fn launcher_running() -> bool {
    let Some(pid) = std::fs::read_to_string(apps::LAUNCHER_PID).ok().and_then(|p| p.trim().parse::<u32>().ok()) else {
        return false;
    };
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .is_ok_and(|c| String::from_utf8_lossy(&c).contains("gizlet-launcher"))
}
