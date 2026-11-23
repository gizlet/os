"""Tests of a Gizlet OS card image. Both run in the build VM."""

load("//tools/bazel/krun:defs.bzl", "vm_test")

def gizlet_rootfs_test(name, image, booter, data_uuid, licenses, **kwargs):
    """Checks the root and data filesystems, the installed Booter and license notices."""
    vm_test(
        name = name,
        src = Label("//boot:rootfs_test.py"),
        data = [image, booter, Label("//boot:card.py")],
        guest_args = [
            "$(rootpath %s)" % image,
            "$(rootpath %s)" % booter,
            json.encode({"data_uuid": data_uuid, "licenses": licenses}),
        ],
        **kwargs
    )

def gizlet_qemu_test(name, image, kernel = "Image", **kwargs):
    """Boots the card on QEMU virt: recovery, installation, persistence and invalid entries."""
    vm_test(
        name = name,
        src = Label("//boot:qemu_test.py"),
        data = [image, Label("//boot:card.py")],
        guest_args = ["$(rootpath %s)" % image, kernel],
        **kwargs
    )
