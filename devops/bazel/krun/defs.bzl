"""Rules that build and use the libkrun VM launcher."""

def _single(target, what):
    files = target.files.to_list()
    if len(files) != 1:
        fail("%s must provide exactly one file for %s" % (target.label, what))
    return files[0]

def _cargo_binary_impl(ctx):
    toolchain = ctx.toolchains["@rules_rust//rust:toolchain_type"]
    out = ctx.actions.declare_file(ctx.attr.name)
    manifest = ctx.file.manifest
    env_files = []
    exports = []
    for target, name in ctx.attr.env_files.items():
        file = _single(target, name)
        env_files.append(file)
        exports.append('export {}="$EXECROOT/{}"'.format(name, file.path))
    target_args = ""
    sysroot = '"$EXECROOT/%s"' % toolchain.sysroot
    built = "$CARGO_TARGET_DIR/release/" + ctx.attr.bin
    if ctx.attr.target:
        # Combine the host toolchain's sysroot with the pinned target library,
        # and link the static guest executable with the toolchain's rust-lld.
        triple = ctx.attr.target
        marker = "/lib/rustlib/%s/" % triple
        roots = [f.path.split(marker)[0] for f in ctx.files.target_std if marker in f.path]
        if not roots:
            fail("target_std has no %s library" % triple)
        exports += [
            'mkdir -p "$scratch/sysroot/lib/rustlib"',
            'for dir in "$EXECROOT/%s"/lib/rustlib/*; do ln -s "$dir" "$scratch/sysroot/lib/rustlib/"; done' % toolchain.sysroot,
            'ln -s "$EXECROOT/%s/lib/rustlib/%s" "$scratch/sysroot/lib/rustlib/%s"' % (roots[0], triple, triple),
            'export CARGO_TARGET_{}_LINKER="$EXECROOT/{}"'.format(triple.upper().replace("-", "_"), toolchain.linker.path),
        ]
        sysroot = '"$scratch/sysroot"'
        target_args = "--target %s" % triple
        built = "$CARGO_TARGET_DIR/%s/release/%s" % (triple, ctx.attr.bin)
    rustflags = "--sysroot=%s --remap-path-prefix=$scratch/src=. --remap-path-prefix=$EXECROOT=." % sysroot
    if ctx.attr.target:
        rustflags += " -C linker-flavor=ld.lld -C link-self-contained=yes"
    script = ctx.actions.declare_file(ctx.attr.name + ".cargo.sh")
    ctx.actions.write(script, """\
#!/bin/sh
set -eu
EXECROOT=$PWD
scratch=$(mktemp -d "${{TMPDIR:-/tmp}}/cargo.XXXXXX")
trap 'rm -rf "$scratch"' EXIT
export CARGO_HOME="$scratch/home" CARGO_TARGET_DIR="$scratch/target"
export RUSTC="$EXECROOT/{rustc}" RUSTDOC=/nonexistent CARGO_INCREMENTAL=0
{exports}
export RUSTFLAGS="{rustflags}"
# Cargo rewrites lock files next to the manifests. Build in a tree of links to
# the repository whose lock files, and the directories leading to them, are copies.
src="$EXECROOT/{root}" work="$scratch/src"
link_tree() {{
    mkdir "$2"
    for entry in "$1"/* "$1"/.[!.]*; do
        if [ -e "$entry" ] || [ -L "$entry" ]; then ln -s "$entry" "$2/"; fi
    done
}}
link_tree "$src" "$work"
for lock in {locks}; do
    current=
    for part in $(dirname "$lock" | tr / ' '); do
        [ "$part" = . ] && continue
        current="$current/$part"
        if [ -L "$work$current" ]; then rm "$work$current"; link_tree "$src$current" "$work$current"; fi
    done
    rm -f "$work/$lock"
    if [ -e "$src/$lock" ]; then cp "$src/$lock" "$work/$lock"; chmod u+w "$work/$lock"; fi
done
cd "$work/{manifest_dir}"
"$EXECROOT/{cargo}" build {locked} --offline --release --quiet {package} --bin {bin} {target} {features}
cp "{built}" "$EXECROOT/{out}"
""".format(
        rustc = toolchain.rustc.path,
        cargo = toolchain.cargo.path,
        exports = "\n".join(exports),
        rustflags = rustflags,
        root = manifest.owner.workspace_root or ".",
        locks = " ".join([(d + "/Cargo.lock" if d else "Cargo.lock") for d in [manifest.short_path.removeprefix("../").partition("/")[2].rpartition("/")[0] if manifest.owner.workspace_root else manifest.dirname]]),
        manifest_dir = manifest.short_path.removeprefix("../").partition("/")[2].rpartition("/")[0] if manifest.owner.workspace_root else manifest.dirname,
        locked = "--locked" if ctx.attr.locked else "",
        package = ("-p " + ctx.attr.package) if ctx.attr.package else "",
        bin = ctx.attr.bin,
        target = target_args,
        features = ("--features " + ",".join(ctx.attr.crate_features)) if ctx.attr.crate_features else "",
        built = built,
        out = out.path,
    ), is_executable = True)
    ctx.actions.run(
        executable = script,
        inputs = depset(
            [manifest, toolchain.linker] + ctx.files.srcs + ctx.files.target_std + env_files,
            transitive = [toolchain.all_files],
        ),
        outputs = [out],
        mnemonic = "CargoBuild",
        progress_message = "Building %s with Cargo" % ctx.attr.bin,
        env = {"PATH": "/usr/bin:/bin", "LC_ALL": "C"},
    )
    return [DefaultInfo(files = depset([out]), executable = out)]

cargo_binary = rule(
    implementation = _cargo_binary_impl,
    doc = "Builds a binary of an offline, vendored Cargo project with the rules_rust toolchain.",
    attrs = {
        "manifest": attr.label(mandatory = True, allow_single_file = ["Cargo.toml"], doc = "Manifest of the workspace or project to build."),
        "srcs": attr.label_list(allow_files = True, doc = "The project including vendored crates and .cargo/config.toml."),
        "package": attr.string(doc = "Workspace member to build."),
        "bin": attr.string(mandatory = True),
        "locked": attr.bool(default = True, doc = "Require Cargo.lock to be up to date."),
        "target": attr.string(doc = "Rust target triple; the binary is linked statically with rust-lld."),
        "target_std": attr.label(allow_files = True, doc = "Rust standard library of `target`."),
        "crate_features": attr.string_list(),
        "env_files": attr.label_keyed_string_dict(allow_files = True, doc = "Single-file labels exported to build scripts as absolute paths."),
    },
    toolchains = ["@rules_rust//rust:toolchain_type"],
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
    "_krun_vm": attr.label(default = Label("//devops/bazel/krun:krun-vm"), cfg = "exec", executable = True),
    "_krunfw": attr.label(default = Label("//devops/bazel/krun:krunfw"), cfg = "exec", allow_single_file = True),
}

def _quote(value):
    return "'" + value.replace("'", "'\\''") + "'"

def _guest(root_dir):
    """Returns the guest shell, mount command and PATH for the root kind."""
    if root_dir:
        return "/bin/busybox sh", "/bin/busybox mount", "/bin"
    return "/bin/sh", "/bin/mount", "/usr/sbin:/usr/bin:/sbin:/bin"

def _run_script(root_dir, env, command):
    # libkrun passes the command line through the kernel command line, which
    # cannot carry arbitrary quoting. The real command travels as this script,
    # which also reports the exit status that libkrun drops for a disk root.
    # The namespace backend (KRUN_BACKEND=namespace) mounts /tmp and /out itself.
    _, mount, path = _guest(root_dir)
    return "\n".join(
        ["set -u", "export PATH=" + path] +
        ["export %s=%s" % (name, _quote(value)) for name, value in sorted(env.items())] +
        [
            "if { [ \"${KRUN_BACKEND:-}\" = namespace ] || { %s -t tmpfs -o mode=1777,size=85%% tmpfs /tmp && %s -t virtiofs out /out; }; } && cd /tmp; then" % (mount, mount),
            "    " + " ".join([_quote(arg) for arg in command]),
            "    status=$?",
            "else",
            "    status=125",
            "fi",
            "echo $status > /in/.krun-status",
            "exit $status",
            "",
        ],
    )

def _entry(root_dir):
    shell, mount, _ = _guest(root_dir)
    # No quotes: this travels on the guest kernel's command line. /in is
    # already there when the namespace backend runs it.
    return shell.split(" ") + ["-c", "{ [ -e /in/.krun-run ] || %s -t virtiofs in /in; } && exec %s /in/.krun-run" % (mount, shell)]

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
    run = ctx.actions.declare_file(ctx.attr.name + ".krun-run")
    ctx.actions.write(run, _run_script(root_dir, env, command))
    manifest = ctx.actions.declare_file(ctx.attr.name + ".krun-inputs")
    entries = [(run, ".krun-run")] + inputs.items()
    ctx.actions.write(manifest, "".join(["%s\t%s\n" % (f.path, dest) for f, dest in entries]))
    args = ctx.actions.args()
    args.add("--krunfw", ctx.file._krunfw)
    if root_disk:
        args.add("--root-disk", root_disk)
    else:
        args.add("--root", root_dir)
    args.add_all(["--cpus", str(cpus), "--mem", str(mem), "--inputs", manifest, "--outputs", out_root, "--"])
    args.add_all(_entry(root_dir))
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

def _guest_path(short_path):
    # Runfiles of external repositories start with "../"; stage them under external/.
    return "/in/" + ("external/" + short_path[3:] if short_path.startswith("../") else short_path)

def _vm_test_impl(ctx):
    files = [ctx.file.src] + ctx.files.data
    paths = {f.short_path: _guest_path(f.short_path) for f in files}
    targets = [ctx.attr.src] + ctx.attr.data
    args = []
    for arg in ctx.attr.guest_args:
        expanded = ctx.expand_location(arg, targets)
        args.append(paths.get(expanded, expanded))
    interpreter = ["python3", "-I"] if ctx.file.src.extension == "py" else ["bash"]
    run = ctx.actions.declare_file(ctx.label.name + ".krun-run")
    ctx.actions.write(run, _run_script(None, ctx.attr.env | {"TEST_TMPDIR": "/tmp"}, interpreter + [paths[ctx.file.src.short_path]] + args))
    manifest = ctx.actions.declare_file(ctx.label.name + ".krun-inputs")
    entries = [(run, ".krun-run")] + [(f, paths[f.short_path][len("/in/"):]) for f in files]
    ctx.actions.write(manifest, "".join(["%s\t%s\n" % (f.short_path, dest) for f, dest in entries]))
    rootfs = ctx.file._rootfs
    launcher = ctx.actions.declare_file(ctx.label.name)
    ctx.actions.write(launcher, "\n".join([
        "#!/bin/sh",
        "# Runs the test in the build VM. Bazel starts tests in the main runfiles tree.",
        "exec " + " ".join([_quote(arg) for arg in [
            ctx.executable._krun_vm.short_path,
            "--krunfw",
            ctx.file._krunfw.short_path,
            "--root-disk",
            rootfs.short_path,
            "--cpus",
            str(ctx.attr.cpus),
            "--mem",
            str(ctx.attr.mem),
            "--inputs",
            manifest.short_path,
        ]]) + ' --outputs "$TEST_TMPDIR" -- ' + " ".join([_quote(arg) for arg in _entry(None)]),
        "",
    ]), is_executable = True)
    runfiles = ctx.runfiles(files = files + [run, manifest, rootfs, ctx.file._krunfw]).merge(
        ctx.attr._krun_vm[DefaultInfo].default_runfiles,
    )
    return [
        DefaultInfo(executable = launcher, runfiles = runfiles),
        testing.ExecutionInfo({"no-remote": "1", "no-sandbox": "1"}),
    ]

vm_test = rule(
    implementation = _vm_test_impl,
    doc = """Runs a Python or Bash test program in the build VM.

The program and `data` appear under /in. Each `guest_args` entry that expands to a
single `$(rootpath ...)` is replaced by that file's path in the guest.
""",
    attrs = KRUN_ATTRS | {
        "src": attr.label(mandatory = True, allow_single_file = [".py", ".sh"]),
        "guest_args": attr.string_list(doc = "Arguments; `$(rootpath ...)` entries become guest paths."),
        "data": attr.label_list(allow_files = True),
        "env": attr.string_dict(),
        "cpus": attr.int(default = 4),
        "mem": attr.int(default = 4096),
        "_rootfs": attr.label(default = Label("//devops/bazel/debian:rootfs"), allow_single_file = True, cfg = "exec"),
    },
    test = True,
)
