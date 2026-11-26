//! Runs one command in a libkrun microVM and exits with its status.
//!
//! krun-vm --krunfw LIB (--root DIR | --root-disk SQUASHFS) [--cpus N] [--mem MIB]
//!         [--inputs MANIFEST] [--outputs DIR] [--env NAME=VALUE]... -- COMMAND [ARG]...
//!
//! KRUN_VM_DEBUG in the host environment enables libkrun and guest kernel logs.
//!
//! MANIFEST lists "SOURCE<TAB>DESTINATION" lines. Each source is resolved on the
//! host and linked, or copied, into a private staging directory exposed to the
//! guest as the virtio-fs tag "in". Bazel's execroot is a symlink forest whose
//! absolute targets do not exist in the guest, so it is never shared directly.
//! DIR is exposed as the tag "out". The guest command mounts both tags.
//!
//! The guest's exit status reaches the VMM only for some root kinds. With
//! --inputs the guest also writes its decimal status to /in/.krun-status, and
//! that value becomes this process's status. A missing status is a failure.

use std::ffi::CString;
use std::fs;
use std::io::{self, BufRead};
use std::os::fd::AsFd;
use std::path::{Component, Path, PathBuf};
use std::process::exit;

use krun::{BlockDevice, ConsoleDevice, DiskFormat, FsDevice, FsOverlay, MmioDeviceManager, Payload, VmmBuilder};

const USAGE: &str = "usage: krun-vm --krunfw LIB (--root DIR | --root-disk SQUASHFS) [--cpus N] [--mem MIB]
               [--inputs MANIFEST] [--outputs DIR] [--env NAME=VALUE]... -- COMMAND [ARG]...";

#[derive(Default)]
struct Options {
    krunfw: Option<String>,
    root: Option<String>,
    root_disk: Option<String>,
    inputs: Option<String>,
    outputs: Option<String>,
    cpus: u8,
    mem: u32,
    env: Vec<String>,
    command: Vec<String>,
}

fn usage() -> ! {
    eprintln!("{USAGE}");
    exit(2)
}

fn fail(what: &str, error: impl std::fmt::Display) -> ! {
    eprintln!("krun-vm: {what}: {error}");
    exit(1)
}

fn parse() -> Options {
    let mut options = Options { cpus: 2, mem: 2048, ..Default::default() };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--" {
            options.command = args.by_ref().collect();
            break;
        }
        let value = args.next().unwrap_or_else(|| usage());
        match arg.as_str() {
            "--krunfw" => options.krunfw = Some(value),
            "--root" => options.root = Some(value),
            "--root-disk" => options.root_disk = Some(value),
            "--inputs" => options.inputs = Some(value),
            "--outputs" => options.outputs = Some(value),
            "--cpus" => options.cpus = value.parse().ok().filter(|&n| n > 0).unwrap_or_else(|| usage()),
            "--mem" => options.mem = value.parse().ok().filter(|&n| n > 0).unwrap_or_else(|| usage()),
            "--env" if value.contains('=') => options.env.push(value),
            _ => usage(),
        }
    }
    if options.krunfw.is_none()
        || options.root.is_some() == options.root_disk.is_some()
        || options.command.is_empty()
        || (options.inputs.is_some() && options.outputs.is_none())
    {
        usage();
    }
    options
}

fn safe_destination(path: &str) -> bool {
    !path.is_empty()
        && !path.contains("//")
        && !path.ends_with('/')
        && Path::new(path).components().all(|c| matches!(c, Component::Normal(_)))
}

fn resolve(path: &str) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|e| fail(&format!("resolve {path}"), e))
}

fn stage_inputs(manifest: &str, staging: &Path) {
    let file = fs::File::open(manifest).unwrap_or_else(|e| fail(&format!("open {manifest}"), e));
    for line in io::BufReader::new(file).lines() {
        let line = line.unwrap_or_else(|e| fail("read manifest", e));
        let Some((source, destination)) = line.split_once('\t').filter(|(_, d)| safe_destination(d)) else {
            eprintln!("krun-vm: invalid manifest line: {line}");
            exit(2);
        };
        let source = resolve(source);
        let target = staging.join(destination);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).unwrap_or_else(|e| fail(&format!("mkdir {}", parent.display()), e));
        }
        if fs::hard_link(&source, &target).is_err() {
            fs::copy(&source, &target).unwrap_or_else(|e| fail(&format!("copy {}", source.display()), e));
        }
    }
}

fn make_staging(outputs: &str) -> PathBuf {
    let template = CString::new(format!("{outputs}/.krun-in.XXXXXX")).unwrap();
    let raw = template.into_raw();
    let result = unsafe { libc::mkdtemp(raw) };
    let path = unsafe { CString::from_raw(raw) };
    if result.is_null() {
        fail("mkdtemp", io::Error::last_os_error());
    }
    PathBuf::from(path.into_string().unwrap())
}

fn wait_for(child: libc::pid_t, staging: Option<&Path>) -> ! {
    let mut status = 0;
    while unsafe { libc::waitpid(child, &mut status, 0) } < 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            fail("wait", error);
        }
    }
    let mut code = if libc::WIFEXITED(status) { libc::WEXITSTATUS(status) } else { 128 + libc::WTERMSIG(status) };
    if let Some(staging) = staging {
        match fs::read_to_string(staging.join(".krun-status")).ok().and_then(|s| s.trim().parse::<u8>().ok()) {
            Some(reported) => code = reported.into(),
            None => {
                eprintln!("krun-vm: the guest reported no exit status (VM status {code})");
                code = if code == 0 { 1 } else { code };
            }
        }
        fs::remove_dir_all(staging).unwrap_or_else(|e| fail(&format!("remove {}", staging.display()), e));
    }
    exit(code)
}

fn run_vm(options: &Options, staging: Option<&Path>) -> ! {
    // libkrun opens libkrunfw by its install name. Loading the pinned build first
    // makes that lookup resolve to it instead of searching the host.
    let krunfw = CString::new(resolve(options.krunfw.as_ref().unwrap()).into_os_string().into_string().unwrap()).unwrap();
    if unsafe { libc::dlopen(krunfw.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) }.is_null() {
        fail("load libkrunfw", krunfw.to_string_lossy());
    }

    let debug = std::env::var_os("KRUN_VM_DEBUG").is_some();
    if debug {
        // SAFETY: standard error stays open for the life of the process.
        let stderr = unsafe { std::os::fd::BorrowedFd::borrow_raw(2) };
        krun::init_log(Some(stderr), krun::LogLevel::Debug, krun::LogStyle::Never, krun::LogOptions::empty())
            .unwrap_or_else(|e| fail("log", e));
    }
    let mut payload = Payload::load_krunfw().unwrap_or_else(|e| fail("load the guest kernel", e));
    let mut builder = krun_init::Config::builder().args(&options.command.iter().map(String::as_str).collect::<Vec<_>>());
    for variable in &options.env {
        builder = builder.env_var(variable);
    }
    builder = builder.workdir("/");
    if options.root_disk.is_some() {
        builder = builder.set_root_disk_remount("/dev/vda", Some("squashfs"), Some("ro"));
    }
    let config = builder.build();
    let mut overlay = FsOverlay::new();
    if options.root_disk.is_some() {
        // The temporary root is an empty virtio-fs device. Provide the mount
        // points init uses before it pivots to the disk.
        for dir in ["dev", "dev/pts", "dev/shm", "proc", "sys", "newroot"] {
            overlay.add_dir(dir, 0o755).unwrap_or_else(|e| fail("root overlay", e));
        }
    }
    config.apply(&mut overlay, &mut payload).unwrap_or_else(|e| fail("apply init configuration", e));

    let mut devices = MmioDeviceManager::new();
    let stdin = io::stdin();
    let stdout = io::stdout();
    let stderr = io::stderr();
    let mut console = ConsoleDevice::builder();
    console
        .add_default_console(Some(stdin.as_fd()), Some(stdout.as_fd()), Some(stderr.as_fd()))
        .unwrap_or_else(|e| fail("console", e));
    devices.add(console.build().unwrap_or_else(|e| fail("console", e)));

    let mut root = match &options.root {
        Some(dir) => FsDevice::new("/dev/root", resolve(dir).to_str().unwrap()),
        None => FsDevice::new_null("/dev/root"),
    }
    .unwrap_or_else(|e| fail("root filesystem", e));
    root.set_overlay(overlay);
    devices.add(root);
    if let Some(disk) = &options.root_disk {
        let mut block = BlockDevice::new("root", resolve(disk).to_str().unwrap(), DiskFormat::Raw)
            .unwrap_or_else(|e| fail("root disk", e));
        block.set_read_only(true);
        devices.add(block);
    }
    for (tag, dir) in [("in", staging.map(Path::to_path_buf)), ("out", options.outputs.as_deref().map(resolve))] {
        if let Some(dir) = dir {
            devices.add(FsDevice::new(tag, dir.to_str().unwrap()).unwrap_or_else(|e| fail("share", e)));
        }
    }

    let vmm = VmmBuilder::new()
        .vcpus(options.cpus)
        .and_then(|b| b.ram_mib(options.mem))
        .map(|b| b.payload(payload).devices(devices))
        .map(|b| if debug { b.set_kernel_console("hvc0") } else { b })
        .and_then(|b| b.build())
        .unwrap_or_else(|e| fail("build the VM", e));
    // The VMM exits the process with the guest's status when the guest stops.
    vmm.run();
    exit(1)
}

fn main() {
    let options = parse();
    // Stage next to the outputs, on the same filesystem as Bazel's output base,
    // so that most inputs are hard links rather than copies.
    let staging = options.inputs.as_ref().map(|manifest| {
        let staging = make_staging(options.outputs.as_ref().unwrap());
        stage_inputs(manifest, &staging);
        staging
    });
    // Run the VM in a child so this process can remove the staging tree and
    // forward the guest's status.
    match unsafe { libc::fork() } {
        -1 => fail("fork", io::Error::last_os_error()),
        0 => run_vm(&options, staging.as_deref()),
        child => wait_for(child, staging.as_deref()),
    }
}
