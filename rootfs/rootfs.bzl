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
    "//rootfs:usr/lib/systemd/system/ssh.service.d/gizlet.conf": "usr/lib/systemd/system/ssh.service.d/gizlet.conf",
    "//rootfs:usr/lib/systemd/journald.conf.d/gizlet.conf": "usr/lib/systemd/journald.conf.d/gizlet.conf",
    "//rootfs:usr/lib/systemd/system.conf.d/gizlet.conf": "usr/lib/systemd/system.conf.d/gizlet.conf",
    "//rootfs:usr/lib/systemd/system/gizlet-audio-jack.service": "usr/lib/systemd/system/gizlet-audio-jack.service",
    "//rootfs:usr/lib/systemd/system/gizlet-boot-log.service": "usr/lib/systemd/system/gizlet-boot-log.service",
    "//rootfs:usr/lib/systemd/system/gizlet-data.service": "usr/lib/systemd/system/gizlet-data.service",
    "//rootfs:usr/lib/systemd/system/gizlet-power.service": "usr/lib/systemd/system/gizlet-power.service",
    "//rootfs:usr/lib/systemd/system/gizlet-late-log.service": "usr/lib/systemd/system/gizlet-late-log.service",
    "//rootfs:usr/lib/systemd/system/gizlet-late-log.timer": "usr/lib/systemd/system/gizlet-late-log.timer",
    "//rootfs:usr/lib/systemd/system/gizlet-session.service": "usr/lib/systemd/system/gizlet-session.service",
    "//rootfs:usr/lib/systemd/system/gizlet-ssh.service": "usr/lib/systemd/system/gizlet-ssh.service",
    "//rootfs:usr/lib/systemd/system/gizlet-switches.service": "usr/lib/systemd/system/gizlet-switches.service",
    "//rootfs:usr/lib/systemd/system/gizlet-var.service": "usr/lib/systemd/system/gizlet-var.service",
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
    "//rootfs:usr/lib/gizlet/gizlet-setup": "usr/lib/gizlet/gizlet-setup",
    "//rootfs:usr/lib/gizlet/gizlet-ssh": "usr/lib/gizlet/gizlet-ssh",
    "//rootfs:usr/lib/gizlet/gizlet-switch": "usr/lib/gizlet/gizlet-switch",
    "//rootfs:usr/lib/gizlet/gizlet-var": "usr/lib/gizlet/gizlet-var",
    "//rootfs:usr/lib/gizlet/save-boot-log": "usr/lib/gizlet/save-boot-log",
    # The launcher runs against Gizlet runtime 1 (its raylib, Mesa and glibc)
    # through its start script; the Booter is static.
    "//launcher:gizlet-launcher": "usr/lib/gizlet/gizlet-launcher",
    "//launcher:start.sh": "usr/lib/gizlet/gizlet-launcher-start",
    "//session:booter": "usr/lib/gizlet/gizlet-booter",
    # Follows the headphone jack; static, like the Booter.
    "//audio:audio-jack": "usr/lib/gizlet/gizlet-audio-jack",
    # Turns the screen off when idle; static too.
    "//power:power": "usr/lib/gizlet/gizlet-power",
}

ROOTFS_ENABLE = [
    "gizlet-audio-jack.service",
    "gizlet-boot-log.service",
    "gizlet-data.service",
    "gizlet-late-log.timer",
    "gizlet-power.service",
    "gizlet-session.service",
    "gizlet-switches.service",
    "gizlet-var.service",
    # The clock from the network once Wi-Fi is up; the RK817 RTC keeps it across reboots.
    "systemd-timesyncd.service",
]

# Started by gizlet-switches.service when turned on in the launcher, not by
# default. Debian enables sshd.
ROOTFS_DISABLE = ["ssh.service"]

# The session owns the console and the launcher the LCD, so no getty runs.
# Each boot looks like a first boot to systemd (the machine ID is not kept), and
# the read-only base never updates its packages.
ROOTFS_MASK = [
    "apt-daily-upgrade.timer",
    "apt-daily.timer",
    "getty@.service",
    "serial-getty@.service",
    "systemd-firstboot.service",
]

# Notices under /usr/share/licenses besides each Debian package's copyright file.
ROOTFS_LICENSES = [
    "glibc-LGPL-2.1",
    "gizlet-launcher/DroidSansFallback-LICENSE.txt",
    "gizlet-launcher/Inter-OFL.txt",
    "gizlet-launcher/SpaceGrotesk-OFL.txt",
]
