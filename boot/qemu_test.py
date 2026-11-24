"""Boot a Gizlet OS card image on QEMU virt and exercise installation and recovery.

Usage: qemu_test.py IMAGE KERNEL_NAME DBCLIENT DROPBEARKEY SCP

KERNEL_NAME is the kernel file on the card's boot partition. The kernel boots
the card's SquashFS partition, which the test finds through the MBR. The
Dropbear tools log in to the guest's sshd over a forwarded virtio-net port.
"""

from contextlib import contextmanager
import hashlib
import os
from pathlib import Path
import selectors
import shutil
import socket
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).parent))
import card  # noqa: E402

PROMPT = b"recovery# "


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
def boot(disk, kernel, root, other=None, ssh_port=None):
    args = ["qemu-system-aarch64", "-machine", "virt", "-cpu", "cortex-a35", "-m", "512",
            "-display", "none", "-monitor", "none", "-serial", "stdio", "-no-reboot",
            "-kernel", str(kernel), "-append", f"console=ttyAMA0 root={root} rootfstype=squashfs rootwait ro"]
    if ssh_port is None:
        args += ["-nic", "none"]
    else:
        args += ["-netdev", f"user,id=n0,hostfwd=tcp:127.0.0.1:{ssh_port}-:22", "-device", "virtio-net-device,netdev=n0"]
    for index, drive in enumerate([disk] if other is None else [disk, other]):
        args += ["-drive", f"if=none,id=d{index},format=raw,file={str(drive).replace(',', ',,')}",
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


def free_port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def check_ssh(private, boot_offset, kernel, root, tools, work):
    """Logs in with a key from authorized_keys on the boot partition, runs ls and copies a file both ways."""
    dbclient, dropbearkey, scp = tools
    key = work / "client_key"
    subprocess.run([dropbearkey, "-t", "ed25519", "-f", key], check=True, capture_output=True)
    public = subprocess.run([dropbearkey, "-y", "-f", key], check=True, capture_output=True, text=True).stdout
    keys = work / "authorized_keys"
    keys.write_text("".join(line + "\n" for line in public.splitlines() if line.startswith("ssh-ed25519 ")))
    subprocess.run(["mcopy", "-i", f"{private}@@{boot_offset}", keys, "::authorized_keys"], check=True)
    # Quotes inside the values must survive into wpa_supplicant's quoted strings.
    wifi = work / "wifi.txt"
    wifi.write_bytes(b'ssid=Test "Net"\r\npassword=!pass"word1\r\n')
    subprocess.run(["mcopy", "-o", "-i", f"{private}@@{boot_offset}", wifi, "::wifi.txt"], check=True)
    port = free_port()
    client = work / "ssh"
    client.write_text(f'#!/bin/sh\nexec "{dbclient}" -y -y -i "{key}" -p {port} "$@"\n')
    client.chmod(0o755)
    with boot(private, kernel, root, ssh_port=port) as console:
        console.expect(b"dropbear listening on port 22")
        console.expect(PROMPT)
        console.command("ip addr add 10.0.2.15/24 dev eth0 && ip link set eth0 up")
        deadline = time.monotonic() + 30
        while True:
            login = subprocess.run([client, "root@127.0.0.1", "ls /; echo ssh-ok"],
                                   stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=30)
            if "ssh-ok" in login.stdout or time.monotonic() > deadline:
                break
            time.sleep(1)
        assert "ssh-ok" in login.stdout and "home" in login.stdout.split(), f"ssh login failed: {login.stderr}"
        # QEMU has no WiFi adapter; gizlet-net still parses wifi.txt and reports.
        wifi_state = subprocess.run([client, "root@127.0.0.1",
                                     "while [ ! -f /run/gizlet-net.log ]; do sleep 1; done; cat /run/gizlet-net/wpa_supplicant.conf; "
                                     "wpa_supplicant -v | head -n 1; grep gizlet-net: /run/gizlet-boot.log"],
                                    stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=90).stdout
        for marker in ('ssid="Test "Net""', 'psk="!pass"word1"', "key_mgmt=WPA-PSK WPA-PSK-SHA256 SAE",
                       "wpa_supplicant v2.12", "no WiFi adapter (wlan0) after 30 s"):
            assert marker in wifi_state, f"WiFi setup lacks {marker!r}: {wifi_state}"
        payload = work / "payload"
        payload.write_bytes(os.urandom(65536))
        subprocess.run([scp, "-S", client, payload, "root@127.0.0.1:/home/payload"],
                       stdin=subprocess.DEVNULL, check=True, timeout=30)
        back = work / "back"
        subprocess.run([scp, "-S", client, "root@127.0.0.1:/home/payload", back],
                       stdin=subprocess.DEVNULL, check=True, timeout=30)
        assert back.read_bytes() == payload.read_bytes(), "scp round trip changed the file"
        console.command("rm /home/payload")
        console.stop()
    net = subprocess.run(["mtype", "-i", f"{private}@@{boot_offset}", "::gizlet-net.log"],
                         check=True, capture_output=True, text=True).stdout
    assert "== Gizlet OS network" in net and "!pass" not in net, "gizlet-net.log is missing or shows the password"
    print("ssh login and scp in both directions work; wifi.txt is parsed and gizlet-net.log saved")


def scenarios(image, kernel_name, work, tools):
    parts, signature = card.partitions(image)
    root = f"PARTUUID={signature:08x}-{parts['root'][0]:02x}"
    kernel = work / "kernel"
    subprocess.run(["mcopy", "-i", f"{image}@@{parts['boot'][1]}", "::" + kernel_name, kernel], check=True)
    private = work / "private.img"
    shutil.copyfile(image, private)
    with boot(private, kernel, root) as console:
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
        console.command("while [ ! -e /run/gizlet-late.done ]; do sleep 1; done")
        console.stop()
    log = subprocess.run(["mtype", "-i", f"{private}@@{parts['boot'][1]}", "::gizlet.log"],
                         check=True, capture_output=True, text=True).stdout
    for marker in ("== Gizlet OS boot log", "== rcS", "data mounts ready", "rcS exit 0", "Linux version"):
        assert marker in log, f"gizlet.log lacks {marker!r}"
    late = subprocess.run(["mtype", "-i", f"{private}@@{parts['boot'][1]}", "::gizlet-late.log"],
                          check=True, capture_output=True, text=True).stdout
    for marker in ("15 s after boot", "== usb devices", "== regulators", "== dmesg"):
        assert marker in late, f"gizlet-late.log lacks {marker!r}"
    print("gizlet.log and gizlet-late.log saved on the boot partition")
    with boot(private, kernel, root) as console:
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
    check_ssh(private, parts["boot"][1], kernel, root, tools, work)
    # A card missing its data partition must not pick a valid data volume on another disk.
    # The second disk is the real, populated data partition as a standalone virtual disk.
    index, offset, size = parts["data"]
    decoy = work / "other.img"
    with private.open("rb") as source, decoy.open("wb") as output:
        source.seek(offset)
        output.write(source.read(size))
    before = hashlib.sha256(decoy.read_bytes()).digest()
    with private.open("r+b") as data:
        data.seek(446 + (index - 1) * 16)
        data.write(bytes(16))
    with boot(private, kernel, root, decoy) as console:
        console.expect(b"data partition missing")
        console.expect(PROMPT)
        console.command("echo local-console-ok")
        console.stop()
    assert hashlib.sha256(decoy.read_bytes()).digest() == before, "other disk was changed"


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(dir=os.environ.get("TEST_TMPDIR")) as temp:
        tools = [Path(tool).resolve() for tool in sys.argv[3:6]]
        scenarios(Path(sys.argv[1]).resolve(), sys.argv[2], Path(temp), tools)
