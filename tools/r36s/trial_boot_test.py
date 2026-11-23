"""Exercise the delivered RAM filesystem on QEMU virt with a PL011-capable Image."""

import gzip
import os
from pathlib import Path
import select
import subprocess
import sys
import tempfile
import time
import zlib


def scenario(kernel, archive, bad_entry, display):
    command = ["qemu-system-aarch64", "-M", "virt", "-cpu", "cortex-a53", "-m", "384",
               "-display", "none", "-nic", "none", "-monitor", "none", "-serial", "stdio", "-no-reboot",
               "-kernel", str(kernel), "-initrd", str(archive), "-append",
               "rdinit=/init ro console=tty0 console=ttyAMA0 loglevel=5 panic=-1"]
    if display:
        command.extend(["-global", "virtio-mmio.force-legacy=false",
                        "-device", "virtio-gpu-device,xres=640,yres=480"])
    proc = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    output = bytearray()
    cursor = 0

    def expect(marker):
        nonlocal cursor
        end = time.monotonic() + 60
        while marker not in output[cursor:]:
            remaining = end - time.monotonic()
            if remaining <= 0 or not select.select([proc.stdout], [], [], remaining)[0]:
                raise RuntimeError(f"QEMU timed out waiting for {marker!r}")
            data = os.read(proc.stdout.fileno(), 8192)
            if not data:
                raise RuntimeError(f"QEMU stopped before {marker!r}")
            sys.stdout.buffer.write(data)
            sys.stdout.buffer.flush()
            output.extend(data)
        cursor = output.index(marker, cursor) + len(marker)

    try:
        expect(b"1. RAM /init reached")
        expect(b"3. Starting Gizlet Booter")
        if bad_entry:
            expect(b"cannot open configured entry")
            expect(b"ERROR: Booter exited 1")
        else:
            expect(b"RAM entry running; Linux")
            expect(b"GIZLET PANEL TEST READY")
        expect(b"gizlet-trial#")
        proc.stdin.write(b"echo PROBE_$(uname -m); cat /proc/mounts\n")
        proc.stdin.flush()
        expect(b"PROBE_aarch64")
        expect(b"sysfs /sys sysfs")
        # Prove a real recovery command runs and the trial never mounted a block device.
        proc.stdin.write(b"while read src rest; do case $src in /dev/*) echo BLOCK_MOUNT=$src;; esac; done </proc/mounts; echo END_$(uname -m)\n")
        proc.stdin.flush()
        expect(b"END_aarch64")
        if b"\nBLOCK_MOUNT=/dev/" in output:
            raise RuntimeError("trial mounted a block device")
        if display:
            proc.stdin.write(
                b"exec 8<>/etc/display.tick; for try in 1 2 3 4 5 6 7 8 9 10; do "
                b"[ -r /sys/class/graphics/fb0/name ] && break; read -r -t 1 wait <&8; done; "
                b"echo DRIVER_$(cat /sys/class/graphics/fb0/name); "
                b"echo SIZE_$(cat /sys/class/graphics/fb0/virtual_size); "
                b"cat /sys/class/vtconsole/vtcon1/name\n"
            )
            proc.stdin.flush()
            expect(b"DRIVER_virtio_gpudrmfb")
            expect(b"SIZE_640,480")
            expect(b"frame buffer device")
            proc.stdin.write(b"echo SCREEN_$(uname -m); cat /dev/vcs1; echo\n")
            proc.stdin.flush()
            expect(b"SCREEN_aarch64")
            expect(b"ERROR: Booter exited 1" if bad_entry else b"GIZLET PANEL TEST READY")
        proc.stdin.write(b"/sbin/reboot -f\n")
        proc.stdin.flush()
        if proc.wait(timeout=20) != 0:
            raise RuntimeError("QEMU failed to reboot")
    finally:
        proc.kill()
        proc.wait()
        proc.stdin.close()
        proc.stdout.close()


def main(kernel, ramdisk, display):
    image = ramdisk.read_bytes()
    if len(image) < 64 or image[:4].hex() != "27051956":
        raise ValueError("expected a legacy U-Boot ramdisk")
    if int.from_bytes(image[4:8], "big") != zlib.crc32(image[:4] + bytes(4) + image[8:64]):
        raise ValueError("invalid ramdisk header CRC")
    payload = image[64:]
    if int.from_bytes(image[12:16], "big") != len(payload) or int.from_bytes(image[24:28], "big") != zlib.crc32(payload):
        raise ValueError("invalid ramdisk payload")
    raw = gzip.decompress(payload)
    good = b"[boot]\nentry=bin/display-check\n"
    if raw.count(good) != 1:
        raise ValueError("expected the trial Booter configuration in the archive")
    with tempfile.TemporaryDirectory() as work:
        archive = Path(work) / "ram.cpio.gz"
        archive.write_bytes(payload)
        scenario(kernel, archive, False, display)
        archive.write_bytes(gzip.compress(raw.replace(good, good.replace(b"bin/", b"bad/")), mtime=0))
        scenario(kernel, archive, True, display)
    print("\nPASS: RAM trial launch, failed entry, interactive recovery and RAM-only mounts")
    if display:
        print("PASS: tty1 framebuffer console text on both boots")


if __name__ == "__main__":
    main(Path(sys.argv[1]), Path(sys.argv[2]), "--serial-only" not in sys.argv[3:])
