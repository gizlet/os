"""Exercise the generated TF1 card and persistent installation area using QEMU virt."""

from contextlib import contextmanager
import hashlib
import os
from pathlib import Path
import selectors
import shutil
import subprocess
import sys
import tempfile
import time

PROMPT = b"recovery# "
MIB = 1024 * 1024


class Console:
    def __init__(self, guest, serial):
        self.guest = guest
        self.serial = serial
        self.pending = bytearray()

    def expect(self, marker):
        deadline = time.monotonic() + 45
        while marker not in self.pending:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not self.serial.select(remaining):
                raise TimeoutError(f"waiting for {marker!r}")
            chunk = os.read(self.guest.stdout.fileno(), 8192)
            if not chunk:
                raise EOFError(f"guest exited before {marker!r}")
            sys.stdout.buffer.write(chunk)
            sys.stdout.buffer.flush()
            self.pending.extend(chunk)
        del self.pending[:self.pending.index(marker) + len(marker)]

    def enter(self, line):
        self.guest.stdin.write((line + "\n").encode())
        self.guest.stdin.flush()

    def command(self, line):
        self.enter(line)
        self.expect(PROMPT)

    def stop(self):
        self.enter("sync && poweroff -f")
        self.expect(b"Power down")
        if self.guest.wait(timeout=10) != 0:
            raise RuntimeError("QEMU failed at shutdown")


@contextmanager
def boot(card, kernel, other=None):
    args = ["qemu-system-aarch64", "-machine", "virt", "-cpu", "cortex-a35", "-m", "512",
            "-display", "none", "-monitor", "none", "-serial", "stdio", "-nic", "none", "-no-reboot",
            "-kernel", str(kernel), "-append", "console=ttyAMA0 root=PARTUUID=36335030-02 rootfstype=squashfs rootwait ro"]
    for index, disk in enumerate([card] if other is None else [card, other]):
        args += ["-drive", f"if=none,id=d{index},format=raw,file={str(disk).replace(',', ',,')}",
                 "-device", f"virtio-blk-device,drive=d{index}"]
    with subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT) as guest, selectors.DefaultSelector() as serial:
        try:
            serial.register(guest.stdout, selectors.EVENT_READ)
            yield Console(guest, serial)
        finally:
            if guest.poll() is None:
                guest.kill()
                guest.wait()


def scenarios(image, work):
    kernel = work / "Image"
    subprocess.run(["mcopy", "-i", f"{image}@@{16 * MIB}", "::Image", kernel], check=True)
    card = work / "private.img"
    shutil.copyfile(image, card)
    with boot(card, kernel) as console:
        console.expect(b"/opt/gizlet/boot.ini: No such file")
        console.expect(PROMPT)
        console.enter("cat /proc/mounts")
        for mount in (b" / squashfs ro,", b" /opt/gizlet ext4 rw,", b" /home ext4 rw,"):
            console.expect(mount)
        console.expect(PROMPT)
        console.command("mkdir /opt/gizlet/bin")
        console.command("printf '%s\\n' '#!/bin/sh' 'echo hello-from-card' > /opt/gizlet/bin/hello && chmod 755 /opt/gizlet/bin/hello")
        console.command("printf '%s\\n' '[boot]' 'entry=bin/hello' > /opt/gizlet/boot.ini")
        console.command("echo persistent-user-file > /home/check")
        console.stop()
    with boot(card, kernel) as console:
        console.expect(b"starting /opt/gizlet/bin/hello")
        console.expect(b"\r\nhello-from-card\r\n")
        console.expect(b"boot entry finished with status 0")
        console.expect(PROMPT)
        console.enter("cat /home/check")
        console.expect(b"\r\npersistent-user-file\r\n")
        console.expect(PROMPT)
        # Exercise invalid configuration, a symlink and an executable rejected by the kernel.
        console.command("printf '%s\\n' '[boot]' 'entry=bin/hello' 'entry=again' > /opt/gizlet/boot.ini")
        console.enter("exit")
        console.expect(b"invalid boot.ini")
        console.expect(PROMPT)
        console.command("printf '%s\\n' '[boot]' 'entry=../bad' > /opt/gizlet/boot.ini")
        console.enter("exit")
        console.expect(b"cannot open configured entry")
        console.expect(PROMPT)
        console.command("ln -s /bin/busybox /opt/gizlet/bin/link")
        console.command("printf '%s\\n' '[boot]' 'entry=bin/link' > /opt/gizlet/boot.ini")
        console.enter("exit")
        console.expect(b"cannot open configured entry")
        console.expect(PROMPT)
        console.command("echo bad-format > /opt/gizlet/bin/bad && chmod 755 /opt/gizlet/bin/bad")
        console.command("printf '%s\\n' '[boot]' 'entry=bin/bad' > /opt/gizlet/boot.ini")
        console.enter("exit")
        console.expect(b"execute entry: Exec format error")
        console.expect(PROMPT)
        console.enter("cat /home/check")
        console.expect(b"\r\npersistent-user-file\r\n")
        console.expect(PROMPT)
        console.stop()
    # A TF1 card missing p3 must not pick a valid data volume on another disk.
    # The second disk is the real, populated data partition as a standalone virtual disk.
    decoy = work / "other.img"
    with card.open("rb") as source, decoy.open("wb") as output:
        source.seek(96 * MIB)
        shutil.copyfileobj(source, output)
    before = hashlib.sha256(decoy.read_bytes()).digest()
    with card.open("r+b") as data:
        data.seek(446 + 2 * 16)
        data.write(bytes(16))
    with boot(card, kernel, decoy) as console:
        console.expect(b"data partition missing")
        console.expect(PROMPT)
        console.command("echo local-console-ok")
        console.stop()
    assert hashlib.sha256(decoy.read_bytes()).digest() == before, "other disk was changed"


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(dir=os.environ.get("TEST_TMPDIR")) as temp:
        scenarios(Path(sys.argv[1]).resolve(), Path(temp))
