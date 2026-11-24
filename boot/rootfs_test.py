"""Check the Gizlet OS root and data filesystems of a card image.

Usage: rootfs_test.py IMAGE BOOTER CONFIG_JSON

CONFIG_JSON names the data filesystem UUID and the license notices required
under /usr/share/licenses.
"""

import json
from pathlib import Path
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
    for directory in ("/gizlet", "/home"):
        assert "Type: directory" in output("debugfs", "-R", f"stat {directory}", data), directory
        names = [line.split("/")[5] for line in output("debugfs", "-R", f"ls -p {directory}", data).splitlines() if line.count("/") >= 5]
        assert set(names) <= {".", ".."}, f"{directory} is not empty: {names}"

    squashfs = work / "root.sqfs"
    extract(image, *parts["root"][1:], squashfs)
    root = work / "root"
    run("unsquashfs", "-no-progress", "-d", root, squashfs, stdout=subprocess.DEVNULL)
    busybox = (root / "bin/busybox").resolve()
    for link in ("sbin/init", "bin/sh", "bin/cttyhack"):
        assert (root / link).resolve() == busybox, f"/{link} is BusyBox"
    inittab = (root / "etc/inittab").read_text().splitlines()
    for line in ("::sysinit:/etc/rcS", "::sysinit:/sbin/debug-link", "::sysinit:/sbin/save-boot-log", "ttyGS0::respawn:-/bin/sh", "::respawn:/bin/cttyhack /sbin/boot-session", "tty1::askfirst:-/bin/sh"):
        assert line in inittab, line
    for executable in ("etc/rcS", "sbin/boot-session", "sbin/save-boot-log", "sbin/debug-link", "sbin/usb-watch", "sbin/gizlet-booter", "usr/sbin/dropbear", "usr/bin/scp"):
        assert (root / executable).stat().st_mode & 0o111, f"/{executable} is executable"
    for directory in ("opt/gizlet", "home"):
        assert (root / directory).is_dir(), f"/{directory}"
    for notice in config["licenses"]:
        assert (root / "usr/share/licenses" / notice).stat().st_size > 0, f"license notice {notice}"
    assert (root / "sbin/gizlet-booter").read_bytes() == booter.read_bytes(), "installed Booter differs from the build"
    for executable in ("sbin/gizlet-booter", "bin/busybox", "usr/sbin/dropbear", "usr/bin/dropbearkey", "usr/bin/scp"):
        assert "AArch64" in output("readelf", "-h", root / executable), f"/{executable} is AArch64"
        assert "INTERP" not in output("readelf", "-lW", root / executable), f"/{executable} requires a dynamic loader"
    run("sha256sum", image, root / "bin/busybox", root / "sbin/gizlet-booter")
    print("Root and data filesystems match the Gizlet OS layout")


if __name__ == "__main__":
    main()
