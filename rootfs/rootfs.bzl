"""The userspace every board shares: BusyBox and what is installed beside it.

A target composes its root filesystem from these and adds its board's notices.
"""

ROOTFS_BUSYBOX = "@busybox//:busybox-1.37.0.tar.bz2"

ROOTFS_BUSYBOX_CONFIG = "//rootfs:busybox.config"

ROOTFS_DIRECTORIES = [
    "dev",
    "etc",
    "proc",
    "sys",
    "run",
    "tmp",
    "home",
    "opt/gizlet",
    "sbin",
    "usr/bin",
    "usr/sbin",
    "usr/share/licenses",
]

ROOTFS_EXECUTABLES = {
    "//rootfs:etc/rcS": "etc/rcS",
    "//rootfs:etc/udhcpc.script": "etc/udhcpc.script",
    "//rootfs:sbin/gizlet-boot-device": "sbin/gizlet-boot-device",
    "//rootfs:sbin/gizlet-net": "sbin/gizlet-net",
    "//rootfs:sbin/save-boot-log": "sbin/save-boot-log",
    "//rootfs/dropbear:dropbear-bin": "usr/sbin/dropbear",
    "//rootfs/dropbear:dropbearkey-bin": "usr/bin/dropbearkey",
    "//rootfs/dropbear:scp-bin": "usr/bin/scp",
    "//rootfs/wpa_supplicant:wpa_cli-bin": "usr/bin/wpa_cli",
    "//rootfs/wpa_supplicant:wpa_supplicant-bin": "usr/sbin/wpa_supplicant",
    "//session:boot-session": "sbin/boot-session",
    "//session:booter": "sbin/gizlet-booter",
}

ROOTFS_FILES = {
    "//rootfs:etc/group": "etc/group",
    "//rootfs:etc/inittab": "etc/inittab",
    "//rootfs:etc/nsswitch.conf": "etc/nsswitch.conf",
    "//rootfs:etc/passwd": "etc/passwd",
    "//rootfs:etc/shells": "etc/shells",
    "//rootfs/dropbear:LICENSE": "usr/share/licenses/dropbear",
    "//rootfs/wpa_supplicant:LICENSE": "usr/share/licenses/wpa_supplicant",
    "//rootfs/wpa_supplicant:libnl-LICENSE": "usr/share/licenses/libnl",
    "//third_party/glibc:LGPL-2.1": "usr/share/licenses/glibc-LGPL-2.1",
}

# Notices under /usr/share/licenses: those above and BusyBox's own.
ROOTFS_LICENSES = ["BusyBox", "dropbear", "glibc-LGPL-2.1", "libnl", "wpa_supplicant"]
