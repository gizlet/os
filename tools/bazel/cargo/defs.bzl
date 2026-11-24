"""Builds a Rust program for AArch64 Gizlet OS with Cargo in the build VM."""

load("//tools/bazel/krun:defs.bzl", "KRUN_ATTRS", "krun_action")

def _repo_relative(f):
    """The file's path inside its repository."""
    root = f.owner.workspace_root
    return f.path.removeprefix(f.root.path + "/").removeprefix(root + "/") if root else f.short_path

def _cargo_binary_impl(ctx):
    out = ctx.actions.declare_file(ctx.attr.name)
    inputs = {ctx.file._script: "build.sh"}
    for f in ctx.files.srcs:
        inputs[f] = "crate/" + f.short_path.removeprefix(ctx.label.package + "/")
    for f in ctx.files.crates:
        inputs[f] = _repo_relative(f)
    for f in ctx.files._rust:
        inputs[f] = "rust/" + _repo_relative(f)
    if ctx.file.raylib:
        inputs[ctx.file.raylib] = "raylib/libraylib.so"
    krun_action(
        ctx,
        root_disk = ctx.file._rootfs,
        inputs = inputs,
        outputs = [out],
        command = [
            "/bin/sh",
            "/in/build.sh",
            ctx.attr.binary or ctx.attr.name,
            out.path.removeprefix(ctx.bin_dir.path + "/" + ctx.label.package + "/"),
            ",".join(ctx.attr.features),
        ],
        env = {"LC_ALL": "C", "SOURCE_DATE_EPOCH": str(ctx.attr.source_date_epoch), "TZ": "UTC", "HOME": "/tmp"},
        cpus = 4,
        mem = 6144,
        mnemonic = "Cargo",
        progress_message = "Building %{output} with Cargo in the build VM",
    )
    return [DefaultInfo(files = depset([out]), executable = out)]

cargo_binary = rule(
    implementation = _cargo_binary_impl,
    doc = """Builds one binary of a Cargo crate for AArch64 Gizlet OS.

Cargo reads crates only from `crates`, vendored from the crate's Cargo.lock
(see repositories.bzl). With `raylib`, the program links Gizlet runtime 1's
raylib, found through GIZLET_RAYLIB_LIB, and runs against the runtime.""",
    attrs = KRUN_ATTRS | {
        "srcs": attr.label_list(mandatory = True, allow_files = True, doc = "The crate: Cargo.toml, Cargo.lock and its sources."),
        "crates": attr.label(mandatory = True, doc = "The vendored crates of its Cargo.lock."),
        "binary": attr.string(doc = "The [[bin]] to build; the target's name by default."),
        "features": attr.string_list(),
        "raylib": attr.label(allow_single_file = True, doc = "Gizlet runtime 1's libraylib.so, for programs that draw."),
        "source_date_epoch": attr.int(default = 1704067200),
        "_rootfs": attr.label(default = Label("//tools/bazel/debian:rootfs"), allow_single_file = True, cfg = "exec"),
        "_rust": attr.label(default = Label("@rust_aarch64_linux//:toolchain")),
        "_script": attr.label(default = Label(":build.sh"), allow_single_file = True),
    },
)
