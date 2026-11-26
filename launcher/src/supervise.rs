//! Runs an app and ends it when the player asks to go back to the launcher,
//! whether or not the app has a way out of its own: holding Home (the R36S's
//! FN key) for a second, or Select and Start together for two seconds.
//!
//! Each input device is asked which buttons it holds down, so an app that
//! grabs its controls still cannot hide these. The app gets SIGTERM, then SIGKILL if it is still there three
//! seconds later. Apps that save progress should do so on SIGTERM.

use std::io;
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

pub const HOME_HOLD: Duration = Duration::from_secs(1);
pub const SELECT_START_HOLD: Duration = Duration::from_secs(2);
const KILL_AFTER: Duration = Duration::from_secs(3);

/// Which of the exit buttons are down, across every device.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Held {
    pub home: bool,
    pub select: bool,
    pub start: bool,
}

/// Decides when to end the app from how long the exit buttons have been held.
#[derive(Default)]
pub struct Exit {
    home_since: Option<Instant>,
    combo_since: Option<Instant>,
}

impl Exit {
    /// True once Home or Select and Start have been held long enough.
    pub fn update(&mut self, held: Held, now: Instant) -> bool {
        let since = |slot: &mut Option<Instant>, down: bool| {
            if down {
                *slot.get_or_insert(now)
            } else {
                *slot = None;
                now
            }
        };
        let home = since(&mut self.home_since, held.home);
        let combo = since(&mut self.combo_since, held.select && held.start);
        (held.home && now - home >= HOME_HOLD) || (held.select && held.start && now - combo >= SELECT_START_HOLD)
    }
}

/// Runs `cmd` until it exits, ending it when the player asks.
///
/// The app gets a process group of its own, in the terminal's foreground,
/// so ending it reaches every program it started, as when its entry is a
/// script such as `game & wait`. Once the entry exits, whatever is left of
/// its group is ended too, so nothing keeps the display or the app's lock.
pub fn run(cmd: &mut Command) -> io::Result<ExitStatus> {
    // Whatever happens, even a spawn that fails after the child took the
    // terminal, this process's group gets it back.
    let _terminal = Terminal;
    let mut child = spawn(cmd)?;
    let mut pad = pad::Pad::open();
    let mut exit = Exit::default();
    let mut termed: Option<Instant> = None;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        let held = pad.wait(Duration::from_millis(100));
        let now = Instant::now();
        match termed {
            None if exit.update(held, now) => {
                eprintln!("gizlet: back to the launcher, ending the app");
                signal_group(&child, Signal::Term);
                termed = Some(now);
            }
            Some(t) if now - t >= KILL_AFTER => {
                signal_group(&child, Signal::Kill);
                break child.wait()?;
            }
            _ => {}
        }
    };
    end_group(&child, termed);
    Ok(status)
}

/// Gives the terminal back to this process's group when dropped.
struct Terminal;

impl Drop for Terminal {
    fn drop(&mut self) {
        take_terminal();
    }
}

#[derive(Clone, Copy)]
enum Signal {
    Term,
    Kill,
}

/// Starts `cmd` as the leader of a new process group.
fn spawn(cmd: &mut Command) -> io::Result<Child> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // In the child, before exec: a group of its own, made the terminal's
        // foreground group so the app can still change the terminal's modes.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setpgid(0, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::isatty(0) == 1 {
                    let old = libc::signal(libc::SIGTTOU, libc::SIG_IGN);
                    libc::tcsetpgrp(0, libc::getpid());
                    libc::signal(libc::SIGTTOU, old);
                }
                Ok(())
            });
        }
    }
    cmd.spawn()
}

/// Sends a signal to the app's whole group; false when nothing is left in it.
fn signal_group(child: &Child, signal: Signal) -> bool {
    #[cfg(unix)]
    {
        let sig = match signal {
            Signal::Term => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
        };
        unsafe { libc::kill(-(child.id() as i32), sig) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = (child, signal);
        false
    }
}

/// After the entry exits: ends what is left of its group, with SIGTERM, then
/// SIGKILL once `KILL_AFTER` has passed since the first SIGTERM.
fn end_group(child: &Child, termed: Option<Instant>) {
    if !signal_group(child, Signal::Term) {
        return;
    }
    let deadline = termed.unwrap_or_else(Instant::now) + KILL_AFTER;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
        if !alive(child) {
            return;
        }
    }
    signal_group(child, Signal::Kill);
}

fn alive(child: &Child) -> bool {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), 0) == 0
    }
    #[cfg(not(unix))]
    {
        let _ = child;
        false
    }
}

/// Makes this process's group the terminal's foreground again.
fn take_terminal() {
    #[cfg(unix)]
    unsafe {
        if libc::isatty(0) == 1 {
            let old = libc::signal(libc::SIGTTOU, libc::SIG_IGN);
            libc::tcsetpgrp(0, libc::getpgrp());
            libc::signal(libc::SIGTTOU, old);
        }
    }
}

/// The exit buttons held in an `EVIOCGKEY` bitmap: one bit per key code.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn held_from_keys(bits: &[u8]) -> Held {
    let down = |code: usize| bits.get(code / 8).is_some_and(|b| b & (1 << (code % 8)) != 0);
    let (home, select, start) = (0x13c, 0x13a, 0x13b);
    Held { home: down(home), select: down(select), start: down(start) }
}

#[cfg(target_os = "linux")]
mod pad {
    //! Every `/dev/input/event*` device, asked for the buttons it holds down
    //! (`EVIOCGKEY`) rather than read for events: an app that grabs a device
    //! takes its events, but not this, so it cannot hide the exit buttons.
    //! Devices are rescanned every two seconds for hotplug.
    use super::Held;
    use std::collections::HashMap;
    use std::fs::File;
    use std::os::fd::AsRawFd;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    const KEY_BYTES: usize = 0x300 / 8;
    /// `EVIOCGKEY(len)`: `_IOC(_IOC_READ, 'E', 0x18, len)`.
    const EVIOCGKEY: libc::c_ulong = (2 << 30) | ((KEY_BYTES as libc::c_ulong) << 16) | (0x45 << 8) | 0x18;

    #[derive(Default)]
    pub struct Pad {
        devices: HashMap<PathBuf, File>,
        scanned: Option<Instant>,
    }

    impl Pad {
        pub fn open() -> Pad {
            let mut pad = Pad::default();
            pad.scan();
            pad
        }

        fn scan(&mut self) {
            self.scanned = Some(Instant::now());
            self.devices.retain(|path, _| path.exists());
            let Ok(dir) = std::fs::read_dir("/dev/input") else { return };
            for e in dir.flatten() {
                let path = e.path();
                if e.file_name().to_string_lossy().starts_with("event") && !self.devices.contains_key(&path) {
                    if let Ok(file) = File::open(&path) {
                        self.devices.insert(path, file);
                    }
                }
            }
        }

        /// Sleeps for `timeout`, then returns what is held on any device.
        pub fn wait(&mut self, timeout: Duration) -> Held {
            std::thread::sleep(timeout);
            if self.scanned.is_none_or(|t| t.elapsed() >= Duration::from_secs(2)) {
                self.scan();
            }
            let mut held = Held::default();
            let mut gone = Vec::new();
            for (path, file) in &self.devices {
                let mut bits = [0u8; KEY_BYTES];
                if unsafe { libc::ioctl(file.as_raw_fd(), EVIOCGKEY as _, bits.as_mut_ptr()) } < 0 {
                    gone.push(path.clone());
                    continue;
                }
                let h = super::held_from_keys(&bits);
                held.home |= h.home;
                held.select |= h.select;
                held.start |= h.start;
            }
            for path in gone {
                self.devices.remove(&path);
            }
            held
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod pad {
    //! Elsewhere the app runs in its own window and closes as any other.
    use super::Held;
    use std::time::Duration;

    pub struct Pad;

    impl Pad {
        pub fn open() -> Pad {
            Pad
        }

        pub fn wait(&mut self, timeout: Duration) -> Held {
            std::thread::sleep(timeout);
            Held::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_end_the_app() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut exit = Exit::default();
        let home = Held { home: true, ..Held::default() };
        assert!(!exit.update(home, at(0)));
        assert!(!exit.update(home, at(900)));
        assert!(exit.update(home, at(1000)));

        // Letting go starts over; Select alone does nothing.
        let mut exit = Exit::default();
        let both = Held { select: true, start: true, ..Held::default() };
        assert!(!exit.update(both, at(0)));
        assert!(!exit.update(Held { select: true, ..Held::default() }, at(1500)));
        assert!(!exit.update(both, at(1600)));
        assert!(!exit.update(both, at(3500)));
        assert!(exit.update(both, at(3600)));
    }

    /// Run in a process of its own, on a pseudo-terminal it controls (see
    /// `failed_spawn_gives_the_terminal_back`): an app that cannot start must
    /// not leave the terminal to its dead process group.
    #[cfg(unix)]
    #[test]
    fn pty_child_failed_spawn() {
        if std::env::var_os("GIZLET_PTY_CHILD").is_none() {
            return;
        }
        let err = run(&mut Command::new("/nonexistent/gizlet-app")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        let (fg, own) = unsafe { (libc::tcgetpgrp(0), libc::getpgrp()) };
        assert_eq!(fg, own, "the terminal stayed with the app's group");
    }

    #[cfg(unix)]
    #[test]
    fn failed_spawn_gives_the_terminal_back() {
        use std::os::unix::process::CommandExt;
        let (mut leader, mut follower) = (0, 0);
        let ok = unsafe {
            libc::openpty(&mut leader, &mut follower, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
        };
        assert_eq!(ok, 0, "openpty");
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "supervise::tests::pty_child_failed_spawn", "--test-threads=1"])
            .env("GIZLET_PTY_CHILD", "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        unsafe {
            cmd.pre_exec(move || {
                // A new session whose controlling terminal is the pseudo-terminal, on stdin.
                if libc::setsid() < 0
                    || libc::ioctl(follower, libc::TIOCSCTTY as _, 0) < 0
                    || libc::dup2(follower, 0) < 0
                {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let status = cmd.status().unwrap();
        unsafe {
            libc::close(leader);
            libc::close(follower);
        }
        assert!(status.success(), "the terminal was not given back after a failed spawn");
    }

    #[test]
    fn reads_the_key_bitmap() {
        let mut bits = [0u8; 0x300 / 8];
        assert_eq!(held_from_keys(&bits), Held::default());
        bits[0x13c / 8] |= 1 << (0x13c % 8);
        assert_eq!(held_from_keys(&bits), Held { home: true, ..Held::default() });
        bits[0x13a / 8] |= 1 << (0x13a % 8);
        bits[0x13b / 8] |= 1 << (0x13b % 8);
        assert_eq!(held_from_keys(&bits), Held { home: true, select: true, start: true });
        // A short bitmap, as from a device without these buttons, holds none.
        assert_eq!(held_from_keys(&bits[..8]), Held::default());
    }

    #[cfg(unix)]
    fn gone(pid: i32) -> bool {
        // Orphans are reaped by init; a zombie counts as gone.
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let state = std::process::Command::new("ps").args(["-o", "stat=", "-p", &pid.to_string()]).output().unwrap();
            let state = String::from_utf8_lossy(&state.stdout);
            if state.trim().is_empty() || state.trim().starts_with('Z') {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    #[cfg(unix)]
    #[test]
    fn ends_what_a_wrapper_script_left() {
        let dir = std::env::temp_dir().join(format!("gizlet-supervise-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pid_file = dir.join("pid");
        // The entry starts the game in the background and exits at once.
        let status = run(Command::new("/bin/sh").args(["-c", "sleep 60 & echo $! > \"$1\"", "sh"]).arg(&pid_file)).unwrap();
        assert!(status.success());
        let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
        assert!(gone(pid), "the background program outlived its entry");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn ending_reaches_the_whole_group() {
        let dir = std::env::temp_dir().join(format!("gizlet-supervise-group-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pid_file = dir.join("pid");
        // As the exit gesture does, while the entry waits for its game.
        let mut child = spawn(Command::new("/bin/sh").args(["-c", "sleep 60 & echo $! > \"$1\"; wait", "sh"]).arg(&pid_file)).unwrap();
        while std::fs::read_to_string(&pid_file).map_or(true, |s| !s.ends_with('\n')) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
        assert!(signal_group(&child, Signal::Term));
        child.wait().unwrap();
        end_group(&child, Some(Instant::now()));
        assert!(gone(pid), "the game outlived the exit");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn returns_the_exit_status() {
        let status = run(Command::new("/bin/sh").args(["-c", "exit 3"])).unwrap();
        assert_eq!(status.code(), Some(3));
    }
}
