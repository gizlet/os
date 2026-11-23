"""Rules that build the libkrun VM launcher."""

def _cargo_staticlib_impl(ctx):
    toolchain = ctx.toolchains["@rules_rust//rust:toolchain_type"]
    out = ctx.actions.declare_file(ctx.attr.name + ".a")
    manifest = ctx.file.manifest
    env_files = []
    exports = []
    for target, name in ctx.attr.env_files.items():
        files = target.files.to_list()
        if len(files) != 1:
            fail("%s must provide exactly one file for %s" % (target.label, name))
        env_files.append(files[0])
        exports.append('export {}="$EXECROOT/{}"'.format(name, files[0].path))
    script = ctx.actions.declare_file(ctx.attr.name + ".cargo.sh")
    ctx.actions.write(script, """\
#!/bin/sh
set -eu
EXECROOT=$PWD
scratch=$(mktemp -d "${{TMPDIR:-/tmp}}/cargo.XXXXXX")
trap 'rm -rf "$scratch"' EXIT
export CARGO_HOME="$scratch/home" CARGO_TARGET_DIR="$scratch/target"
export RUSTC="$EXECROOT/{rustc}" RUSTDOC=/nonexistent CARGO_INCREMENTAL=0
export RUSTFLAGS="--sysroot=$EXECROOT/{sysroot} --remap-path-prefix=$EXECROOT=."
{exports}
cd "$(dirname "{manifest}")"
"$EXECROOT/{cargo}" rustc --locked --offline --release --quiet -p {package} --lib \\
    --crate-type staticlib {features}
cp "$CARGO_TARGET_DIR/release/{lib}" "$EXECROOT/{out}"
""".format(
        rustc = toolchain.rustc.path,
        cargo = toolchain.cargo.path,
        sysroot = toolchain.sysroot,
        exports = "\n".join(exports),
        manifest = manifest.path,
        package = ctx.attr.package,
        features = ("--features " + ",".join(ctx.attr.crate_features)) if ctx.attr.crate_features else "",
        lib = ctx.attr.lib,
        out = out.path,
    ), is_executable = True)
    ctx.actions.run(
        executable = script,
        inputs = depset([manifest] + ctx.files.srcs + env_files, transitive = [toolchain.all_files]),
        outputs = [out],
        mnemonic = "CargoStaticlib",
        progress_message = "Building %s with Cargo" % ctx.attr.package,
        env = {"PATH": "/usr/bin:/bin", "LC_ALL": "C"},
    )
    return [DefaultInfo(files = depset([out]))]

cargo_staticlib = rule(
    implementation = _cargo_staticlib_impl,
    doc = "Builds one package of an offline, vendored Cargo workspace as a static library.",
    attrs = {
        "manifest": attr.label(mandatory = True, allow_single_file = ["Cargo.toml"], doc = "Workspace root manifest."),
        "srcs": attr.label_list(allow_files = True, doc = "The workspace including vendored crates and .cargo/config.toml."),
        "package": attr.string(mandatory = True),
        "lib": attr.string(mandatory = True, doc = "File name of the produced archive under target/release."),
        "crate_features": attr.string_list(),
        "env_files": attr.label_keyed_string_dict(allow_files = True, doc = "Single-file labels exported to build scripts as absolute paths."),
    },
    toolchains = ["@rules_rust//rust:toolchain_type"],
)

def _linux_aarch64_transition_impl(_settings, _attr):
    return {
        "//command_line_option:platforms": [str(Label("//tools/bazel/platforms:linux_aarch64"))],
        "//command_line_option:extra_toolchains": ["@zig_sdk//toolchain:linux_arm64_musl"],
    }

_linux_aarch64_transition = transition(
    implementation = _linux_aarch64_transition_impl,
    inputs = [],
    outputs = ["//command_line_option:platforms", "//command_line_option:extra_toolchains"],
)

def _linux_aarch64_binary_impl(ctx):
    src = ctx.attr.binary[0][DefaultInfo].files_to_run.executable
    out = ctx.actions.declare_file(ctx.attr.name)
    ctx.actions.symlink(output = out, target_file = src, is_executable = True)
    return [DefaultInfo(files = depset([out]), executable = out)]

linux_aarch64_binary = rule(
    implementation = _linux_aarch64_binary_impl,
    doc = "Builds a cc_binary as a static AArch64 Linux musl executable, independent of the target platform.",
    attrs = {"binary": attr.label(mandatory = True, cfg = _linux_aarch64_transition, executable = True)},
    executable = True,
)

def _hypervisor_binary_impl(ctx):
    src = ctx.attr.binary[DefaultInfo].files_to_run.executable
    out = ctx.actions.declare_file(ctx.attr.name)
    if ctx.attr.codesign:
        # Hypervisor.framework requires its entitlement. An ad-hoc signature is enough.
        ctx.actions.run_shell(
            inputs = [src, ctx.file.entitlements],
            outputs = [out],
            command = 'cp "$1" "$3" && chmod u+w "$3" && /usr/bin/codesign --sign - --force --entitlements "$2" "$3"',
            arguments = [src.path, ctx.file.entitlements.path, out.path],
            mnemonic = "Codesign",
            execution_requirements = {"no-sandbox": "1"},
        )
    else:
        ctx.actions.symlink(output = out, target_file = src, is_executable = True)
    runfiles = ctx.runfiles(files = [out]).merge(ctx.attr.binary[DefaultInfo].default_runfiles)
    return [DefaultInfo(files = depset([out]), executable = out, runfiles = runfiles)]

hypervisor_binary = rule(
    implementation = _hypervisor_binary_impl,
    doc = "Signs a host executable with the hypervisor entitlement on macOS.",
    attrs = {
        "binary": attr.label(mandatory = True, executable = True, cfg = "target"),
        "codesign": attr.bool(mandatory = True),
        "entitlements": attr.label(allow_single_file = True),
    },
    executable = True,
)

KRUN_ATTRS = {
    "_krun_vm": attr.label(default = Label("//tools/bazel/krun:krun-vm"), cfg = "exec", executable = True),
    "_krunfw": attr.label(default = Label("//tools/bazel/krun:libkrunfw.5.dylib"), cfg = "exec", allow_single_file = True),
}

def _quote(value):
    return "'" + value.replace("'", "'\\''") + "'"

def krun_action(ctx, *, command, inputs, outputs, mnemonic, root_dir = None, root_disk = None, env = {}, cpus = 4, mem = 6144, progress_message = None):
    """Runs `command` in a microVM.

    `inputs` maps Files to their paths under /in in the guest. Outputs must be
    in this package; the guest writes them under /out at their package paths.
    Exactly one of `root_dir` (a directory path) and `root_disk` (a SquashFS File) is the guest root.
    The command starts in /tmp, a private tmpfs, with only `env` and PATH set.
    """
    out_root = ctx.bin_dir.path + "/" + ctx.label.package
    for output in outputs:
        if not output.path.startswith(out_root + "/"):
            fail("%s is outside %s" % (output.path, out_root))

    # libkrun passes the command line through the kernel command line, which
    # cannot carry arbitrary quoting. The real command travels as a script.
    mount = "/bin/busybox mount" if root_dir else "/bin/mount"
    path = "/bin" if root_dir else "/usr/sbin:/usr/bin:/sbin:/bin"
    run = ctx.actions.declare_file(ctx.attr.name + ".krun-run")
    ctx.actions.write(run, "\n".join(
        ["set -u", "export PATH=" + path] +
        ["export %s=%s" % (name, _quote(value)) for name, value in sorted(env.items())] +
        [
            "if %s -t tmpfs -o mode=1777 tmpfs /tmp && %s -t virtiofs out /out && cd /tmp; then" % (mount, mount),
            "    " + " ".join([_quote(arg) for arg in command]),
            "    status=$?",
            "else",
            "    status=125",
            "fi",
            "echo $status > /in/.krun-status",
            "exit $status",
            "",
        ],
    ))
    manifest = ctx.actions.declare_file(ctx.attr.name + ".krun-inputs")
    entries = [(run, ".krun-run")] + inputs.items()
    ctx.actions.write(manifest, "".join(["%s\t%s\n" % (f.path, dest) for f, dest in entries]))
    args = ctx.actions.args()
    args.add("--krunfw", ctx.file._krunfw)
    if root_disk:
        args.add("--root-disk", root_disk)
    else:
        args.add("--root", root_dir)
    args.add_all(["--cpus", str(cpus), "--mem", str(mem), "--inputs", manifest, "--outputs", out_root])
    shell = "/bin/busybox sh" if root_dir else "/bin/sh"
    args.add_all(["--", shell.split(" ")[0]] + shell.split(" ")[1:] + ["-c", "%s -t virtiofs in /in && exec %s /in/.krun-run" % (mount, shell)])
    ctx.actions.run(
        executable = ctx.executable._krun_vm,
        arguments = [args],
        inputs = [manifest, run, ctx.file._krunfw] + ([root_disk] if root_disk else []) + inputs.keys(),
        outputs = outputs,
        mnemonic = mnemonic,
        progress_message = progress_message,
        # Hypervisor access and the staging links need the real output tree.
        execution_requirements = {"no-remote": "1", "no-sandbox": "1"},
    )
