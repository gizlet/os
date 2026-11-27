//! `gizlet`: installs, lists, runs and removes apps, and installs and
//! controls services, from the shell. On Gizlet OS `/usr/bin/gizlet` points
//! to the CLI installed alongside the launcher.

use crate::apps::{self, Kind, Library};
use crate::services::{self, Service};
use crate::settings::{self, Settings};
use crate::store::{self, Store};
use std::path::Path;

const USAGE: &str = "\
usage: gizlet <command>

  install FILE.gzl       install or update an app or a service
  uninstall ID [--saves] remove an app, and its saves with --saves; or stop
                         a service and remove it with all its files
  list                   list installed apps
  run ID [ARGS]          run an app here and wait for it; holding Home
                         for 1 s or Select+Start for 2 s ends it
  start ID               hand the screen from the launcher to an app
  target                 print this device's target triple and tier

  store search [WORDS]   search the Gizlet Store for apps this device runs
  store info LINK        show an app's page: a short link such as
                         gizlet.io/rocket, or a GitHub repository
  store install LINK     install an app or a service from its short link
                         or GitHub repository
  store updates          list updates for what the Store installed
  store upgrade [ID...]  install those updates, all of them without IDs

  service list           list installed services: ID, version, state,
                         whether it starts at boot, and name
  service status ID      show a service's state, unit and folders
  service start|stop|restart ID
                         start or stop a service now
  service enable|disable ID
                         start a service at boot, or not
  service logs ID [-f]   show a service's log

Apps live in <prefix>/apps/<ID>, their saves in <prefix>/saves/<ID>;
services in <prefix>/services/<ID>, their state in <prefix>/service-data/<ID>.
The prefix is /opt/gizlet on Gizlet OS, or GIZLET_PREFIX.";

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
        ("service" | "services", rest) => service(&library, rest),
        ("store", rest) => store_command(&library, rest),
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
    // A file replaces what the Store installed; the Store no longer updates it.
    Store::new(library).forget(&m.id);
    if m.kind() == Kind::Service {
        let service = library.service(&m.id)?;
        let boot = if service.boot { "starts at boot" } else { "does not start at boot" };
        println!("installed the service {} {} ({}) in {}: {}, {boot}", m.id, m.version, m.name, app.dir.display(), service.state.word());
        return Ok(());
    }
    println!("installed {} {} ({}) in {}", m.id, m.version, m.name, app.dir.display());
    Ok(())
}

fn uninstall(library: &Library, id: &str, saves: bool) -> Result<(), String> {
    let kind = library.remove(id, saves)?;
    Store::new(library).forget(id);
    if kind == Kind::Service {
        println!("removed the service {id}: stopped, unregistered, and deleted with its state");
    } else if saves {
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

/// `gizlet store ...`: the Gizlet Store, without signing in.
fn store_command(library: &Library, args: &[String]) -> Result<(), String> {
    let store = Store::new(library);
    let lang = Settings::load(&settings::data_dir().join("settings.ini")).language.app_code();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let fail = |e: store::Error| e.to_string();
    match args.as_slice() {
        ["search", words @ ..] => {
            let found = store.search(&words.join(" ")).map_err(fail)?;
            if found.is_empty() {
                println!("no apps found");
            }
            for g in found {
                println!("{}\t{}\t{}", g.repo, g.version.as_deref().unwrap_or("-"), g.name_for(lang));
            }
            Ok(())
        }
        ["info", link] => {
            let detail = store.detail(&store.resolve(link).map_err(fail)?).map_err(fail)?;
            let g = &detail.summary;
            println!("{} {} ({})", g.name_for(lang), g.version.as_deref().unwrap_or("-"), g.id);
            println!("repository: {}", g.repo);
            if g.is_unlisted() {
                println!("unlisted: not reviewed for the Store's lists");
            }
            if let Some(d) = g.description_for(lang) {
                println!("{d}");
            }
            if g.is_service() {
                println!("a background service: it runs as root, starts now and at boot");
            }
            if let Some(p) = detail.package_here() {
                println!("package: {} ({} bytes)", p.file, p.size.unwrap_or(0));
            } else {
                println!("no package for {} ({})", apps::device_target(), apps::device_tier());
            }
            if let Some(origin) = store.origin(&g.id) {
                println!("installed: {} from {}", origin.version, origin.repo);
            }
            Ok(())
        }
        ["install", link] => {
            let repo = store.resolve(link).map_err(fail)?;
            let app = store.install(library, &repo, &mut progress()).map_err(fail)?;
            eprintln!();
            let m = &app.manifest;
            if m.kind() == Kind::Service {
                let state = library.service(&m.id).map(|s| s.state.word().to_string()).unwrap_or_else(|_| "installed".into());
                println!("installed the service {} {} ({}) from {repo}: {state}", m.id, m.version, m.name_for(lang));
            } else {
                println!("installed {} {} ({}) from {repo}", m.id, m.version, m.name_for(lang));
            }
            Ok(())
        }
        ["updates"] => {
            let updates: Vec<_> = store.updates(library).map_err(fail)?.into_iter().filter(|u| u.has_update()).collect();
            if updates.is_empty() {
                println!("everything from the Store is up to date");
            }
            for u in updates {
                println!("{}\t{} -> {}\t{}", u.id, u.installed, u.version.as_deref().unwrap_or("-"), u.name_for(lang));
            }
            Ok(())
        }
        ["upgrade", ids @ ..] => {
            let updates = store.updates(library).map_err(fail)?;
            for id in ids {
                if !updates.iter().any(|u| u.id == *id && u.has_update()) {
                    return Err(format!("{id} has no update from the Store"));
                }
            }
            let chosen: Vec<_> = updates.iter().filter(|u| u.has_update() && (ids.is_empty() || ids.contains(&u.id.as_str()))).collect();
            if chosen.is_empty() {
                println!("everything from the Store is up to date");
            }
            for u in chosen {
                let app = store.upgrade(library, u, &mut progress()).map_err(fail)?;
                eprintln!();
                println!("updated {} {} -> {}", app.manifest.id, u.installed, app.manifest.version);
            }
            Ok(())
        }
        _ => Err(format!("wrong arguments for store\n{USAGE}")),
    }
}

/// Prints download progress on one line of standard error.
fn progress() -> impl FnMut(store::Progress) {
    let mut last = None;
    move |p| {
        let line = match p {
            store::Progress::Downloading(done, total) => format!("downloading {}%", done * 100 / total.max(1)),
            store::Progress::Installing => "installing".into(),
        };
        if last.as_ref() != Some(&line) {
            eprint!("\r{line:<20}");
            last = Some(line);
        }
    }
}

/// `gizlet service ...`: shows and controls services.
fn service(library: &Library, args: &[String]) -> Result<(), String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] | ["list"] => {
            for s in library.services()? {
                let m = &s.package.manifest;
                let boot = if s.boot { "enabled" } else { "disabled" };
                println!("{}\t{}\t{}\t{boot}\t{}", m.id, m.version, s.state.word(), m.name);
            }
            Ok(())
        }
        ["status", id] => {
            print_status(library, &library.service(id)?);
            Ok(())
        }
        ["start", id] => report(library.start_service(id)?),
        ["stop", id] => report(library.stop_service(id)?),
        ["restart", id] => report(library.restart_service(id)?),
        ["enable", id] => report(library.set_service_boot(id, true)?),
        ["disable", id] => report(library.set_service_boot(id, false)?),
        ["logs", id, follow @ ..] if follow.is_empty() || follow == ["-f"] => {
            library.service(id)?;
            let mut journal = std::process::Command::new("journalctl");
            journal.args(["--no-pager", "-u", &services::unit_name(id)]).args(if follow.is_empty() { &["-n", "200"][..] } else { &["-f"][..] });
            let status = journal.status().map_err(|e| format!("cannot run journalctl: {e}"))?;
            if status.success() { Ok(()) } else { Err(format!("journalctl failed ({status})")) }
        }
        // Run by gizlet-services.service at boot.
        ["boot"] => {
            let started = library.boot_services()?;
            if !started.is_empty() {
                println!("started {}", started.join(" "));
            }
            Ok(())
        }
        // The ExecStart of each service's unit; returns only on failure.
        ["exec", id] => Err(library.exec_service(id)),
        _ => Err(format!("wrong arguments for service\n{USAGE}")),
    }
}

fn report(service: Service) -> Result<(), String> {
    let boot = if service.boot { "starts at boot" } else { "does not start at boot" };
    println!("{}: {}, {boot}", service.id(), service.state.word());
    Ok(())
}

fn print_status(library: &Library, service: &Service) {
    let m = &service.package.manifest;
    let id = m.id.as_str();
    println!("{id} {} ({})", m.version, m.name);
    match service.pid {
        Some(pid) => println!("state: {} (pid {pid})", service.state.word()),
        None => println!("state: {}", service.state.word()),
    }
    println!("starts at boot: {}", if service.boot { "yes" } else { "no" });
    println!("unit: {}", services::unit_name(id));
    println!("package: {}", service.package.dir.display());
    println!("state folder: {}", library.service_data(id).display());
    println!("runtime folder: {}", library.service_runtime(id).display());
    println!("log: gizlet service logs {id}");
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
