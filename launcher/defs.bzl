"""Builds the launcher for Gizlet OS in the build VM."""

load("//tools/bazel/krun:defs.bzl", "KRUN_ATTRS", "krun_action")

def _repo_relative(f):
    """The file's path inside its repository."""
    root = f.owner.workspace_root
    return f.path.removeprefix(f.root.path + "/").removeprefix(root + "/") if root else f.short_path

def _gizlet_launcher_impl(ctx):
    out = ctx.actions.declare_file(ctx.attr.name)
    inputs = {ctx.file._script: "build_card.sh"}
    for f in ctx.files.srcs:
        inputs[f] = "launcher/" + f.short_path.removeprefix(ctx.label.package + "/")
    for f in ctx.files._crates:
        inputs[f] = _repo_relative(f)
    for f in ctx.files._rust:
        inputs[f] = "rust/" + _repo_relative(f)
    krun_action(
        ctx,
        root_disk = ctx.file._rootfs,
        inputs = inputs,
        outputs = [out],
        command = ["/bin/sh", "/in/build_card.sh", out.path.removeprefix(ctx.bin_dir.path + "/" + ctx.label.package + "/")],
        env = {"LC_ALL": "C", "SOURCE_DATE_EPOCH": str(ctx.attr.source_date_epoch), "TZ": "UTC", "HOME": "/tmp"},
        cpus = 4,
        mem = 6144,
        mnemonic = "LauncherCargo",
        progress_message = "Building the launcher %{output} in the build VM",
    )
    return [DefaultInfo(files = depset([out]), executable = out)]

gizlet_launcher = rule(
    implementation = _gizlet_launcher_impl,
    doc = "Builds the launcher's Rust crate with a static DRM raylib for AArch64 Gizlet OS (glibc, libdrm).",
    attrs = KRUN_ATTRS | {
        "srcs": attr.label_list(mandatory = True, allow_files = True),
        "source_date_epoch": attr.int(default = 1704067200),
        "_crates": attr.label(default = Label("@launcher_crates//:vendor")),
        "_rootfs": attr.label(default = Label("//tools/bazel/debian:rootfs"), allow_single_file = True, cfg = "exec"),
        "_rust": attr.label(default = Label("@rust_aarch64_linux//:toolchain")),
        "_script": attr.label(default = Label(":build_card.sh"), allow_single_file = True),
    },
)
