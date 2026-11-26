//! Runs an app and ends it when the player asks to go back to the launcher,
//! whether or not the app has a way out of its own: holding Home (the R36S's
//! FN key) for a second, or Select and Start together for two seconds.
//!
//! The controls are read from evdev without grabbing them, so the app sees
//! them too. The app gets SIGTERM, then SIGKILL if it is still there three
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
pub fn run(cmd: &mut Command) -> io::Result<ExitStatus> {
    let mut child = cmd.spawn()?;
    let mut pad = pad::Pad::open();
    let mut exit = Exit::default();
    let mut termed: Option<Instant> = None;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        let held = pad.wait(Duration::from_millis(100));
        let now = Instant::now();
        match termed {
            None if exit.update(held, now) => {
                eprintln!("gizlet: back to the launcher, ending the app");
                terminate(&child);
                termed = Some(now);
            }
            Some(t) if now - t >= KILL_AFTER => {
                let _ = child.kill();
                return child.wait();
            }
            _ => {}
        }
    }
}

fn terminate(child: &Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    #[cfg(not(unix))]
    let _ = child;
}

#[cfg(target_os = "linux")]
mod pad {
    //! Every `/dev/input/event*` device that has gamepad buttons, read
    //! without grabbing and rescanned every two seconds for hotplug.
    use super::Held;
    use std::collections::HashMap;
    use std::fs::File;
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    const EV_KEY: u16 = 0x01;
    const BTN_SELECT: u16 = 0x13a;
    const BTN_START: u16 = 0x13b;
    const BTN_MODE: u16 = 0x13c;
    /// `struct input_event` on 64-bit Linux: a timeval, then type, code and value.
    const EVENT: usize = 24;

    #[derive(Default)]
    pub struct Pad {
        devices: HashMap<PathBuf, (File, Held)>,
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
                let is_event = e.file_name().to_string_lossy().starts_with("event");
                if !is_event || self.devices.contains_key(&path) {
                    continue;
                }
                let file = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(&path);
                if let Ok(file) = file {
                    self.devices.insert(path, (file, Held::default()));
                }
            }
        }

        /// Waits up to `timeout` for input, then returns what is held.
        pub fn wait(&mut self, timeout: Duration) -> Held {
            if self.scanned.is_none_or(|t| t.elapsed() >= Duration::from_secs(2)) {
                self.scan();
            }
            let mut fds: Vec<libc::pollfd> = self
                .devices
                .values()
                .map(|(f, _)| libc::pollfd { fd: f.as_raw_fd(), events: libc::POLLIN, revents: 0 })
                .collect();
            if fds.is_empty() {
                std::thread::sleep(timeout);
            } else {
                unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout.as_millis() as i32) };
            }
            let mut gone = Vec::new();
            for (path, (file, held)) in &mut self.devices {
                let mut buf = [0u8; EVENT * 64];
                loop {
                    match file.read(&mut buf) {
                        Ok(n) if n >= EVENT => {
                            for ev in buf[..n - n % EVENT].chunks(EVENT) {
                                apply(held, ev);
                            }
                        }
                        Ok(0) | Err(_) if !path.exists() => {
                            gone.push(path.clone());
                            break;
                        }
                        _ => break,
                    }
                }
            }
            for path in gone {
                self.devices.remove(&path);
            }
            self.devices.values().fold(Held::default(), |a, (_, h)| Held {
                home: a.home || h.home,
                select: a.select || h.select,
                start: a.start || h.start,
            })
        }
    }

    fn apply(held: &mut Held, ev: &[u8]) {
        let kind = u16::from_ne_bytes([ev[16], ev[17]]);
        let code = u16::from_ne_bytes([ev[18], ev[19]]);
        let down = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]) != 0;
        if kind != EV_KEY {
            return;
        }
        match code {
            BTN_MODE => held.home = down,
            BTN_SELECT => held.select = down,
            BTN_START => held.start = down,
            _ => {}
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

    #[cfg(unix)]
    #[test]
    fn returns_the_exit_status() {
        let status = run(Command::new("/bin/sh").args(["-c", "exit 3"])).unwrap();
        assert_eq!(status.code(), Some(3));
    }
}
