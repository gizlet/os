"""Builds a Gizlet runtime image, and the libraries it adds to Debian's, in the build VM."""

load("//devops/bazel/krun:defs.bzl", "KRUN_ATTRS", "krun_action")

def _package_path(ctx, out):
    return out.path.removeprefix(ctx.bin_dir.path + "/" + ctx.label.package + "/")

def _repo_relative(f):
    """The file's path inside its repository."""
    root = f.owner.workspace_root
    return f.path.removeprefix(f.root.path + "/").removeprefix(root + "/") if root else f.short_path

def _raylib_library_impl(ctx):
    out = ctx.outputs.out
    inputs = {
        ctx.file._script: "build_raylib.sh",
        ctx.file._es3_patch: "raylib-es3.patch",
    }
    for f in ctx.files._crates:
        inputs[f] = _repo_relative(f)
    krun_action(
        ctx,
        root_disk = ctx.file._rootfs,
        inputs = inputs,
        outputs = [out],
        command = ["/bin/sh", "/in/build_raylib.sh", _package_path(ctx, out)],
        env = {"LC_ALL": "C", "SOURCE_DATE_EPOCH": str(ctx.attr.source_date_epoch), "TZ": "UTC"},
        cpus = 4,
        mem = 4096,
        mnemonic = "RaylibDrm",
        progress_message = "Building %{output} in the build VM",
    )
    return [DefaultInfo(files = depset([out]))]

raylib_library = rule(
    implementation = _raylib_library_impl,
    doc = "Builds raylib from the launcher's vendored raylib-sys sources as a shared library for DRM with OpenGL ES 3.0.",
    attrs = KRUN_ATTRS | {
        "out": attr.output(mandatory = True),
        "source_date_epoch": attr.int(default = 1704067200),
        "_crates": attr.label(default = Label("@launcher_crates//:vendor")),
        "_es3_patch": attr.label(default = Label(":raylib-es3.patch"), allow_single_file = True),
        "_rootfs": attr.label(default = Label("//devops/bazel/debian:rootfs"), allow_single_file = True, cfg = "exec"),
        "_script": attr.label(default = Label(":build_raylib.sh"), allow_single_file = True),
    },
)

def _gizlet_runtime_impl(ctx):
    out = ctx.outputs.out
    inputs = {
        ctx.file._script: "build_runtime.sh",
        ctx.file._run: "gizlet-run",
        ctx.file._gl_info: "gl-info.c",
    }
    for deb in ctx.files.debs:
        inputs[deb] = "debs/" + deb.basename
    for library in ctx.files.libraries:
        inputs[library] = "libraries/" + library.basename
    krun_action(
        ctx,
        root_disk = ctx.file._rootfs,
        inputs = inputs,
        outputs = [out],
        command = ["/bin/sh", "/in/build_runtime.sh", _package_path(ctx, out)],
        env = {"LC_ALL": "C", "SOURCE_DATE_EPOCH": str(ctx.attr.source_date_epoch), "TZ": "UTC"},
        cpus = 4,
        mem = 4096,
        mnemonic = "GizletRuntime",
        progress_message = "Building the Gizlet runtime %{output} in the build VM",
    )
    return [DefaultInfo(files = depset([out]))]

gizlet_runtime = rule(
    implementation = _gizlet_runtime_impl,
    doc = "Lays out locked Debian libraries and built shared libraries as a Gizlet runtime and writes it as a read-only SquashFS image.",
    attrs = KRUN_ATTRS | {
        "debs": attr.label(mandatory = True, allow_files = [".deb"], doc = "The runtime's Debian libraries."),
        "libraries": attr.label_list(allow_files = True, doc = "Shared libraries built here, installed in lib/ under their SONAMEs."),
        "out": attr.output(mandatory = True),
        "source_date_epoch": attr.int(default = 1704067200),
        "_gl_info": attr.label(default = Label(":gl-info.c"), allow_single_file = True),
        "_rootfs": attr.label(default = Label("//devops/bazel/debian:rootfs"), allow_single_file = True, cfg = "exec"),
        "_run": attr.label(default = Label(":gizlet-run"), allow_single_file = True),
        "_script": attr.label(default = Label(":build_runtime.sh"), allow_single_file = True),
    },
)
