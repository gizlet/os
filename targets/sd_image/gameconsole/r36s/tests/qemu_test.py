"""Boot a Gizlet OS card image on QEMU virt and exercise installation and recovery.

Usage: qemu_test.py IMAGE KERNEL_NAME FLASH_SH FLASH_CARD_SH

KERNEL_NAME is the kernel file on the card's boot partition. The kernel boots
the card's SquashFS partition, which the test finds through the MBR, with
systemd. The build VM's OpenSSH client logs in to the guest's sshd over a
forwarded virtio-net port.
"""

import base64
from contextlib import contextmanager
import hashlib
import io
import json
import os
from pathlib import Path
import selectors
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import zipfile

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
        # gizlet-switches.service imports setup.ini while the console is already up,
        # and starts sshd, whose gizlet-ssh.service writes to the console too.
        console.command("for i in $(seq 60); do [ \"$(systemctl show -P ActiveState gizlet-switches)\" != activating ] && "
                        "systemctl -q is-active ssh && break; sleep 1; done")
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
        # Without a Wi-Fi adapter the virtio-net adapter's MAC address, QEMU's
        # 52:54:00:12:34:56, names the device, and systemd-resolved, started
        # with iwd, answers for it by mDNS on eth0. Over ssh it takes another
        # name, lowercased; names that are not one DNS label are refused.
        names = subprocess.run([client, "root@127.0.0.1", "hostname; resolvectl status eth0; ss -ulnH 'sport = :5353' | wc -l; "
                                "for n in -x x- a_b ''$(printf %064d 0); do /usr/lib/gizlet/gizlet-switch hostname \"$n\" 2>/dev/null || echo refused; done; "
                                "/usr/lib/gizlet/gizlet-switch hostname R36S-Test && hostname && /usr/lib/gizlet/gizlet-switch hostname"],
                               stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=60).stdout
        assert names.startswith("gizlet-3456\n"), f"default hostname: {names}"
        assert "mDNS/IPv4" in names, f"no mDNS on eth0: {names}"
        assert names.endswith("refused\n" * 4 + "r36s-test\nr36s-test\n"), f"renaming: {names}"
        assert int(names.split("\n")[-8]) > 0, f"nothing listens on UDP 5353: {names}"
        payload = work / "payload"
        payload.write_bytes(os.urandom(65536))
        subprocess.run(["scp", "-O", "-S", client, payload, "root@127.0.0.1:/home/payload"],
                       stdin=subprocess.DEVNULL, check=True, timeout=30)
        back = work / "back"
        subprocess.run(["scp", "-O", "-S", client, "root@127.0.0.1:/home/payload", back],
                       stdin=subprocess.DEVNULL, check=True, timeout=30)
        assert back.read_bytes() == payload.read_bytes(), "scp round trip changed the file"
        console.command("rm /home/payload")
        console.enter("/usr/lib/gizlet/launcher-release start --probe")
        console.expect(b"wifi: Ok(WifiState { on: true")
        console.expect(b'ssh: Ok(SshState { on: true, running: true, keys: Some(1), password: Some("gizlet test") })')
        console.expect(b'clock: Ok(ClockState { zone: "Asia/Shanghai", synced: ')
        console.expect(b'hostname: "r36s-test"')
        console.expect(PROMPT)
        console.stop()
    # Both switches survive a reboot, and turning them off stops the services.
    with boot(private, kernel, root) as console:
        console.expect(PROMPT)
        # gizlet-switches.service starts them without holding up boot; wait for both.
        console.enter("for i in $(seq 60); do systemctl -q is-active ssh && systemctl -q is-active iwd && break; sleep 1; done; "
                      "systemctl is-active ssh iwd systemd-resolved")
        console.expect(b"\r\nactive\r\nactive\r\nactive\r\n")
        console.expect(PROMPT)
        # The chosen name holds after a reboot, here without a network adapter.
        console.enter("for i in $(seq 30); do [ \"$(hostname)\" = r36s-test ] && break; sleep 1; done; echo \"[$(hostname)]\"")
        console.expect(b"\r\n[r36s-test]\r\n")
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
        # An empty name goes back to the default: gizlet, without a network adapter.
        console.enter("/usr/lib/gizlet/gizlet-switch hostname '' && echo \"[$(hostname)]\"")
        console.expect(b"\r\n[gizlet]\r\n")
        console.expect(PROMPT)
        console.stop()
    print("setup.ini (imported once) and the launcher's switches start sshd and iwd and set the time zone and hostname, "
          "also after a reboot; mDNS runs with Wi-Fi; key and password logins and scp work")


def gzl(files, listed=None):
    """A .gzl package as the store's gizlet-cli packs it: FILES (name -> (bytes, mode)) and
    CHECKSUMS, computed from LISTED instead when given."""
    sums = "".join(f"{hashlib.sha256(data).hexdigest()}  {name}\n" for name, (data, _) in sorted((listed or files).items()))
    out = io.BytesIO()
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        for name, (data, mode) in [*sorted(files.items()), ("CHECKSUMS", (sums.encode(), 0o644))]:
            info = zipfile.ZipInfo(name, (2024, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = (0o100000 | mode) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, data)
    return out.getvalue()


def send(console, data, path):
    """Writes DATA to PATH in the guest through the console, in base64 lines."""
    text = base64.b64encode(data).decode()
    console.command(f"rm -f {path}.b64")
    for i in range(0, len(text), 600):
        console.command(f"printf '%s\\n' '{text[i:i + 600]}' >> {path}.b64")
    console.command(f"base64 -d {path}.b64 > {path} && rm {path}.b64")


def check_apps(console):
    """Installs, lists, runs and removes an app with the gizlet command, and refuses a tampered package."""
    console.enter("gizlet")
    console.expect(b"usage: gizlet <command>")
    console.expect(PROMPT)
    console.enter("/usr/lib/gizlet/launcher/bin/cli target")
    console.expect(b"\r\naarch64-unknown-linux-gnu lite\r\n")
    console.expect(PROMPT)
    manifest = {
        "schema": 1,
        "id": "dev.gizlet.test-hello",
        "name": "Hello",
        "names": {"zh-CN": "你好"},
        "version": "0.1.0",
        "runtime": 1,
        "target": "aarch64-unknown-linux-gnu",
        "tier": "lite",
        "entry": "bin/hello.js",
    }
    hello = (b'#!/usr/bin/bun\n'
             b'import { EOL } from "node:os";\n'
             b'const { GIZLET_SWAP_AB, GIZLET_SAVE_DIR } = process.env;\n'
             b'console.log("hello-app", GIZLET_SWAP_AB, process.cwd(), GIZLET_SAVE_DIR);\n'
             b'await Bun.write(`${GIZLET_SAVE_DIR}/kept`, "kept" + EOL);\n')
    files = {"manifest.json": (json.dumps(manifest).encode(), 0o644), "bin/hello.js": (hello, 0o755), "assets/a.txt": (b"asset\n", 0o644)}
    send(console, gzl(files), "/tmp/hello.gzl")
    # A file changed after packing no longer matches CHECKSUMS.
    send(console, gzl(dict(files, **{"assets/a.txt": (b"other\n", 0o644)}), listed=files), "/tmp/bad.gzl")
    console.enter("gizlet install /tmp/bad.gzl; echo status-$?")
    console.expect(b"does not match its checksum")
    console.expect(b"\r\nstatus-1\r\n")
    console.expect(PROMPT)
    console.enter("gizlet install /tmp/hello.gzl")
    console.expect(b"\r\ninstalled dev.gizlet.test-hello 0.1.0 (Hello) in /opt/gizlet/apps/dev.gizlet.test-hello")
    console.expect(PROMPT)
    console.enter("gizlet list")
    console.expect(b"\r\ndev.gizlet.test-hello\t0.1.0\tHello\r\n")
    console.expect(PROMPT)
    # The kernel keeps all but critical messages off the console the LCD shows between apps.
    console.enter("cat /proc/sys/kernel/printk")
    console.expect(b"\r\n3\t4\t1\t7\r\n")
    console.expect(PROMPT)
    console.enter("bun -e 'process.stdout.write(String(6 * 7) + \"\\n\")'")
    console.expect(b"\r\n42\r\n")
    console.expect(PROMPT)
    console.enter("gizlet run dev.gizlet.test-hello")
    console.expect(b"\r\nhello-app 0 /opt/gizlet/apps/dev.gizlet.test-hello /opt/gizlet/saves/dev.gizlet.test-hello\r\n")
    console.expect(PROMPT)
    # Without a launcher on a screen, start says to use run.
    console.enter("gizlet start dev.gizlet.test-hello; echo status-$?")
    console.expect(b"no launcher is on the screen")
    console.expect(b"\r\nstatus-1\r\n")
    console.expect(PROMPT)
    console.enter("gizlet uninstall dev.gizlet.test-hello && cat /opt/gizlet/saves/dev.gizlet.test-hello/kept")
    console.expect(b"\r\nremoved dev.gizlet.test-hello; its saves stay")
    console.expect(b"\r\nkept\r\n")
    console.expect(PROMPT)
    console.enter("gizlet list | wc -l; ls -A /opt/gizlet/apps | grep -vxE '[.](lock|running-.*)' | wc -l")
    # Nothing is left but the lock files: no staging, removal or backup folders.
    console.expect(b"\r\n0\r\n0\r\n")
    console.expect(PROMPT)
    print("gizlet installs, lists, runs and removes a .gzl app and refuses one that fails its checksums")


def install_launcher_updates(console, pack_release):
    console.command("set +H; mkdir -p /opt/gizlet/launcher /tmp/updates && cd /tmp/updates")
    send(console, pack_release.read_bytes(), "/tmp/updates/pack.sh")
    console.command("cp /usr/lib/gizlet/launcher/start start.sh && ssh-keygen -qt ed25519 -N '' -f test-key")
    console.command("(printf 'launcher '; cat test-key.pub) > /opt/gizlet/launcher/allowed_signers")
    console.command('pack() { printf "#!/bin/sh\\necho cli-%s\\n" "$1" > cli; '
                    'sh pack.sh "$1" test-key /usr/lib/gizlet/launcher/bin/gizlet-launcher cli "$1.gizlet"; }')
    for version in ("1.0.0", "1.1.0", "1.2.0"):
        console.enter(f"pack {version} && gizlet update {version}.gizlet")
        console.expect(f"Installed Launcher release {version}".encode())
        console.expect(PROMPT)
    console.enter("pack 1.10.0 && openssl req -x509 -newkey ed25519 -nodes -days 1 -subj /CN=localhost "
                  "-addext subjectAltName=DNS:localhost -keyout tls.key -out tls.crt && echo tls-ready")
    console.expect(b"\r\ntls-ready\r\n")
    console.expect(PROMPT)
    console.command("openssl s_server -quiet -WWW -accept 8443 -cert tls.crt -key tls.key </dev/null >/tmp/tls.log 2>&1 &")
    console.enter("CURL_CA_BUNDLE=/tmp/updates/tls.crt gizlet update https://localhost:8443/1.10.0.gizlet")
    console.expect(b"Installed Launcher release 1.10.0")
    console.expect(PROMPT)
    console.enter("pack 2.0.0 && printf x >> 2.0.0.gizlet; gizlet update 2.0.0.gizlet; echo status-$?")
    console.expect(b"signature verification failed")
    console.expect(b"\r\nstatus-1\r\n")
    console.expect(PROMPT)
    console.command("mkdir -p /opt/gizlet/launcher/releases/9.0.0 /opt/gizlet/launcher/releases/.install.interrupted")
    console.enter("gizlet target && LC_ALL=C ls -1 /opt/gizlet/launcher/releases")
    console.expect(b"\r\naarch64-unknown-linux-gnu lite\r\n1.1.0\r\n1.10.0\r\n1.2.0\r\n9.0.0\r\n")
    console.expect(PROMPT)


def check_runtime_reload(console):
    """New apps use the active mount and a running process keeps its own runtime path."""
    manifest = {
        "id": "dev.gizlet.test-runtime",
        "name": "Runtime path",
        "version": "1.0.0",
        "runtime": 1,
        "target": "aarch64-unknown-linux-gnu",
        "tier": "lite",
        "entry": "bin/printenv",
    }
    send(console, gzl({"manifest.json": (json.dumps(manifest).encode(), 0o644),
                      "bin/printenv": (b"replaced in the guest", 0o755)}), "/tmp/runtime.gzl")
    console.command("gizlet install /tmp/runtime.gzl && cp /usr/bin/printenv /opt/gizlet/apps/dev.gizlet.test-runtime/bin/printenv")
    console.enter("gizlet run dev.gizlet.test-runtime ALSA_CONFIG_DIR")
    console.expect(b"\r\n/opt/gizlet/runtime/1/share/alsa\r\n")
    console.expect(PROMPT)
    console.command("runtime=/opt/gizlet/runtime")
    console.enter('gizlet system reload-runtime "$runtime/1.sqfs"')
    console.expect(b"\r\nRuntime 1 activated.")
    console.expect(PROMPT)
    console.command('first=$(readlink -e "$runtime/1-current")')
    console.enter('test "$(gizlet run dev.gizlet.test-runtime ALSA_CONFIG_DIR)" = "$first/share/alsa" && echo first-runtime')
    console.expect(b"\r\nfirst-runtime\r\n")
    console.expect(PROMPT)
    console.command('"$runtime/1-current/bin/gizlet-run" /bin/sh -c '
                    '\'touch /tmp/runtime-running; while [ ! -f /tmp/runtime-continue ]; do sleep .1; done; '
                    'test -r "$ALSA_CONFIG_PATH" && echo "$ALSA_CONFIG_DIR" > /tmp/pinned-runtime\' & '
                    'old_runtime_pid=$!')
    console.enter("timeout 15 sh -c 'while [ ! -f /tmp/runtime-running ]; do sleep .1; done' && echo runtime-running")
    console.expect(b"\r\nruntime-running\r\n")
    console.expect(PROMPT)
    console.enter('gizlet system reload-runtime "$runtime/1.sqfs"')
    console.expect(b"\r\nRuntime 1 activated.")
    console.expect(PROMPT)
    console.command('second=$(readlink -e "$runtime/1-current") && touch /tmp/runtime-continue')
    console.enter('wait "$old_runtime_pid" && test "$first" != "$second" && mountpoint -q "$first" '
                  '&& test "$(cat /tmp/pinned-runtime)" = "$first/share/alsa" '
                  '&& test "$(gizlet run dev.gizlet.test-runtime ALSA_CONFIG_DIR)" = "$second/share/alsa" '
                  '&& echo runtime-pinned')
    console.expect(b"\r\nruntime-pinned\r\n")
    console.expect(PROMPT)
    console.enter('echo invalid > /tmp/invalid.sqfs; gizlet system reload-runtime /tmp/invalid.sqfs; echo status-$?')
    console.expect(b"\r\nstatus-1\r\n")
    console.expect(PROMPT)
    console.enter('test "$(readlink -e "$runtime/1-current")" = "$second" '
                  '&& test "$(ls -d "$runtime"/.runtime-1.* | wc -l)" -eq 2 '
                  '&& printf "%s\\n" "$second" > /home/active-runtime && echo reload-rejected')
    console.expect(b"\r\nreload-rejected\r\n")
    console.expect(PROMPT)


def check_runtime_after_reboot(console):
    console.enter('runtime=/opt/gizlet/runtime; active=$(cat /home/active-runtime) '
                  '&& mountpoint -q "$active" && test "$(ls -d "$runtime"/.runtime-1.* | wc -l)" -eq 1 '
                  '&& test "$(gizlet run dev.gizlet.test-runtime ALSA_CONFIG_DIR)" = "$active/share/alsa" '
                  '&& echo runtime-restored')
    console.expect(b"\r\nruntime-restored\r\n")
    console.expect(PROMPT)
    console.command("gizlet uninstall dev.gizlet.test-runtime --saves && rm /home/active-runtime")


def start_ota_fixture(console, pack_release, ota_fixture, tag):
    console.command("mkdir -p /tmp/updates && cd /tmp/updates && ssh-keygen -qt ed25519 -N '' -f test-key "
                    "&& mkdir -p /opt/gizlet/launcher "
                    "&& (printf 'launcher '; cat test-key.pub) > /opt/gizlet/launcher/allowed_signers")
    send(console, pack_release.read_bytes(), "/tmp/updates/pack.sh")
    send(console, ota_fixture.read_bytes(), "/tmp/updates/ota_fixture.sh")
    console.enter("sh /tmp/updates/ota_fixture.sh && echo ota-fixture-ready")
    console.expect(b"\r\nota-fixture-ready\r\n")
    console.expect(PROMPT)
    console.command(f"cd /tmp/updates/ota && echo {tag} > latest && (bun serve.js </dev/null >/tmp/ota-server.log 2>&1 &)")
    console.command("export CURL_CA_BUNDLE=$PWD/tls.crt PATH=$PWD/bin:$PATH")
    console.enter("timeout 15 sh -c 'until curl -qfsS https://dl.gizlet.io/; do sleep 1; done' && echo ota-https-ready")
    console.expect(b"readyota-https-ready")
    console.expect(PROMPT)


def stage_ota_updates(console, pack_release, ota_fixture):
    start_ota_fixture(console, pack_release, ota_fixture, "v2.0.0")
    for fault, message, action in (
        ("signature", b"Release signature verification failed", "check"),
        ("interrupt", b"Download failed", "install v2.0.0"),
        ("checksum", b"Update file checksum mismatch", "install v2.0.0"),
    ):
        console.enter(f"echo {fault} > fault; gizlet system ota {action}; "
                      "test $? -ne 0 && test ! -L /opt/gizlet/ota/pending && echo ota-refused")
        console.expect(message)
        console.expect(b"\r\nota-refused\r\n")
        console.expect(PROMPT)
    console.enter(": > fault; gizlet system ota check && gizlet system ota install v2.0.0")
    console.expect(b"Available: v2.0.0 (Runtime: true, Launcher: true")
    console.expect(b"Restart to finish installing v2.0.0")
    console.expect(PROMPT)
    console.enter("gizlet system ota status")
    console.expect(b"\r\nLauncher: factory\r\n")
    console.expect(b"\r\nPending restart: v2.0.0\r\n")
    console.expect(PROMPT)
    console.command("umount /usr/lib/gizlet/launcher/allowed_signers")


def check_ota_after_boot(console, pack_release, ota_fixture):
    console.expect(b"\r\nota-confirmed\r\n")
    console.expect(PROMPT)
    console.enter("gizlet system ota status && current=$(readlink /opt/gizlet/ota/current) && echo \"${current%.*}\" && "
                  "mountpoint -q \"/opt/gizlet/ota/$current/root\" && gizlet target && "
                  "test \"$(gizlet run dev.gizlet.ota-test ALSA_CONFIG_DIR)\" = "
                  "\"/opt/gizlet/ota/$current/root/share/alsa\" && echo ota-runtime-in-use")
    console.expect(b"\r\nLauncher: 2.0.0\r\n")
    console.expect(b"\r\nv2.0.0\r\naarch64-unknown-linux-gnu lite\r\nota-runtime-in-use\r\n")
    console.expect(PROMPT)
    start_ota_fixture(console, pack_release, ota_fixture, "v2.0.1")
    console.enter("gizlet system ota install v2.0.1")
    console.expect(b"Restart to finish installing v2.0.1")
    console.expect(PROMPT)


def ota_scenarios(image, kernel_name, flash, flash_card, pack_release, ota_fixture, work):
    private = work / "ota.img"
    parts, signature = card.partitions(image)
    root = f"PARTUUID={signature:08x}-{parts['root'][0]:02x}"
    kernel = work / "kernel"
    subprocess.run(["mcopy", "-i", f"{image}@@{parts['boot'][1]}", "::" + kernel_name, kernel], check=True)
    subprocess.run(["bash", flash_card, flash, image, private, str(2 << 30)], check=True)
    with boot(private, kernel, root) as console:
        console.expect(PROMPT)
        manifest = {
            "id": "dev.gizlet.ota-test",
            "name": "Runtime path",
            "version": "1.0.0",
            "runtime": 1,
            "target": "aarch64-unknown-linux-gnu",
            "tier": "lite",
            "entry": "bin/printenv",
        }
        send(console, gzl({"manifest.json": (json.dumps(manifest).encode(), 0o644),
                          "bin/printenv": (b"replaced in the guest", 0o755)}), "/tmp/ota-app.gzl")
        console.command("gizlet install /tmp/ota-app.gzl && cp /usr/bin/printenv /opt/gizlet/apps/dev.gizlet.ota-test/bin/printenv")
        console.command("echo persistent-user-file > /home/check && "
                        "mkdir -p /opt/gizlet/saves/dev.gizlet.ota-test && "
                        "echo kept > /opt/gizlet/saves/dev.gizlet.ota-test/kept && "
                        "/usr/lib/gizlet/gizlet-switch timezone America/New_York")
        stage_ota_updates(console, pack_release, ota_fixture)
        console.stop()
    with boot(private, kernel, root) as console:
        check_ota_after_boot(console, pack_release, ota_fixture)
        console.command("mkdir -p /opt/gizlet/bin")
        send(console, b"""#!/bin/sh
set -eu
gizlet system ota status
current=$(readlink /opt/gizlet/ota/current)
test "$(gizlet run dev.gizlet.ota-test ALSA_CONFIG_DIR)" = "/opt/gizlet/ota/$current/root/share/alsa"
test ! -L /opt/gizlet/ota/attempt
echo custom-entry-uses-ota
""", "/opt/gizlet/bin/ota-check")
        console.command("chmod 755 /opt/gizlet/bin/ota-check && "
                        "printf '%s\\n' '[boot]' 'entry=bin/ota-check' > /opt/gizlet/boot.ini")
        console.stop()
    with boot(private, kernel, root) as console:
        console.expect(b"\r\nLauncher: 2.0.0\r\n")
        console.expect(b"\r\nPending restart: v2.0.1\r\ncustom-entry-uses-ota\r\n")
        console.expect(PROMPT)
        console.command("rm /opt/gizlet/boot.ini")
        console.enter("exit")
        console.expect(b"ota-launch-failed")
        console.expect(b"update v2.0.1 rolled back")
        console.expect(b"ota-confirmed")
        console.expect(PROMPT)
        console.enter("gizlet system ota status; current=$(readlink /opt/gizlet/ota/current); echo \"${current%.*}\"; "
                      "cat /home/check /opt/gizlet/saves/dev.gizlet.ota-test/kept; "
                      "/usr/lib/gizlet/gizlet-switch timezone; "
                      "test \"$(gizlet run dev.gizlet.ota-test ALSA_CONFIG_DIR)\" = "
                      "\"/opt/gizlet/ota/$current/root/share/alsa\" && echo ota-runtime-in-use")
        console.expect(b"\r\nLauncher: 2.0.0\r\n")
        console.expect(b"\r\nRolled back: v2.0.1\r\nv2.0.0\r\npersistent-user-file\r\nkept\r\n"
                       b"America/New_York\r\nota-runtime-in-use\r\n")
        console.expect(PROMPT)
        console.enter("/usr/lib/gizlet/gizlet-ota-boot restore 2.1.0 && "
                      "test ! -L /run/gizlet-ota-launcher && test -L /run/gizlet-ota-runtime && "
                      "echo newer-factory-launcher-selected")
        console.expect(b"\r\nnewer-factory-launcher-selected\r\n")
        console.expect(PROMPT)
        console.stop()
    print("signed OTA verifies HTTPS downloads, activates a matching runtime, Launcher and CLI, and preserves user data after rollback")


def scenarios(image, kernel_name, flash, flash_card, pack_release, work):
    parts, signature = card.partitions(image)
    root = f"PARTUUID={signature:08x}-{parts['root'][0]:02x}"
    kernel = work / "kernel"
    subprocess.run(["mcopy", "-i", f"{image}@@{parts['boot'][1]}", "::" + kernel_name, kernel], check=True)
    # A card written by another tool, larger than the image: the first boot
    # extends the data partition and grows its filesystem before mounting it.
    other = work / "other-tool.img"
    shutil.copyfile(image, other)
    os.truncate(other, 2 << 30)
    with boot(other, kernel, root) as console:
        console.expect(PROMPT)
        console.enter("cat /run/gizlet-boot.log; [ $(df -k --output=size /opt/gizlet | tail -n 1) -gt 1500000 ] && echo data-grown")
        console.expect(b"data partition grown to ")
        console.expect(b"data filesystem grown to fill its partition")
        console.expect(b"\r\ndata-grown\r\n")
        console.expect(PROMPT)
        console.stop()
    other.unlink()
    # A larger card, written as flash.sh writes it: partition 3 extended to the
    # card's end and its filesystem grown to fill it, so the first boot grows nothing.
    private = work / "private.img"
    card_bytes = 16 << 30
    subprocess.run(["bash", flash_card, flash, image, private, str(card_bytes)], check=True)
    with boot(private, kernel, root) as console:
        # A fresh card starts the factory launcher. QEMU's machine has no
        # display, so it says so and the recovery shell opens.
        console.expect(b"starting the selected launcher")
        console.expect(b"Gizlet launcher 0.")
        console.expect(b"Gizlet launcher: no display")
        console.expect(b"boot entry finished with status 1")
        console.expect(PROMPT)
        # Its device probe runs without a display: no battery, backlight, sound or WiFi adapter here.
        # No Sleep either: gizlet-power, which suspends, needs a backlight.
        console.enter("/usr/lib/gizlet/launcher-release start --probe")
        for line in (b"battery: Err(NoHardware)", b"brightness: Err(NoHardware)", b"volume: Err(NoHardware)",
                     b"wifi: Ok(WifiState { on: false, adapter: false", b"ssh: Ok(SshState { on: false, running: false",
                     b"power: [Reboot, PowerOff]"):
            console.expect(line)
        console.expect(PROMPT)
        # A fresh card keeps UTC until a time zone is chosen; network time runs without a network.
        # systemd-resolved waits for Wi-Fi, and nothing scrubs ext4.
        console.enter("date +%Z; /usr/lib/gizlet/gizlet-switch timezone; systemctl is-active systemd-timesyncd systemd-resolved e2scrub_reap")
        console.expect(b"\r\nUTC\r\nEtc/UTC\r\nactive\r\ninactive\r\ninactive\r\n")
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
        console.enter("[ $(df -k --output=size /opt/gizlet | tail -n 1) -gt 15000000 ] && echo data-full; grep -c grown /run/gizlet-boot.log")
        console.expect(b"\r\ndata-full\r\n0\r\n")
        console.expect(PROMPT)
        # The first boot writes little to the card: no growth, no inode tables.
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
        # Gizlet runtime 1 creates an ES 3 context with llvmpipe, since QEMU has no GPU.
        console.enter("/opt/gizlet/runtime/1/bin/gizlet-run /opt/gizlet/runtime/1/bin/gizlet-gl-info")
        for line in (b"platform: surfaceless", b"renderer: llvmpipe", b"version: OpenGL ES 3.", b"render: ok"):
            console.expect(line)
        console.expect(PROMPT)
        check_apps(console)
        check_runtime_reload(console)
        console.command("mkdir /opt/gizlet/bin")
        console.command("printf '%s\\n' '#!/bin/sh' 'echo hello-from-card' > /opt/gizlet/bin/hello && chmod 755 /opt/gizlet/bin/hello")
        console.command("printf '%s\\n' '[boot]' 'entry=bin/hello' > /opt/gizlet/boot.ini")
        console.command("echo persistent-user-file > /home/check")
        console.command("while [ ! -e /run/gizlet-late.done ]; do sleep 1; done")
        console.stop()
    log = subprocess.run(["mtype", "-i", f"{private}@@{parts['boot'][1]}", "::gizlet.log"],
                         check=True, capture_output=True, text=True).stdout
    for marker in ("== Gizlet OS boot log", "== Gizlet OS services", "data mounts ready", "gizlet-mount-data exit 0", "Linux version",
                   "gizlet-mount-data steps: start="):
        assert marker in log, f"gizlet.log lacks {marker!r}"
    print(next(line for line in log.splitlines() if line.startswith("gizlet-mount-data steps:")))
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
        check_runtime_after_reboot(console)
        console.enter("cat /home/check")
        console.expect(b"\r\npersistent-user-file\r\n")
        console.expect(PROMPT)
        console.enter("dmesg | grep -c 'not properly unmounted'")
        console.expect(b"\r\n0\r\n")
        console.expect(PROMPT)
        console.command("printf '%s\\n' '#!/usr/bin/bun' 'console.log(\"js-from-card\");' > /opt/gizlet/bin/hello.js "
                        "&& chmod 755 /opt/gizlet/bin/hello.js "
                        "&& printf '%s\\n' '[boot]' 'entry=bin/hello.js' > /opt/gizlet/boot.ini")
        console.enter("exit")
        console.expect(b"starting /opt/gizlet/bin/hello.js")
        console.expect(b"\r\njs-from-card\r\n")
        console.expect(b"boot entry finished with status 0")
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
    with boot(private, kernel, root) as console:
        console.expect(PROMPT)
        install_launcher_updates(console, pack_release)
        console.command("printf '%s\\n' '[boot]' 'entry=bin/hello' > /opt/gizlet/boot.ini")
        console.stop()
    # The SD-card update keeps releases, settings and custom boot entries.
    subprocess.run(["bash", flash_card, "--update", flash, image, private], check=True)
    with boot(private, kernel, root) as console:
        console.expect(b"starting /opt/gizlet/bin/hello")
        console.expect(b"\r\nhello-from-card\r\n")
        console.expect(PROMPT)
        console.enter("gizlet target; cat /home/check; /usr/lib/gizlet/gizlet-switch timezone; "
                      "test ! -e /opt/gizlet/launcher/releases/.install.interrupted && echo staging-cleaned")
        console.expect(b"\r\ncli-1.10.0\r\npersistent-user-file\r\nAmerica/New_York\r\nstaging-cleaned\r\n")
        console.expect(PROMPT)
        # A CRLF boot.ini from older images still selects the matching Launcher.
        console.command("printf '%s\\r\\n' '[boot]' 'entry=launcher/gizlet-launcher' > /opt/gizlet/boot.ini")
        console.enter("exit")
        console.expect(b"using Launcher from /opt/gizlet/launcher/releases/1.10.0")
        console.expect(b"Gizlet launcher: no display")
        console.expect(PROMPT)
        # A boot.ini that is a FIFO is refused without blocking the session.
        console.command("rm /opt/gizlet/boot.ini && mkfifo /opt/gizlet/boot.ini")
        console.enter("exit")
        console.expect(b"invalid boot.ini")
        console.expect(PROMPT)
        console.command("rm /opt/gizlet/boot.ini")
        console.stop()
    print("signed updates select a matching Launcher and CLI and keep custom entries and user data")
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
        common = (Path(sys.argv[1]).resolve(), sys.argv[2], sys.argv[3], sys.argv[4], Path(sys.argv[5]))
        if len(sys.argv) == 7:
            ota_scenarios(*common, Path(sys.argv[6]), Path(temp))
        else:
            scenarios(*common, Path(temp))
