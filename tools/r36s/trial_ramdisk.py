"""Build the V30 RAM trial's deterministic newc archive and legacy U-Boot ramdisk."""

import argparse
import gzip
import io
from pathlib import Path
import stat
import struct
import subprocess
import zlib

EPOCH = 1704067200
RAMDISK_LIMIT = 0x01F00000 - 0x01100000


def align(stream, size):
    stream.write(bytes((-stream.tell()) % size))


def newc(entries):
    stream = io.BytesIO()
    for ino, (path, (mode, data, major, minor)) in enumerate([*entries.items(), ("TRAILER!!!", (0, b"", 0, 0))], 1):
        name = path.encode("ascii") + b"\0"
        fields = (ino, mode, 0, 0, 2 if stat.S_ISDIR(mode) else 1, EPOCH, len(data), 0, 0,
                  major, minor, len(name), 0)
        stream.write(b"070701" + b"".join(f"{field:08x}".encode() for field in fields) + name)
        align(stream, 4)
        stream.write(data)
        align(stream, 4)
    align(stream, 512)
    return stream.getvalue()


def static_arm64(data):
    if data[:6] != b"\x7fELF\x02\x01" or struct.unpack_from("<H", data, 18)[0] != 183:
        raise ValueError("expected a little-endian AArch64 ELF")
    offset, = struct.unpack_from("<Q", data, 32)
    step, count = struct.unpack_from("<HH", data, 54)
    if step != 56 or not count or offset + step * count > len(data):
        raise ValueError("invalid ELF program headers")
    if any(struct.unpack_from("<I", data, offset + i * step)[0] == 3 for i in range(count)):
        raise ValueError("the RAM trial requires a static executable")
    return data


def build(read_root, board):
    entries = {}
    for path in ("bin", "dev", "etc", "opt", "opt/gizlet", "opt/gizlet/bin", "proc", "run", "sbin",
                 "sys", "tmp", "usr", "usr/share", "usr/share/licenses"):
        entries[path] = (stat.S_IFDIR | (0o1777 if path == "tmp" else 0o755), b"", 0, 0)
    for path, source in (("bin/busybox", "bin/busybox"), ("sbin/gizlet-booter", "sbin/gizlet-booter")):
        entries[path] = (0o100755, static_arm64(read_root(source)), 0, 0)
    for name in ("BusyBox", "GizOS", "glibc-LGPL-2.1", "musl-COPYRIGHT", "Zig-MIT"):
        path = "usr/share/licenses/" + name
        entries[path] = (0o100644, read_root(path), 0, 0)
    for path, name, mode in (("init", "init", 0o755), ("etc/inittab", "inittab", 0o644),
                             ("sbin/recovery", "recovery", 0o755), ("opt/gizlet/boot.ini", "trial-entry.ini", 0o644),
                             ("opt/gizlet/bin/display-check", "display-check", 0o755)):
        entries[path] = (stat.S_IFREG | mode, (board / name).read_bytes(), 0, 0)
    for applet in ("ash", "sh", "cat", "cttyhack", "dmesg", "ls", "mount", "ps", "readlink", "stty", "uname"):
        entries["bin/" + applet] = (0o120777, b"busybox", 0, 0)
    for applet in ("init", "poweroff", "reboot"):
        entries["sbin/" + applet] = (0o120777, b"../bin/busybox", 0, 0)
    for name, major, minor in (("console", 5, 1), ("null", 1, 3), ("tty1", 4, 1)):
        entries["dev/" + name] = (0o020600, b"", major, minor)
    entries["etc/display.tick"] = (0o010600, b"", 0, 0)
    compressed = io.BytesIO()
    with gzip.GzipFile(fileobj=compressed, mode="wb", filename="", mtime=0) as stream:
        stream.write(newc(entries))
    payload = compressed.getvalue()
    # IH_OS_LINUX / IH_ARCH_ARM / IH_TYPE_RAMDISK / IH_COMP_GZIP, as accepted by Clone U-Boot.
    header = bytearray(struct.pack(">7I4B32s", 0x27051956, 0, EPOCH, len(payload), 0, 0,
                                   zlib.crc32(payload), 5, 2, 3, 1, b"Gizlet V30 Panel 4 RAM trial"))
    struct.pack_into(">I", header, 4, zlib.crc32(header))
    if len(header) + len(payload) >= RAMDISK_LIMIT:
        raise ValueError("ramdisk exceeds the gap before the board DTB")
    return header + payload


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("rootfs", type=Path, help="Gizlet OS ARM64 SquashFS")
    parser.add_argument("board", type=Path)
    parser.add_argument("out", type=Path)
    args = parser.parse_args()
    def read_root(name):
        return subprocess.check_output(["unsquashfs", "-cat", str(args.rootfs), name])
    args.out.write_bytes(build(read_root, args.board))


if __name__ == "__main__":
    main()
