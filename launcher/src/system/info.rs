//! What the System page shows about the machine.

use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct Info {
    pub model: Option<String>,
    pub os: String,
    pub kernel: String,
    pub arch: String,
    pub cpus: usize,
    pub memory_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
pub struct Storage {
    pub total: u64,
    pub free: u64,
}

fn read_trim(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim_matches(|c: char| c.is_whitespace() || c == '\0').to_string())
        .filter(|s| !s.is_empty())
}

/// The name the system has now.
pub fn hostname() -> String {
    #[cfg(unix)]
    return uname().2;
    #[cfg(windows)]
    return std::env::var("COMPUTERNAME").unwrap_or_default();
    #[cfg(not(any(unix, windows)))]
    return String::new();
}

#[cfg(unix)]
fn uname() -> (String, String, String) {
    let mut u: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut u) } != 0 {
        return Default::default();
    }
    let s = |f: &[libc::c_char]| unsafe { std::ffi::CStr::from_ptr(f.as_ptr()) }.to_string_lossy().into_owned();
    (s(&u.sysname), s(&u.release), s(&u.nodename))
}

#[cfg(target_os = "macos")]
fn sysctl(name: &str) -> Option<String> {
    super::util::run("sysctl", &["-n", name]).ok().map(|s| s.trim().to_string())
}

impl Info {
    pub fn detect() -> Info {
        let mut info = Info {
            arch: std::env::consts::ARCH.to_string(),
            cpus: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
            ..Info::default()
        };
        #[cfg(unix)]
        {
            let (sysname, release, _) = uname();
            info.kernel = format!("{sysname} {release}");
        }
        #[cfg(target_os = "linux")]
        {
            info.model = read_trim("/sys/firmware/devicetree/base/model")
                .or_else(|| read_trim("/sys/class/dmi/id/product_name"));
            info.os = std::fs::read_to_string("/etc/os-release")
                .ok()
                .and_then(|t| {
                    t.lines()
                        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                        .map(|v| v.trim_matches('"').to_string())
                })
                .unwrap_or_else(|| "Linux".into());
            info.memory_bytes = std::fs::read_to_string("/proc/meminfo").ok().and_then(|t| {
                let kb: u64 = t.lines().find_map(|l| l.strip_prefix("MemTotal:"))?.trim().trim_end_matches("kB").trim().parse().ok()?;
                Some(kb * 1024)
            });
        }
        #[cfg(target_os = "macos")]
        {
            info.model = sysctl("hw.model");
            info.os = super::util::run("sw_vers", &["-productVersion"])
                .map(|v| format!("macOS {}", v.trim()))
                .unwrap_or_else(|_| "macOS".into());
            info.memory_bytes = sysctl("hw.memsize").and_then(|v| v.parse().ok());
        }
        #[cfg(windows)]
        {
            info.os = "Windows".into();
        }
        let _ = read_trim;
        info
    }
}

/// The first IPv4 address of an interface that is up, other than loopback.
#[cfg(unix)]
pub fn local_ip() -> Option<String> {
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut list) } != 0 {
        return None;
    }
    let mut found = None;
    let mut cur = list;
    while !cur.is_null() {
        let ifa = unsafe { &*cur };
        cur = ifa.ifa_next;
        let up = ifa.ifa_flags & libc::IFF_UP as u32 != 0;
        let loopback = ifa.ifa_flags & libc::IFF_LOOPBACK as u32 != 0;
        if !up || loopback || ifa.ifa_addr.is_null() {
            continue;
        }
        if unsafe { (*ifa.ifa_addr).sa_family } as i32 != libc::AF_INET {
            continue;
        }
        let sin = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in) };
        found = Some(std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)).to_string());
        break;
    }
    unsafe { libc::freeifaddrs(list) };
    found
}

#[cfg(not(unix))]
pub fn local_ip() -> Option<String> {
    None
}

#[cfg(unix)]
pub fn storage(root: &Path) -> Option<Storage> {
    let mut dir = root;
    // The folder may not exist yet; report the disk it will be on.
    while !dir.exists() {
        dir = dir.parent()?;
    }
    let c = std::ffi::CString::new(dir.as_os_str().as_encoded_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let frag = st.f_frsize as u64;
    Some(Storage { total: st.f_blocks as u64 * frag, free: st.f_bavail as u64 * frag })
}

#[cfg(not(unix))]
pub fn storage(_root: &Path) -> Option<Storage> {
    None
}
