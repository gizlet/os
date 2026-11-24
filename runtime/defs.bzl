"""Builds a Gizlet runtime image in the build VM."""

load("//tools/bazel/krun:defs.bzl", "KRUN_ATTRS", "krun_action")

def _gizlet_runtime_impl(ctx):
    out = ctx.outputs.out
    inputs = {
        ctx.file._script: "build_runtime.sh",
        ctx.file._run: "gizlet-run",
        ctx.file._gl_info: "gl-info.c",
    }
    for deb in ctx.files.debs:
        inputs[deb] = "debs/" + deb.basename
    krun_action(
        ctx,
        root_disk = ctx.file._rootfs,
        inputs = inputs,
        outputs = [out],
        command = ["/bin/sh", "/in/build_runtime.sh", out.path.removeprefix(ctx.bin_dir.path + "/" + ctx.label.package + "/")],
        env = {"LC_ALL": "C", "SOURCE_DATE_EPOCH": str(ctx.attr.source_date_epoch), "TZ": "UTC"},
        cpus = 4,
        mem = 4096,
        mnemonic = "GizletRuntime",
        progress_message = "Building the Gizlet runtime %{output} in the build VM",
    )
    return [DefaultInfo(files = depset([out]))]

gizlet_runtime = rule(
    implementation = _gizlet_runtime_impl,
    doc = "Lays out locked Debian libraries as a Gizlet runtime and writes it as a read-only SquashFS image.",
    attrs = KRUN_ATTRS | {
        "debs": attr.label(mandatory = True, allow_files = [".deb"], doc = "The runtime's libraries."),
        "out": attr.output(mandatory = True),
        "source_date_epoch": attr.int(default = 1704067200),
        "_gl_info": attr.label(default = Label(":gl-info.c"), allow_single_file = True),
        "_rootfs": attr.label(default = Label("//tools/bazel/debian:rootfs"), allow_single_file = True, cfg = "exec"),
        "_run": attr.label(default = Label(":gizlet-run"), allow_single_file = True),
        "_script": attr.label(default = Label(":build_runtime.sh"), allow_single_file = True),
    },
)
