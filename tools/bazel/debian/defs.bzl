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
        "_rootfs": attr.label(default = Label("//tools/bazel/debian:rootfs"), allow_single_file = True, cfg = "exec"),
    },
)

def _debian_build_impl(ctx):
    inputs = {}
    for target, name in ctx.attr.srcs.items():
        files = target.files.to_list()
        if len(files) != 1:
            fail("%s must provide exactly one file for %s" % (target.label, name))
        inputs[files[0]] = "src/" + name
    out_root = ctx.bin_dir.path + "/" + ctx.label.package + "/"
    copies = ['cp "%s" "/out/%s"' % (path, out.path.removeprefix(out_root)) for path, out in zip(ctx.attr.built_paths, ctx.outputs.outs)]
    krun_action(
        ctx,
        root_disk = ctx.file._rootfs,
        inputs = inputs,
        outputs = ctx.outputs.outs,
        command = ["/bin/sh", "-c", "\n".join(["set -eu", "mkdir /tmp/build && cd /tmp/build", "(", ctx.attr.cmd, ")"] + copies)],
        env = {"LC_ALL": "C", "SOURCE_DATE_EPOCH": str(ctx.attr.source_date_epoch), "TZ": "UTC"},
        cpus = 4,
        mem = 2048,
        mnemonic = "DebianBuild",
        progress_message = "Building %{label} in the build VM",
    )
    return [DefaultInfo(files = depset(ctx.outputs.outs))]

_debian_build = rule(
    implementation = _debian_build_impl,
    attrs = KRUN_ATTRS | {
        "built_paths": attr.string_list(mandatory = True),
        "cmd": attr.string(mandatory = True),
        "outs": attr.output_list(mandatory = True),
        "source_date_epoch": attr.int(default = 1704067200),
        "srcs": attr.label_keyed_string_dict(allow_files = True),
        "_rootfs": attr.label(default = Label("//tools/bazel/debian:rootfs"), allow_single_file = True, cfg = "exec"),
    },
)

def debian_build(name, srcs, cmd, outs, **kwargs):
    """Runs the shell commands `cmd` (set -eu) in an empty /tmp/build in the build VM.

    Each `srcs` label is available as /in/src/<name>. `outs` maps paths relative
    to /tmp/build after `cmd` to output file names, which become targets.
    """
    _debian_build(
        name = name,
        srcs = srcs,
        cmd = cmd,
        built_paths = list(outs.keys()),
        outs = list(outs.values()),
        **kwargs
    )
