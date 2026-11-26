"""Check the Gizlet OS root and data filesystems of a card image.

Usage: rootfs_test.py IMAGE BOOTER CONFIG_JSON FLASH_SH FLASH_CARD_SH

CONFIG_JSON names the data filesystem UUID and the license notices required
under /usr/share/licenses. FLASH_SH, the card writer, grows the data
filesystem to fill cards of several sizes, and e2fsck checks each; its update
keeps a card's data partition.
"""

import hashlib
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


def check_flash_growth(flash, flash_card, image, work):
    """Writes cards as flash.sh does and checks the grown data filesystem of each."""
    card_file = work / "card.img"
    data = work / "grown.ext4"
    # Two groups, the last one 1 MiB; a 32 GB card; the MBR's 2 TiB limit.
    for card_bytes in (465 << 20, 31_300_000_256, 2 << 40):
        written = int(output("bash", flash_card, flash, image, card_file, str(card_bytes)))
        _, offset, size = card.partitions(card_file)[0]["data"]
        # The partition alone, sparse as the card.
        with open(card_file, "rb") as source, open(data, "wb") as target:
            target.truncate(size)
            position = offset
            while position < offset + size:
                try:
                    begin = os.lseek(source.fileno(), position, os.SEEK_DATA)
                except OSError:
                    break
                end = min(os.lseek(source.fileno(), begin, os.SEEK_HOLE), offset + size)
                source.seek(begin)
                target.seek(begin - offset)
                while begin < end:
                    chunk = source.read(min(1 << 22, end - begin))
                    target.write(chunk)
                    begin += len(chunk)
                position = end
        card_file.unlink()
        check = subprocess.run(["e2fsck", "-fn", data], capture_output=True, text=True)
        print(check.stdout, check.stderr, flush=True)
        assert check.returncode == 0 and "?" not in check.stdout, f"e2fsck on a {card_bytes}-byte card"
        blocks = int(re.search(r"^Block count:\s+(\d+)", output("dumpe2fs", "-h", data), re.M).group(1))
        # Every whole 4 KiB block: at 2 TiB the MBR's sector count is not a multiple of 8.
        assert blocks == size // 4096, f"{blocks} blocks on a {size}-byte partition"
        print(f"a {card_bytes}-byte card: {blocks} blocks, {written} written to grow them", flush=True)
        data.unlink()


def digest(path, offset, size):
    """SHA-256 of SIZE bytes at OFFSET of a sparse file, reading only its data."""
    sha = hashlib.sha256()
    with open(path, "rb") as source:
        position = offset
        while position < offset + size:
            try:
                begin = min(os.lseek(source.fileno(), position, os.SEEK_DATA), offset + size)
            except OSError:
                begin = offset + size
            sha.update(b"hole %d %d" % (position, begin))
            if begin >= offset + size:
                break
            end = min(os.lseek(source.fileno(), begin, os.SEEK_HOLE), offset + size)
            source.seek(begin)
            sha.update(source.read(end - begin))
            position = end
    return sha.hexdigest()


def check_flash_update(flash, flash_card, image, work):
    """flash.sh's update rewrites the system and keeps the data partition; it refuses another card."""
    card_file = work / "update.img"
    output("bash", flash_card, flash, image, card_file, str(1 << 30))
    _, offset, size = card.partitions(card_file)[0]["data"]
    data = digest(card_file, offset, size)
    run("bash", flash_card, "--update", flash, image, card_file)
    assert digest(card_file, offset, size) == data, "the update changed the data partition"
    # Everything before the data partition but the partition table is the image's.
    with open(image, "rb") as source, open(card_file, "rb") as target:
        assert source.read(offset)[512:] == target.read(offset)[512:], "the update did not write the image's system"
    assert card.partitions(card_file)[0]["data"][2] == size, "the update changed the data partition's size"
    # Another data filesystem (here its UUID changed) is refused, and nothing is written.
    with open(card_file, "r+b") as target:
        target.seek(offset + 1024 + 0x68)
        target.write(b"\0")
    before = digest(card_file, 0, offset + size)
    refused = subprocess.run(["bash", flash_card, "--update", flash, image, card_file], capture_output=True, text=True)
    assert refused.returncode != 0 and "cannot be updated" in refused.stderr, refused.stderr
    assert digest(card_file, 0, offset + size) == before, "a refused update changed the card"
    card_file.unlink()
    print("flash.sh's update keeps the data partition and refuses another card", flush=True)


def main():
    image, booter, config = sys.argv[1], Path(sys.argv[2]), json.loads(sys.argv[3])
    parts, _ = card.partitions(image)
    work = Path(tempfile.mkdtemp(prefix="rootfs-test-"))
    check_flash_growth(sys.argv[4], sys.argv[5], image, work)
    check_flash_update(sys.argv[4], sys.argv[5], image, work)

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
                 "lib/libraylib.so.600", "lib/dri/rockchip_dri.so",
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
    # bluetoothd starts when a USB controller appears (udev pulls in bluetooth.target).
    assert (systemd / "bluetooth.target.wants/bluetooth.service").is_symlink(), "bluetooth.service is enabled"
    for tool in ("usr/libexec/bluetooth/bluetoothd", "usr/bin/bluetoothctl", "usr/bin/btmgmt"):
        assert (root / tool).exists(), f"/{tool}"
    for unit in ("getty@.service", "serial-getty@.service", "systemd-firstboot.service",
                 "e2scrub_all.timer", "e2scrub_reap.service",
                 "systemd-journal-catalog-update.service", "systemd-update-done.service"):
        assert os.readlink(systemd / unit) == "/dev/null", f"{unit} is masked"
    # systemd-resolved starts with iwd, not beside the data mount at boot.
    for link in ("sysinit.target.wants/systemd-resolved.service", "dbus-org.freedesktop.resolve1.service"):
        assert not os.path.lexists(systemd / link), f"systemd-resolved is enabled ({link})"
    assert "Wants=systemd-resolved.service" in (root / "usr/lib/systemd/system/iwd.service.d/gizlet.conf").read_text(), \
        "iwd.service starts systemd-resolved"
    # It answers for the hostname by mDNS, which gizlet-hostname.service sets at
    # boot and when udev adds a network adapter.
    assert "MulticastDNS=yes" in (root / "usr/lib/systemd/resolved.conf.d/gizlet.conf").read_text(), "mDNS is on"
    assert (systemd / "multi-user.target.wants/gizlet-hostname.service").is_symlink(), "gizlet-hostname.service is enabled"
    assert "gizlet-hostname.service" in (root / "usr/lib/udev/rules.d/90-gizlet-hostname.rules").read_text(), \
        "udev sets the hostname for a new network adapter"
    # Network time sets the clock; the time zone is linked under /run at boot (gizlet-switch).
    assert (systemd / "sysinit.target.wants/systemd-timesyncd.service").is_symlink(), "systemd-timesyncd is enabled"
    assert "NTP=time.apple.com " in (root / "usr/lib/systemd/timesyncd.conf.d/gizlet.conf").read_text(), \
        "systemd-timesyncd asks Apple's time servers"
    assert os.readlink(root / "etc/localtime") == "/run/gizlet-localtime", "/etc/localtime points into /run"
    for zone in ("Etc/UTC", "Asia/Shanghai", "America/New_York"):
        assert (root / "usr/share/zoneinfo" / zone).read_bytes()[:4] == b"TZif", f"zone {zone}"
    # Each card creates its own identity at boot.
    assert not list((root / "etc/ssh").glob("ssh_host_*")), "the image carries ssh host keys"
    assert (root / "etc/machine-id").read_text() == "", "the image carries a machine ID"
    for script in ("boot-session", "gizlet-boot-device", "gizlet-mount-data", "gizlet-ssh", "gizlet-setup", "gizlet-switch",
                   "gizlet-var", "save-boot-log", "gizlet-booter", "gizlet-launcher", "gizlet-launcher-start"):
        assert (root / "usr/lib/gizlet" / script).stat().st_mode & 0o111, f"/usr/lib/gizlet/{script} is executable"
    # First boot grows the data partition and its filesystem.
    for tool in ("usr/sbin/sfdisk", "usr/bin/partx", "usr/sbin/resize2fs"):
        assert (root / tool).exists(), f"/{tool}"
    for directory in ("opt/gizlet", "home"):
        assert (root / directory).is_dir(), f"/{directory}"
    for notice in config["licenses"]:
        assert (root / "usr/share/licenses" / notice).stat().st_size > 0, f"license notice {notice}"
    for package in ("libc6", "systemd", "openssh-server", "iwd", "bluez"):
        assert (root / "usr/share/doc" / package / "copyright").stat().st_size > 0, f"{package} copyright notice"
    assert (root / "usr/lib/gizlet/gizlet-booter").read_bytes() == booter.read_bytes(), "installed Booter differs from the build"
    for executable in ("usr/lib/gizlet/gizlet-booter", "usr/lib/gizlet/gizlet-launcher"):
        assert "AArch64" in output("readelf", "-h", root / executable), f"/{executable} is AArch64"
    booter_path = "usr/lib/gizlet/gizlet-booter"
    assert "INTERP" not in output("readelf", "-lW", root / booter_path), f"/{booter_path} requires a dynamic loader"
    # The launcher runs against runtime 1, which has every library it needs, raylib among them.
    needed = re.findall(r"\(NEEDED\)\s+Shared library: \[([^\]]+)\]", output("readelf", "-dW", root / "usr/lib/gizlet/gizlet-launcher"))
    assert "libraylib.so.600" in needed, needed
    for library in needed:
        assert f"squashfs-root/lib/{library}\n" in listing, f"runtime 1 lacks {library} for the launcher"
    run("sha256sum", image, root / "usr/lib/gizlet/gizlet-booter")
    print("Root and data filesystems match the Gizlet OS layout")


if __name__ == "__main__":
    main()
