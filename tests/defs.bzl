"""Tests of a Gizlet OS card image. All run in the build VM."""

load("//tools/bazel/krun:defs.bzl", "vm_test")

def gizlet_rootfs_test(name, image, booter, data_uuid, licenses, **kwargs):
    """Checks the root and data filesystems, the installed Booter and license notices."""
    vm_test(
        name = name,
        src = Label("//tests:rootfs_test.py"),
        data = [image, booter, Label("//tests:card.py")],
        guest_args = [
            "$(rootpath %s)" % image,
            "$(rootpath %s)" % booter,
            json.encode({"data_uuid": data_uuid, "licenses": licenses}),
        ],
        **kwargs
    )

_SSH_TOOLS = [
    Label("//rootfs/dropbear:dbclient-bin"),
    Label("//rootfs/dropbear:dropbearkey-bin"),
    Label("//rootfs/dropbear:scp-bin"),
]

def gizlet_qemu_test(name, image, kernel = "Image", **kwargs):
    """Boots the card on QEMU virt: recovery, installation, persistence, invalid entries and ssh."""
    vm_test(
        name = name,
        src = Label("//tests:qemu_test.py"),
        data = [image, Label("//tests:card.py")] + _SSH_TOOLS,
        guest_args = ["$(rootpath %s)" % image, kernel] + ["$(rootpath %s)" % tool for tool in _SSH_TOOLS],
        **kwargs
    )

def rockchip_card_test(name, image, bl31, trust_checksum, boot_command, card, **kwargs):
    """Checks a legacy-boot card image in the build VM.

    `card` is a dict with `disk_signature`, `size_mib`, `partitions`,
    `idbloader`, `boot_files` and optional `device_tree`; see card_test.py.
    """
    vm_test(
        name = name,
        src = Label("//tests:card_test.py"),
        data = [image, bl31, trust_checksum, boot_command],
        guest_args = [
            "$(rootpath %s)" % image,
            "$(rootpath %s)" % bl31,
            "$(rootpath %s)" % trust_checksum,
            "$(rootpath %s)" % boot_command,
            json.encode(card),
        ],
        **kwargs
    )
