"""Builds the read-only root image of the build VM."""

load("//tools/bazel/krun:defs.bzl", "KRUN_ATTRS", "krun_action")

def _debian_rootfs_impl(ctx):
    out = ctx.actions.declare_file(ctx.attr.name + ".sqfs")
    inputs = {ctx.file.script: "build_rootfs.sh"}
    for deb in ctx.files.debs:
        inputs[deb] = "debs/" + deb.basename
    busybox = ctx.file.bootstrap_busybox
    krun_action(
        ctx,
        root_dir = busybox.dirname.rpartition("/")[0],
        inputs = inputs,
        outputs = [out],
        command = ["/bin/busybox", "sh", "/in/build_rootfs.sh", out.basename],
        env = {"PATH": "/bin", "SOURCE_DATE_EPOCH": str(ctx.attr.source_date_epoch)},
        cpus = 4,
        mem = 4096,
        mnemonic = "DebianRootfs",
        progress_message = "Building the build VM root image %{output}",
    )
    return [DefaultInfo(files = depset([out]))]

debian_rootfs = rule(
    implementation = _debian_rootfs_impl,
    doc = "Unpacks locked Debian packages in a bootstrap microVM into a SquashFS root image.",
    attrs = KRUN_ATTRS | {
        "debs": attr.label(mandatory = True, allow_files = [".deb"]),
        "bootstrap_busybox": attr.label(mandatory = True, allow_single_file = True),
        "script": attr.label(default = ":build_rootfs.sh", allow_single_file = True),
        "source_date_epoch": attr.int(default = 1704067200),
    },
)
