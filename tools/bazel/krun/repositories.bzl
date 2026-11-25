"""Pinned libkrun, libkrunfw and Rust target inputs for the build VM.

libkrun 2.0 comes from a fixed commit of its main branch. Every crate in its
lock files is vendored by checksum, and git dependencies come from pinned
archives, so Cargo runs offline. The workspace gains one member, krun-vm, the
launcher in this directory. libkrunfw ships its guest kernel as a prebuilt C
bundle that any host C compiler turns into the loadable library.
"""

_LIBKRUN_COMMIT = "715c1c9857576c7a1e2057abcaf4a782ba527f97"
_LIBKRUN_SHA256 = "d6166e34dd4eaf0048d9eb72f136e22cf2da15d424075bc88ffeaf80c14bfac3"

# Git dependencies named in libkrun's Cargo.lock: repository URL -> (commit, archive SHA-256).
_GIT_ARCHIVES = {
    "https://github.com/mtjhrc/ffier.git": ("51fc8b1abb282e06a81ac9658aff6ca23507004a", "c126a87446b0e10824897929805482a76449bdc020cc2dfd2095ff9ecf9aa8b4"),
}

_LIBKRUNFW_VERSION = "5.6.4"
_LIBKRUNFW_AARCH64_SHA256 = "db126ed9775e0f92fc910a90a8b7ff52bdf55644b5edc75977115f81603e48c9"

# The guest init is a static Rust program. Its target library matches the host Rust toolchain.
_RUST_STD_MUSL = (
    "https://static.rust-lang.org/dist/2026-11-12/rust-std-1.100.0-aarch64-unknown-linux-musl.tar.xz",
    "30eceb11cecdb0f4374c6c6e9b193a195949e7ae2687840ffdd3daa637c06db2",
    "rust-std-1.100.0-aarch64-unknown-linux-musl/rust-std-aarch64-unknown-linux-musl",
)

_CRATES = "https://static.crates.io/crates/{name}/{name}-{version}.crate"
_REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"

def _lock_packages(text):
    """Returns the [[package]] tables of a Cargo.lock as dicts of string fields."""
    packages = []
    current = None
    for line in text.splitlines():
        line = line.strip()
        if line == "[[package]]":
            current = {}
            packages.append(current)
        elif line.startswith("["):
            current = None
        elif current != None and " = \"" in line:
            key, value = line.split(" = ", 1)
            current[key] = value.strip("\"")
    return packages

def _git_url(source):
    # git+https://host/repo.git?tag=v1#commit
    url, _, commit = source.removeprefix("git+").partition("#")
    return url.partition("?")[0], commit

def _libkrun_impl(ctx):
    ctx.download_and_extract(
        url = "https://github.com/libkrun/libkrun/archive/%s.tar.gz" % _LIBKRUN_COMMIT,
        sha256 = _LIBKRUN_SHA256,
        strip_prefix = "libkrun-" + _LIBKRUN_COMMIT,
    )
    crates = {}
    git = {}
    for lock in ("Cargo.lock", "init/init-binary/Cargo.lock"):
        for package in _lock_packages(ctx.read(lock)):
            source = package.get("source")
            if source == None:
                continue
            if source.startswith("git+"):
                url, commit = _git_url(source)
                if url not in _GIT_ARCHIVES or _GIT_ARCHIVES[url][0] != commit:
                    fail("Unpinned git dependency %s at %s" % (url, commit))
                git.setdefault(url, {})[package["name"]] = True
                continue
            if source != _REGISTRY or "checksum" not in package:
                fail("Unsupported Cargo.lock source for %s: %s" % (package["name"], source))
            crates["%s-%s" % (package["name"], package["version"])] = (package["name"], package["version"], package["checksum"])
    pending = []
    for directory, (name, version, checksum) in sorted(crates.items()):
        archive = "crates/%s.crate" % directory
        pending.append((directory, checksum, archive, ctx.download(
            url = _CRATES.format(name = name, version = version),
            output = archive,
            sha256 = checksum,
            block = False,
        )))
    for directory, checksum, archive, download in pending:
        download.wait()
        ctx.extract(archive, output = "vendor/" + directory, strip_prefix = directory, type = "tar.gz")
        ctx.file("vendor/%s/.cargo-checksum.json" % directory, json.encode({"files": {}, "package": checksum}))
        ctx.delete(archive)

    # Git dependencies are whole repositories, patched in by path so their
    # crates keep the relative path dependencies between them.
    patches = []
    for url, names in sorted(git.items()):
        commit, sha256 = _GIT_ARCHIVES[url]
        owner_repo = url.removeprefix("https://github.com/").removesuffix(".git")
        directory = "git/" + owner_repo.replace("/", "-")
        ctx.download_and_extract(
            url = "https://codeload.github.com/%s/tar.gz/%s" % (owner_repo, commit),
            output = directory,
            sha256 = sha256,
            type = "tar.gz",
            strip_prefix = "%s-%s" % (owner_repo.split("/")[1], commit),
        )
        patches.append('[patch."%s"]' % url)
        for name in sorted(names):
            if ctx.path("%s/%s/Cargo.toml" % (directory, name)).exists:
                patches.append('%s = { path = "%s/%s" }' % (name, directory, name))
    ctx.file(".cargo/config.toml", """\
[source.crates-io]
replace-with = "vendored"

[source.vendored]
directory = "vendor"

[net]
offline = true

""" + "\n".join(patches) + "\n")

    # Add the launcher to the workspace.
    ctx.file("krun-vm/Cargo.toml", ctx.read(ctx.attr.launcher_manifest))
    ctx.file("krun-vm/src/main.rs", ctx.read(ctx.attr.launcher_main))
    manifest = ctx.read("Cargo.toml")
    if "members = [\n" not in manifest:
        fail("Unexpected libkrun workspace manifest")
    ctx.file("Cargo.toml", manifest.replace("members = [\n", 'members = [\n    "krun-vm",\n', 1))

    ctx.file("BUILD.bazel", """\
package(default_visibility = ["//visibility:public"])

filegroup(
    name = "workspace",
    srcs = glob(["**"], exclude = ["BUILD.bazel", "WORKSPACE*", "MODULE.bazel", "REPO.bazel"]),
)

exports_files(["Cargo.toml", "LICENSE", "init/init-binary/Cargo.toml"])
""")

_libkrun = repository_rule(
    implementation = _libkrun_impl,
    attrs = {
        "launcher_manifest": attr.label(mandatory = True, allow_single_file = True),
        "launcher_main": attr.label(mandatory = True, allow_single_file = True),
    },
)

def _libkrunfw_impl(ctx):
    ctx.download_and_extract(
        url = "https://github.com/containers/libkrunfw/releases/download/v%s/libkrunfw-prebuilt-aarch64.tgz" % _LIBKRUNFW_VERSION,
        sha256 = _LIBKRUNFW_AARCH64_SHA256,
        strip_prefix = "libkrunfw",
    )
    ctx.file("BUILD.bazel", """\
package(default_visibility = ["//visibility:public"])

exports_files(["kernel.c", "LICENSE-GPL-2.0-only", "LICENSE-LGPL-2.1-only"])
""")

_libkrunfw = repository_rule(implementation = _libkrunfw_impl)

def _rust_std_impl(ctx):
    url, sha256, prefix = _RUST_STD_MUSL
    ctx.download_and_extract(url = url, sha256 = sha256, strip_prefix = prefix)
    ctx.file("BUILD.bazel", """\
package(default_visibility = ["//visibility:public"])

filegroup(
    name = "rust_std",
    srcs = glob(["lib/rustlib/aarch64-unknown-linux-musl/**"]),
)
""")

_rust_std = repository_rule(implementation = _rust_std_impl)

def _krun_repositories_impl(module_ctx):
    _libkrun(
        name = "libkrun",
        launcher_manifest = Label("//tools/bazel/krun/vm:Cargo.toml"),
        launcher_main = Label("//tools/bazel/krun/vm:main.rs"),
    )
    _libkrunfw(name = "libkrunfw")
    _rust_std(name = "rust_std_aarch64_unknown_linux_musl")
    return module_ctx.extension_metadata(reproducible = True)

krun_repositories = module_extension(
    implementation = _krun_repositories_impl,
    doc = "Provides libkrun 2.0 with vendored crates, the libkrunfw aarch64 kernel and the musl Rust target library.",
)
