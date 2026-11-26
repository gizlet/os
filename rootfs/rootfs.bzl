"""The Gizlet OS base every board shares, for a target's debian_base.

A target adds its board's notices under /usr/share/licenses.
"""

ROOTFS_DEBS = "@debian_trixie_arm64_base//:debs"

ROOTFS_SUITE = "trixie"

# Mount points of the data partition on the read-only root.
ROOTFS_DIRECTORIES = ["opt/gizlet"]

ROOTFS_FILES = {
    "//rootfs:etc/hostname": "etc/hostname",
    "//rootfs:etc/hosts": "etc/hosts",
    "//rootfs:etc/iwd/main.conf": "etc/iwd/main.conf",
    "//rootfs:etc/ssh/sshd_config.d/gizlet.conf": "etc/ssh/sshd_config.d/gizlet.conf",
    "//rootfs:usr/lib/gizlet/inputrc": "usr/lib/gizlet/inputrc",
    "//rootfs:usr/lib/sysctl.d/gizlet.conf": "usr/lib/sysctl.d/gizlet.conf",
    "//rootfs:usr/lib/systemd/system/iwd.service.d/gizlet.conf": "usr/lib/systemd/system/iwd.service.d/gizlet.conf",
    "//rootfs:usr/lib/systemd/system/ssh.service.d/gizlet.conf": "usr/lib/systemd/system/ssh.service.d/gizlet.conf",
    "//rootfs:usr/lib/systemd/journald.conf.d/gizlet.conf": "usr/lib/systemd/journald.conf.d/gizlet.conf",
    "//rootfs:usr/lib/systemd/logind.conf.d/gizlet.conf": "usr/lib/systemd/logind.conf.d/gizlet.conf",
    "//rootfs:usr/lib/systemd/resolved.conf.d/gizlet.conf": "usr/lib/systemd/resolved.conf.d/gizlet.conf",
    "//rootfs:usr/lib/systemd/system.conf.d/gizlet.conf": "usr/lib/systemd/system.conf.d/gizlet.conf",
    "//rootfs:usr/lib/systemd/timesyncd.conf.d/gizlet.conf": "usr/lib/systemd/timesyncd.conf.d/gizlet.conf",
    # systemd-tmpfiles-setup sets schedutil's rate limit at boot.
    "//rootfs:usr/lib/tmpfiles.d/gizlet-cpufreq.conf": "usr/lib/tmpfiles.d/gizlet-cpufreq.conf",
    "//rootfs:usr/lib/systemd/system/gizlet-audio-jack.service": "usr/lib/systemd/system/gizlet-audio-jack.service",
    "//rootfs:usr/lib/systemd/system/gizlet-boot-log.service": "usr/lib/systemd/system/gizlet-boot-log.service",
    "//rootfs:usr/lib/systemd/system/gizlet-data.service": "usr/lib/systemd/system/gizlet-data.service",
    "//rootfs:usr/lib/systemd/system/gizlet-hostname.service": "usr/lib/systemd/system/gizlet-hostname.service",
    "//rootfs:usr/lib/systemd/system/gizlet-power.service": "usr/lib/systemd/system/gizlet-power.service",
    "//rootfs:usr/lib/systemd/system/gizlet-late-log.service": "usr/lib/systemd/system/gizlet-late-log.service",
    "//rootfs:usr/lib/systemd/system/gizlet-late-log.timer": "usr/lib/systemd/system/gizlet-late-log.timer",
    "//rootfs:usr/lib/systemd/system/gizlet-session.service": "usr/lib/systemd/system/gizlet-session.service",
    "//rootfs:usr/lib/systemd/system/gizlet-ssh.service": "usr/lib/systemd/system/gizlet-ssh.service",
    "//rootfs:usr/lib/systemd/system/gizlet-switches.service": "usr/lib/systemd/system/gizlet-switches.service",
    "//rootfs:usr/lib/systemd/system/gizlet-var.service": "usr/lib/systemd/system/gizlet-var.service",
    "//rootfs:usr/lib/udev/rules.d/90-gizlet-hostname.rules": "usr/lib/udev/rules.d/90-gizlet-hostname.rules",
    "@bun//:LICENSE.md": "usr/share/licenses/bun/LICENSE.md",
    "//third_party/glibc:LGPL-2.1": "usr/share/licenses/glibc-LGPL-2.1",
    # Fonts built into the launcher.
    "//launcher:fonts/DroidSansFallback-LICENSE.txt": "usr/share/licenses/gizlet-launcher/DroidSansFallback-LICENSE.txt",
    "//launcher:fonts/Inter-OFL.txt": "usr/share/licenses/gizlet-launcher/Inter-OFL.txt",
    "//launcher:fonts/SpaceGrotesk-OFL.txt": "usr/share/licenses/gizlet-launcher/SpaceGrotesk-OFL.txt",
}

ROOTFS_EXECUTABLES = {
    "//rootfs:usr/lib/gizlet/boot-session": "usr/lib/gizlet/boot-session",
    "//rootfs:usr/lib/gizlet/gizlet-boot-device": "usr/lib/gizlet/gizlet-boot-device",
    "//rootfs:usr/lib/gizlet/gizlet-mount-boot": "usr/lib/gizlet/gizlet-mount-boot",
    "//rootfs:usr/lib/gizlet/gizlet-mount-data": "usr/lib/gizlet/gizlet-mount-data",
    "//rootfs:usr/lib/gizlet/gizlet-runtime": "usr/lib/gizlet/gizlet-runtime",
    "//rootfs:usr/lib/gizlet/gizlet-setup": "usr/lib/gizlet/gizlet-setup",
    "//rootfs:usr/lib/gizlet/gizlet-ssh": "usr/lib/gizlet/gizlet-ssh",
    "//rootfs:usr/lib/gizlet/gizlet-switch": "usr/lib/gizlet/gizlet-switch",
    "//rootfs:usr/lib/gizlet/gizlet-var": "usr/lib/gizlet/gizlet-var",
    "//rootfs:usr/lib/gizlet/save-boot-log": "usr/lib/gizlet/save-boot-log",
    "@bun//:bun": "usr/bin/bun",
    # The launcher runs against Gizlet runtime 1 (its raylib, Mesa and glibc)
    # through its start script; the CLI uses the base system's loader.
    "//launcher:gizlet-launcher": "usr/lib/gizlet/launcher/bin/gizlet-launcher",
    "//launcher:gizlet-cli": "usr/lib/gizlet/launcher/bin/cli",
    "//launcher:start.sh": "usr/lib/gizlet/launcher/start",
    "//rootfs:usr/lib/gizlet/launcher-release": "usr/lib/gizlet/launcher-release",
    "//session:booter": "usr/lib/gizlet/gizlet-booter",
    # Follows the headphone jack; static, like the Booter.
    "//audio:audio-jack": "usr/lib/gizlet/gizlet-audio-jack",
    # Puts the device to sleep when idle or on the power key; static too.
    "//power:power": "usr/lib/gizlet/gizlet-power",
}

ROOTFS_ENABLE = [
    # bluetoothd, wanted by bluetooth.target, which udev pulls in when a
    # controller appears: it starts once one is plugged in, not at boot.
    "bluetooth.service",
    "gizlet-audio-jack.service",
    "gizlet-boot-log.service",
    "gizlet-data.service",
    "gizlet-hostname.service",
    "gizlet-late-log.timer",
    "gizlet-power.service",
    "gizlet-session.service",
    "gizlet-switches.service",
    "gizlet-var.service",
    # The clock from the network once Wi-Fi is up; the RK817 RTC keeps it across reboots.
    "systemd-timesyncd.service",
]

# The read-only base cannot change /etc/localtime, so it points into /run, where
# `gizlet-switch apply-timezone` links the zone chosen in the launcher at boot
# (gizlet-data.service). Until then, or without a data partition, it is UTC.
ROOTFS_SYMLINKS = {
    "etc/localtime": "/run/gizlet-localtime",
    "usr/bin/gizlet": "/usr/lib/gizlet/launcher-release",
}

# Started by gizlet-switches.service when turned on in the launcher, not by
# default. Debian enables sshd. systemd-resolved, which would otherwise start
# beside gizlet-data.service and read the card with it, starts with iwd.
ROOTFS_DISABLE = [
    "ssh.service",
    "systemd-resolved.service",
]

# The session owns the console and the launcher the LCD, so no getty runs.
# Each boot looks like a first boot to systemd (the machine ID is not kept), and
# the read-only base never updates its packages: its /var, in RAM, would
# otherwise look updated on every boot and rebuild the journal catalog before
# the session starts. e2scrub checks ext4 on LVM, which a card does not have,
# yet its reaper takes seconds of every boot.
ROOTFS_MASK = [
    "apt-daily-upgrade.timer",
    "apt-daily.timer",
    "e2scrub_all.timer",
    "e2scrub_reap.service",
    "getty@.service",
    "serial-getty@.service",
    "systemd-firstboot.service",
    "systemd-journal-catalog-update.service",
    "systemd-update-done.service",
]

# Notices under /usr/share/licenses besides each Debian package's copyright file.
ROOTFS_LICENSES = [
    "bun/LICENSE.md",
    "glibc-LGPL-2.1",
    "gizlet-launcher/DroidSansFallback-LICENSE.txt",
    "gizlet-launcher/Inter-OFL.txt",
    "gizlet-launcher/SpaceGrotesk-OFL.txt",
]
