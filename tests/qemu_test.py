"""Boot a Gizlet OS card image on QEMU virt and exercise installation and recovery.

Usage: qemu_test.py IMAGE KERNEL_NAME

KERNEL_NAME is the kernel file on the card's boot partition. The kernel boots
the card's SquashFS partition, which the test finds through the MBR, with
systemd. The build VM's OpenSSH client logs in to the guest's sshd over a
forwarded virtio-net port.
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
        """Waits for MARKER and returns what came before it."""
        deadline = time.monotonic() + 120
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
        at = self.pending.index(marker)
        before = bytes(self.pending[:at])
        del self.pending[:at + len(marker)]
        return before

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


def set_fat_dirty(disk, offset, dirty):
    """Sets or clears the dirty flag of the FAT file system at byte OFFSET of DISK."""
    with open(disk, "r+b") as f:
        f.seek(offset)
        sector = f.read(90)
        # The state byte Linux uses: after the FAT32 extended fields, or the FAT12/16 ones.
        at = 0x41 if sector[82:90] == b"FAT32   " else 0x25
        state = sector[at] | 1 if dirty else sector[at] & ~1
        f.seek(offset + at)
        f.write(bytes([state]))


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


def check_ssh(private, boot_offset, kernel, root, work):
    """Starts sshd as the launcher will, logs in with a key from authorized_keys on the boot partition and copies a file both ways."""
    key = work / "client_key"
    subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", key], check=True)
    subprocess.run(["mcopy", "-i", f"{private}@@{boot_offset}", key.with_suffix(".pub"), "::authorized_keys"], check=True)
    # setup.ini saves a Wi-Fi network (an SSID iwd stores as =hex), sets the ssh
    # password and time zone and turns ssh on, once, as the launcher would.
    setup = work / "setup.ini"
    setup.write_bytes(b"wifi_ssid=Test Net!\r\nwifi_passphrase=passphrase-123\r\nssh=on\r\nssh_password=gizlet test\r\n"
                      b"timezone=Asia/Shanghai\r\n")
    subprocess.run(["mcopy", "-i", f"{private}@@{boot_offset}", setup, "::setup.ini"], check=True)
    port = free_port()
    client = work / "ssh"
    client.write_text(f'#!/bin/sh\nexec ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -i "{key}" -p {port} "$@"\n')
    client.chmod(0o755)
    with boot(private, kernel, root, ssh_port=port) as console:
        console.expect(PROMPT)
        console.command("ip addr add 10.0.2.15/24 dev eth0 && ip link set eth0 up")
        # gizlet-switches.service imports setup.ini while the console is already up.
        console.command("for i in $(seq 60); do [ \"$(systemctl show -P ActiveState gizlet-switches)\" = activating ] || break; sleep 1; done")
        console.enter("cat '/var/lib/iwd/=54657374204e657421.psk'; /usr/lib/gizlet/gizlet-switch wifi; /usr/lib/gizlet/gizlet-switch ssh; echo \"[$(/usr/lib/gizlet/gizlet-switch ssh-password)]\"")
        # Line by line: services such as gizlet-ssh may print to the console in between.
        for line in (b"[Security]\r\nPassphrase=passphrase-123", b"\r\n[Settings]\r\nAutoConnect=true", b"\r\non",
                     b"\r\non", b"\r\n[gizlet test]\r\n"):
            console.expect(line)
        console.expect(PROMPT)
        # setup.ini is imported once: renamed, without the passphrase and password.
        console.enter("mount -o ro /dev/vda1 /mnt && cat /mnt/setup.ini.done; ls /mnt/setup.ini; umount /mnt")
        console.expect(b"wifi_ssid=Test Net!\r\nssh=on\r\ntimezone=Asia/Shanghai\r\n")
        console.expect(b"No such file")
        console.expect(PROMPT)
        # The time zone applies at once, to programs already running too.
        console.enter("date +%z; /usr/lib/gizlet/gizlet-switch timezone")
        console.expect(b"\r\n+0800\r\nAsia/Shanghai\r\n")
        console.expect(PROMPT)
        # Only compiled zones under /usr/share/zoneinfo are accepted.
        console.enter("for z in ../../../etc/passwd /etc/passwd zone1970.tab Nowhere/City; do "
                      "/usr/lib/gizlet/gizlet-switch timezone $z 2>/dev/null || echo refused; done")
        console.expect(b"\r\nrefused\r\nrefused\r\nrefused\r\nrefused\r\n")
        console.expect(PROMPT)
        deadline = time.monotonic() + 60
        while True:
            login = subprocess.run([client, "root@127.0.0.1", "ls /; echo ssh-ok"],
                                   stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=30)
            if "ssh-ok" in login.stdout or time.monotonic() > deadline:
                break
            time.sleep(1)
        assert "ssh-ok" in login.stdout and "home" in login.stdout.split(), f"ssh login failed: {login.stderr}"
        # An interactive login asks for a pseudo-terminal first. The client
        # gets a terminal of its own, as when a person runs ssh.
        local, remote = os.openpty()
        try:
            pty = subprocess.run([client, "-t", "root@127.0.0.1", "readlink /proc/self/fd/0"],
                                 stdin=remote, capture_output=True, text=True, timeout=30)
        finally:
            os.close(local)
            os.close(remote)
        # The password logs in too, as typed in the launcher.
        askpass = work / "askpass"
        askpass.write_text("#!/bin/sh\necho 'gizlet test'\n")
        askpass.chmod(0o755)
        by_password = subprocess.run(
            ["setsid", "ssh", "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null", "-o", "LogLevel=ERROR",
             "-o", "PubkeyAuthentication=no", "-o", "PreferredAuthentications=password", "-p", str(port),
             "root@127.0.0.1", "echo password-ok"],
            stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=30,
            env=os.environ | {"SSH_ASKPASS": str(askpass), "SSH_ASKPASS_REQUIRE": "force", "DISPLAY": ":0"})
        assert "password-ok" in by_password.stdout, f"password login failed: {by_password.stderr}"
        assert "/dev/pts/" in pty.stdout, f"ssh got no pseudo-terminal: {pty.stdout!r} {pty.stderr!r}"
        # iwd is off at boot; the launcher turns it on. It must stay up without a WiFi adapter.
        iwd = subprocess.run([client, "root@127.0.0.1", "/usr/lib/gizlet/gizlet-switch wifi on; sleep 3; systemctl is-active iwd; "
                              "journalctl -b --no-pager -u iwd | tail -n 20"],
                             stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=60).stdout
        assert iwd.startswith("active\n"), f"iwd did not stay up: {iwd}"
        payload = work / "payload"
        payload.write_bytes(os.urandom(65536))
        subprocess.run(["scp", "-O", "-S", client, payload, "root@127.0.0.1:/home/payload"],
                       stdin=subprocess.DEVNULL, check=True, timeout=30)
        back = work / "back"
        subprocess.run(["scp", "-O", "-S", client, "root@127.0.0.1:/home/payload", back],
                       stdin=subprocess.DEVNULL, check=True, timeout=30)
        assert back.read_bytes() == payload.read_bytes(), "scp round trip changed the file"
        console.command("rm /home/payload")
        console.enter("/opt/gizlet/launcher/gizlet-launcher --probe")
        console.expect(b"wifi: Ok(WifiState { on: true")
        console.expect(b'ssh: Ok(SshState { on: true, running: true, keys: Some(1), password: Some("gizlet test") })')
        console.expect(b'clock: Ok(ClockState { zone: "Asia/Shanghai", synced: ')
        console.expect(PROMPT)
        console.stop()
    # Both switches survive a reboot, and turning them off stops the services.
    with boot(private, kernel, root) as console:
        console.expect(PROMPT)
        # gizlet-switches.service starts them without holding up boot; wait for both.
        console.enter("for i in $(seq 60); do systemctl -q is-active ssh && systemctl -q is-active iwd && break; sleep 1; done; systemctl is-active ssh iwd")
        console.expect(b"\r\nactive\r\nactive\r\n")
        console.expect(PROMPT)
        # Once imported, the switches stay as the launcher leaves them.
        console.command("/usr/lib/gizlet/gizlet-switch ssh off && /usr/lib/gizlet/gizlet-switch wifi off")
        console.enter("systemctl is-active ssh iwd; /usr/lib/gizlet/gizlet-switch ssh")
        console.expect(b"\r\ninactive\r\ninactive\r\noff\r\n")
        console.expect(PROMPT)
        # The time zone holds after a reboot and changes back as the launcher changes it.
        console.enter("date +%z; /usr/lib/gizlet/gizlet-switch timezone America/New_York && date +%Z | grep -qx -e EST -e EDT && echo new-york")
        console.expect(b"\r\n+0800\r\nnew-york\r\n")
        console.expect(PROMPT)
        console.stop()
    with boot(private, kernel, root) as console:
        console.expect(PROMPT)
        console.enter("sleep 5; systemctl is-active ssh iwd; /usr/lib/gizlet/gizlet-switch timezone")
        console.expect(b"\r\ninactive\r\ninactive\r\nAmerica/New_York\r\n")
        console.expect(PROMPT)
        console.stop()
    print("setup.ini (imported once) and the launcher's switches start sshd and iwd and set the time zone, "
          "also after a reboot; key and password logins and scp work")


def scenarios(image, kernel_name, work):
    parts, signature = card.partitions(image)
    root = f"PARTUUID={signature:08x}-{parts['root'][0]:02x}"
    kernel = work / "kernel"
    subprocess.run(["mcopy", "-i", f"{image}@@{parts['boot'][1]}", "::" + kernel_name, kernel], check=True)
    private = work / "private.img"
    shutil.copyfile(image, private)
    # A larger card than the image, written as flash.sh and flash.ps1 write it:
    # partition 3 extended to the card's end. First boot grows the filesystem.
    card_bytes = 16 << 30
    os.truncate(private, card_bytes)
    with open(private, "r+b") as f:
        f.seek(478 + 8)
        start = int.from_bytes(f.read(4), "little")
        f.write((card_bytes // 512 // 2048 * 2048 - start).to_bytes(4, "little"))
    with boot(private, kernel, root) as console:
        # A fresh card starts the factory launcher. QEMU's machine has no
        # display, so it says so and the recovery shell opens.
        console.expect(b"installed the factory launcher as the boot entry")
        console.expect(b"starting /opt/gizlet/launcher/gizlet-launcher")
        console.expect(b"Gizlet launcher 0.")
        console.expect(b"Gizlet launcher: no display")
        console.expect(b"boot entry finished with status 1")
        console.expect(PROMPT)
        # Its device probe runs without a display: no battery, backlight, sound or WiFi adapter here.
        # No Sleep either: gizlet-power, which suspends, needs a backlight.
        console.enter("/opt/gizlet/launcher/gizlet-launcher --probe")
        for line in (b"battery: Err(NoHardware)", b"brightness: Err(NoHardware)", b"volume: Err(NoHardware)",
                     b"wifi: Ok(WifiState { on: false, adapter: false", b"ssh: Ok(SshState { on: false, running: false",
                     b"power: [Reboot, PowerOff]"):
            console.expect(line)
        console.expect(PROMPT)
        # A fresh card keeps UTC until a time zone is chosen; network time runs without a network.
        console.enter("date +%Z; /usr/lib/gizlet/gizlet-switch timezone; systemctl is-active systemd-timesyncd")
        console.expect(b"\r\nUTC\r\nEtc/UTC\r\nactive\r\n")
        console.expect(PROMPT)
        # The Bluetooth core is built in; with no controller plugged in, bluetoothd
        # has not started. Started by hand it runs on the read-only base, keeps its
        # state on the data partition and answers bluetoothctl.
        console.enter("systemctl is-active bluetooth; systemctl start bluetooth; systemctl is-active bluetooth; "
                      "findmnt -n -o SOURCE /var/lib/bluetooth; timeout 10 bluetoothctl show")
        console.expect(b"\r\ninactive\r\nactive\r\n")
        console.expect(b"[/system/bluetooth]\r\n")
        console.expect(b"No default controller available")
        console.expect(PROMPT)
        console.enter("[ $(df -k --output=size /opt/gizlet | tail -n 1) -gt 15000000 ] && echo data-grown")
        console.expect(b"\r\ndata-grown\r\n")
        console.expect(PROMPT)
        # Growing writes no inode tables: the whole first boot writes little to the card.
        console.enter("echo written-$(( $(awk '{print $7}' /sys/block/vda/stat) / 2048 ))-MiB")
        console.expect(b"\r\nwritten-")  # the output line, not the echoed command
        written = int(console.expect(b"-MiB\r\n"))
        console.expect(PROMPT)
        print(f"first boot wrote {written} MiB to a 16 GiB card")
        assert written < 64, f"first boot wrote {written} MiB"
        console.enter("cat /proc/mounts")
        for mount in (b" / squashfs ro,", b" /opt/gizlet ext4 rw,", b" /home ext4 rw,", b" /opt/gizlet/runtime/1 squashfs ro,"):
            console.expect(mount)
        console.expect(PROMPT)
        # Gizlet runtime 1 renders with its own glibc and Mesa; QEMU has no GPU, so llvmpipe.
        console.enter("/opt/gizlet/runtime/1/bin/gizlet-run /opt/gizlet/runtime/1/bin/gizlet-gl-info")
        for line in (b"platform: surfaceless", b"renderer: llvmpipe", b"render: ok"):
            console.expect(line)
        console.expect(PROMPT)
        console.command("mkdir /opt/gizlet/bin")
        console.command("printf '%s\\n' '#!/bin/sh' 'echo hello-from-card' > /opt/gizlet/bin/hello && chmod 755 /opt/gizlet/bin/hello")
        console.command("printf '%s\\n' '[boot]' 'entry=bin/hello' > /opt/gizlet/boot.ini")
        console.command("echo persistent-user-file > /home/check")
        console.command("while [ ! -e /run/gizlet-late.done ]; do sleep 1; done")
        console.stop()
    log = subprocess.run(["mtype", "-i", f"{private}@@{parts['boot'][1]}", "::gizlet.log"],
                         check=True, capture_output=True, text=True).stdout
    for marker in ("== Gizlet OS boot log", "== Gizlet OS services", "data mounts ready", "gizlet-mount-data exit 0", "Linux version"):
        assert marker in log, f"gizlet.log lacks {marker!r}"
    late = subprocess.run(["mtype", "-i", f"{private}@@{parts['boot'][1]}", "::gizlet-late.log"],
                          check=True, capture_output=True, text=True).stdout
    # The late log carries the launcher's own output, captured by boot-session.
    for marker in ("15 s after boot", "== usb devices", "== regulators", "== session output",
                   "Gizlet launcher: no display", "== processes", "== dmesg"):
        assert marker in late, f"gizlet-late.log lacks {marker!r}"
    print("gizlet.log and gizlet-late.log saved on the boot partition")
    # A card powered off while the boot partition was mounted keeps FAT's dirty
    # flag; the first mount of the next boot checks and clears it.
    set_fat_dirty(private, parts["boot"][1], True)
    with boot(private, kernel, root) as console:
        console.expect(b"starting /opt/gizlet/bin/hello")
        console.expect(b"\r\nhello-from-card\r\n")
        console.expect(b"boot entry finished with status 0")
        console.expect(PROMPT)
        console.enter("cat /home/check")
        console.expect(b"\r\npersistent-user-file\r\n")
        console.expect(PROMPT)
        console.enter("dmesg | grep -c 'not properly unmounted'")
        console.expect(b"\r\n0\r\n")
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
        console.command("ln -s /bin/true /opt/gizlet/bin/link")
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
    check_ssh(private, parts["boot"][1], kernel, root, work)
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
        scenarios(Path(sys.argv[1]).resolve(), sys.argv[2], Path(temp))
