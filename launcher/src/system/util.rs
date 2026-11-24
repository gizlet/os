use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Runs `program args` and returns its stdout when it exits successfully
/// within `limit`; a hung tool is killed. Both pipes are read while it runs,
/// so a tool that prints more than a pipe holds does not stall.
pub fn run_for(limit: Duration, program: &str, args: &[&str]) -> Result<String, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut p) = pipe {
                let mut bytes = Vec::new();
                let _ = p.read_to_end(&mut bytes);
                text = String::from_utf8_lossy(&bytes).into_owned();
            }
            text
        })
    };
    let out = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < limit => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} did not finish in {} s", limit.as_secs()));
            }
            Err(e) => return Err(format!("{program}: {e}")),
        }
    };
    let out = out.join().unwrap_or_default();
    let err = err.join().unwrap_or_default();
    if status.success() {
        return Ok(out);
    }
    let msg = strip_ansi(if err.trim().is_empty() { out.trim() } else { err.trim() });
    Err(if msg.is_empty() { format!("{program} failed ({status})") } else { msg })
}

pub fn run(program: &str, args: &[&str]) -> Result<String, String> {
    run_for(Duration::from_secs(10), program, args)
}

#[allow(dead_code)]
pub fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
        .unwrap_or(false)
}

/// Removes terminal escape sequences.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Splits a table row on runs of two or more spaces.
#[allow(dead_code)]
pub fn columns(line: &str) -> Vec<String> {
    line.split("  ").map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_ansi_and_splits_columns() {
        assert_eq!(strip_ansi("\u{1b}[1;90m>\u{1b}[0m Home"), "> Home");
        assert_eq!(columns("    Home Net        psk      ****"), ["Home Net", "psk", "****"]);
    }

    #[cfg(unix)]
    #[test]
    fn reads_output_larger_than_a_pipe() {
        // 1 MiB on stdout and on stderr, far over a 64 KiB pipe buffer.
        let script = "head -c 1048576 /dev/zero; head -c 1048576 /dev/zero >&2";
        let out = run_for(Duration::from_secs(10), "sh", &["-c", script]).unwrap();
        assert_eq!(out.len(), 1 << 20);
    }

    #[cfg(unix)]
    #[test]
    fn kills_a_hung_command() {
        let started = Instant::now();
        assert!(run_for(Duration::from_millis(200), "sleep", &["5"]).is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(run("echo", &["hi"]).unwrap(), "hi\n");
    }
}
