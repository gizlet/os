"""Check the Gizlet OS root and data filesystems of a card image.

Usage: rootfs_test.py IMAGE BOOTER CONFIG_JSON

CONFIG_JSON names the data filesystem UUID and the license notices required
under /usr/share/licenses.
"""

import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).parent))
import card  # noqa: E402


def run(*command, **options):
    print("+", " ".join(map(str, command)), flush=True)
    return subprocess.run(command, check=True, text=True, **options)


def output(*command):
    return run(*command, capture_output=True).stdout


def extract(image, offset, size, path):
    with open(image, "rb") as source, open(path, "wb") as target:
        source.seek(offset)
        target.write(source.read(size))


def main():
    image, booter, config = sys.argv[1], Path(sys.argv[2]), json.loads(sys.argv[3])
    parts, _ = card.partitions(image)
    work = Path(tempfile.mkdtemp(prefix="rootfs-test-"))

    data = work / "data.ext4"
    extract(image, *parts["data"][1:], data)
    run("e2fsck", "-nf", data)
    assert output("blkid", "-p", "-s", "UUID", "-o", "value", data).strip() == config["data_uuid"], "data UUID"
    def names(directory):
        return {line.split("/")[5] for line in output("debugfs", "-R", f"ls -p {directory}", data).splitlines() if line.count("/") >= 5}
    # A fresh card holds only Gizlet runtime 1, a SquashFS file mounted at boot.
    expected = {"/gizlet": {"runtime"}, "/gizlet/runtime": {"1.sqfs"}, "/home": set()}
    for directory, entries in expected.items():
        assert "Type: directory" in output("debugfs", "-R", f"stat {directory}", data), directory
        assert names(directory) - {".", ".."} == entries, f"{directory} holds {sorted(names(directory))}"
    runtime = work / "runtime-1.sqfs"
    run("debugfs", "-R", f"dump /gizlet/runtime/1.sqfs {runtime}", data)
    listing = output("unsquashfs", "-l", runtime)
    for path in ("lib/ld-linux-aarch64.so.1", "lib/libEGL.so.1", "lib/libGLESv2.so.2", "lib/libgbm.so.1",
                 "share/glvnd/egl_vendor.d/50_mesa.json", "bin/gizlet-run", "bin/gizlet-gl-info"):
        assert f"squashfs-root/{path}\n" in listing, f"runtime 1 lacks {path}"
    assert any("_dri.so" in line and "panfrost" in line for line in listing.splitlines()), "runtime 1 lacks Panfrost"

    squashfs = work / "root.sqfs"
    extract(image, *parts["root"][1:], squashfs)
    root = work / "root"
    run("unsquashfs", "-no-progress", "-d", root, squashfs, stdout=subprocess.DEVNULL)
    # Debian with systemd as init; links are read, not followed, as they point into the card.
    assert os.readlink(root / "usr/sbin/init").endswith("/systemd"), "/sbin/init is systemd"
    systemd = root / "etc/systemd/system"
    for wants in ("local-fs.target.wants/gizlet-var.service", "local-fs.target.wants/gizlet-data.service",
                  "multi-user.target.wants/gizlet-session.service", "multi-user.target.wants/gizlet-boot-log.service",
                  "multi-user.target.wants/gizlet-switches.service",
                  "timers.target.wants/gizlet-late-log.timer"):
        assert (systemd / wants).is_symlink(), f"{wants} is enabled"
    # WiFi and ssh start at boot only when turned on in the launcher (gizlet-switch).
    for wants in ("multi-user.target.wants/ssh.service", "multi-user.target.wants/iwd.service"):
        assert not (systemd / wants).is_symlink(), f"{wants} starts at boot"
    assert (root / "usr/lib/systemd/system/ssh.service.d/gizlet.conf").is_file(), "ssh.service uses gizlet-ssh"
    for unit in ("getty@.service", "serial-getty@.service", "systemd-firstboot.service"):
        assert os.readlink(systemd / unit) == "/dev/null", f"{unit} is masked"
    # Each card creates its own identity at boot.
    assert not list((root / "etc/ssh").glob("ssh_host_*")), "the image carries ssh host keys"
    assert (root / "etc/machine-id").read_text() == "", "the image carries a machine ID"
    for script in ("boot-session", "gizlet-boot-device", "gizlet-mount-data", "gizlet-ssh", "gizlet-setup", "gizlet-switch",
                   "gizlet-var", "save-boot-log", "gizlet-booter", "gizlet-launcher"):
        assert (root / "usr/lib/gizlet" / script).stat().st_mode & 0o111, f"/usr/lib/gizlet/{script} is executable"
    # First boot grows the data partition and its filesystem.
    for tool in ("usr/sbin/sfdisk", "usr/bin/partx", "usr/sbin/resize2fs"):
        assert (root / tool).exists(), f"/{tool}"
    for directory in ("opt/gizlet", "home"):
        assert (root / directory).is_dir(), f"/{directory}"
    for notice in config["licenses"]:
        assert (root / "usr/share/licenses" / notice).stat().st_size > 0, f"license notice {notice}"
    for package in ("libc6", "systemd", "openssh-server", "iwd"):
        assert (root / "usr/share/doc" / package / "copyright").stat().st_size > 0, f"{package} copyright notice"
    assert (root / "usr/lib/gizlet/gizlet-booter").read_bytes() == booter.read_bytes(), "installed Booter differs from the build"
    for executable in ("usr/lib/gizlet/gizlet-booter", "usr/lib/gizlet/gizlet-launcher"):
        assert "AArch64" in output("readelf", "-h", root / executable), f"/{executable} is AArch64"
    booter_path = "usr/lib/gizlet/gizlet-booter"
    assert "INTERP" not in output("readelf", "-lW", root / booter_path), f"/{booter_path} requires a dynamic loader"
    # The launcher links the base's glibc and libdrm; every library it needs is installed.
    needed = re.findall(r"\(NEEDED\)\s+Shared library: \[([^\]]+)\]", output("readelf", "-dW", root / "usr/lib/gizlet/gizlet-launcher"))
    assert "libdrm.so.2" in needed, needed
    for library in needed:
        assert list(root.glob(f"usr/lib/aarch64-linux-gnu/{library}")), f"the base lacks {library} for the launcher"
    run("sha256sum", image, root / "usr/lib/gizlet/gizlet-booter")
    print("Root and data filesystems match the Gizlet OS layout")


if __name__ == "__main__":
    main()
