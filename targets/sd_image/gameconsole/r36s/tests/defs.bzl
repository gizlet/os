"""Tests of a Gizlet OS card image. All run in the build VM."""

load("//devops/bazel/krun:defs.bzl", "vm_test")

def gizlet_rootfs_test(name, image, booter, flash, data_uuid, licenses, **kwargs):
    """Checks the root and data filesystems, the installed Booter and license notices.

    `flash` is the card writer whose offline growth of the data filesystem is checked.
    """
    vm_test(
        name = name,
        src = Label(":rootfs_test.py"),
        data = [image, booter, flash, Label(":card.py"), Label(":flash_card.sh")],
        guest_args = [
            "$(rootpath %s)" % image,
            "$(rootpath %s)" % booter,
            json.encode({"data_uuid": data_uuid, "licenses": licenses}),
            "$(rootpath %s)" % flash,
            "$(rootpath %s)" % Label(":flash_card.sh"),
        ],
        **kwargs
    )

def gizlet_qemu_test(name, image, flash, kernel = "Image", **kwargs):
    """Boots the card on QEMU virt: recovery, installation, persistence, invalid entries and ssh.

    The card is written as `flash`, the card writer, writes it.
    """
    vm_test(
        name = name,
        src = Label(":qemu_test.py"),
        data = [image, flash, Label(":card.py"), Label(":flash_card.sh"), Label("//launcher:pack_release.sh")],
        guest_args = [
            "$(rootpath %s)" % image,
            kernel,
            "$(rootpath %s)" % flash,
            "$(rootpath %s)" % Label(":flash_card.sh"),
            "$(rootpath %s)" % Label("//launcher:pack_release.sh"),
        ],
        **kwargs
    )

def rockchip_card_test(name, image, bl31, trust_checksum, boot_command, card, **kwargs):
    """Checks a legacy-boot card image in the build VM.

    `card` is a dict with `disk_signature`, `size_mib`, `partitions`,
    `idbloader`, `boot_files` and optional `device_tree`; see card_test.py.
    """
    vm_test(
        name = name,
        src = Label(":card_test.py"),
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
