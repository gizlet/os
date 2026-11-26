"""Rules that use Debian in the build VM: its own root image, static C programs and the Gizlet OS base."""

load("//devops/bazel/krun:defs.bzl", "KRUN_ATTRS", "krun_action")

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

def _debian_cc_binary_impl(ctx):
    out = ctx.actions.declare_file(ctx.attr.name)
    sources = [f for f in ctx.files.srcs if f.extension == "c"]
    if not sources:
        fail("srcs must contain at least one C file")
    krun_action(
        ctx,
        root_disk = ctx.file._rootfs,
        inputs = {f: f.path for f in ctx.files.srcs},
        outputs = [out],
        command = ["gcc"] + ctx.attr.copts + [
            # Sources arrive under /in; record their workspace paths only.
            "-ffile-prefix-map=/in/=",
            "-static",
            "-s",
            "-o",
            "/out/" + out.path.removeprefix(ctx.bin_dir.path + "/" + ctx.label.package + "/"),
        ] + ["/in/" + f.path for f in sources] + ctx.attr.linkopts,
        env = {"LC_ALL": "C", "SOURCE_DATE_EPOCH": str(ctx.attr.source_date_epoch), "TZ": "UTC"},
        cpus = 2,
        mem = 1024,
        mnemonic = "DebianCc",
        progress_message = "Compiling %{output} in the build VM",
    )
    return [DefaultInfo(files = depset([out]), executable = out)]

debian_cc_binary = rule(
    implementation = _debian_cc_binary_impl,
    doc = "Compiles a static, stripped AArch64 Linux glibc executable with the build VM's Debian GCC.",
    attrs = KRUN_ATTRS | {
        "srcs": attr.label_list(mandatory = True, allow_files = [".c", ".h"]),
        "copts": attr.string_list(),
        "linkopts": attr.string_list(),
        "source_date_epoch": attr.int(default = 1704067200),
        "_rootfs": attr.label(default = Label("//devops/bazel/debian:rootfs"), allow_single_file = True, cfg = "exec"),
    },
)

def _single(target, what):
    files = target.files.to_list()
    if len(files) != 1:
        fail("%s must provide exactly one file for %s" % (target.label, what))
    return files[0]

def _debian_base_impl(ctx):
    inputs = {ctx.file._script: "build_base.sh"}
    for deb in ctx.files.debs:
        inputs[deb] = "debs/" + deb.basename
    for attr, prefix in ((ctx.attr.files, "files/"), (ctx.attr.executables, "executables/")):
        for target, path in attr.items():
            if path.startswith("/") or ".." in path.split("/"):
                fail("Invalid root path: " + path)
            inputs[_single(target, path)] = prefix + path
    for path, target in ctx.attr.symlinks.items():
        if path.startswith("/") or ".." in path.split("/") or " " in path + target or ":" in path:
            fail("Invalid symbolic link: %s -> %s" % (path, target))
    out = ctx.outputs.out
    krun_action(
        ctx,
        root_disk = ctx.file._rootfs,
        inputs = inputs,
        outputs = [out],
        command = [
            "/bin/sh",
            "/in/build_base.sh",
            out.path.removeprefix(ctx.bin_dir.path + "/" + ctx.label.package + "/"),
            ctx.attr.suite,
            " ".join(ctx.attr.directories),
            " ".join(ctx.attr.enable),
            " ".join(ctx.attr.disable),
            " ".join(ctx.attr.mask),
            " ".join(["%s:%s" % (path, target) for path, target in sorted(ctx.attr.symlinks.items())]),
        ],
        env = {"LC_ALL": "C", "SOURCE_DATE_EPOCH": str(ctx.attr.source_date_epoch), "TZ": "UTC"},
        cpus = 4,
        mem = 4096,
        mnemonic = "DebianBase",
        progress_message = "Building the Debian base %{output} in the build VM",
    )
    return [DefaultInfo(files = depset([out]))]

debian_base = rule(
    implementation = _debian_base_impl,
    doc = """Installs locked Debian packages with mmdebstrap and writes a read-only SquashFS.

Maintainer scripts run in the build VM, which has the target's architecture.
`directories` are created, `files` (0644) and `executables` (0755) installed at their root paths,
`symlinks` replace their root paths with symbolic links to the given targets,
the `enable` units are enabled, the `disable` units disabled and the `mask` units masked. Host keys and the
machine ID are removed, so every boot of a card creates its own.
""",
    attrs = KRUN_ATTRS | {
        "debs": attr.label(mandatory = True, allow_files = [".deb"]),
        "directories": attr.string_list(doc = "Root-relative directories to create, such as mount points."),
        "disable": attr.string_list(doc = "Units a package enabled that must not start at boot."),
        "enable": attr.string_list(),
        "executables": attr.label_keyed_string_dict(allow_files = True),
        "files": attr.label_keyed_string_dict(allow_files = True),
        "mask": attr.string_list(),
        "out": attr.output(mandatory = True),
        "source_date_epoch": attr.int(default = 1704067200),
        "suite": attr.string(mandatory = True),
        "symlinks": attr.string_dict(doc = "Root-relative paths to replace with symbolic links, mapped to their targets."),
        "_rootfs": attr.label(default = Label("//devops/bazel/debian:rootfs"), allow_single_file = True, cfg = "exec"),
        "_script": attr.label(default = Label(":build_base.sh"), allow_single_file = True),
    },
)
