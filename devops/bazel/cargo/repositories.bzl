"""Cargo inputs for programs built in the build VM: crates vendored from a
Cargo.lock by checksum, and the Rust toolchain that runs there.

The build VM is AArch64 Linux, so its Rust is the official aarch64-unknown-linux-gnu
release, pinned like the host toolchain in MODULE.bazel.
"""

_RUST_DIST = "https://static.rust-lang.org/dist/2026-11-12/"
_RUST_VERSION = "1.100.0"
_RUST_TRIPLE = "aarch64-unknown-linux-gnu"

# Component -> (archive SHA-256, directory inside the archive).
_RUST_COMPONENTS = {
    "cargo": ("d07ee80eb6ba3c5845c554ac88086e1ff5d63f83d99c3c09cb32504cbedd6844", "cargo"),
    "rust-std": ("c73f62fbe5d837f9807ed9e0076575dd8d3475c79af2b30e3570d328e6713f8f", "rust-std-" + _RUST_TRIPLE),
    "rustc": ("fcbfa5b8a0cb28f0c4edfb6dda6910ee910993db31f05258613f5cd5b851263a", "rustc"),
}

_CRATES = "https://static.crates.io/crates/{name}/{name}-{version}.crate"
_REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"

def cargo_lock_packages(text):
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

def _vendored_crates_impl(ctx):
    pending = []
    for package in cargo_lock_packages(ctx.read(ctx.attr.lock)):
        source = package.get("source")
        if source == None:
            continue
        if source != _REGISTRY or "checksum" not in package:
            fail("Unsupported Cargo.lock source for %s: %s" % (package["name"], source))
        directory = "%s-%s" % (package["name"], package["version"])
        archive = "crates/%s.crate" % directory
        pending.append((directory, package["checksum"], archive, ctx.download(
            url = _CRATES.format(name = package["name"], version = package["version"]),
            output = archive,
            sha256 = package["checksum"],
            block = False,
        )))
    for directory, checksum, archive, download in pending:
        download.wait()
        ctx.extract(archive, output = "vendor/" + directory, strip_prefix = directory, type = "tar.gz")
        ctx.file("vendor/%s/.cargo-checksum.json" % directory, json.encode({"files": {}, "package": checksum}))
        ctx.delete(archive)
    ctx.file("BUILD.bazel", """\
package(default_visibility = ["//visibility:public"])

filegroup(
    name = "vendor",
    srcs = glob(["vendor/**"]),
)
""")

vendored_crates = repository_rule(
    implementation = _vendored_crates_impl,
    doc = "Downloads every crates.io package of a Cargo.lock by checksum into vendor/, for Cargo's directory source.",
    attrs = {"lock": attr.label(mandatory = True, allow_single_file = True)},
)

def _rust_linux_impl(ctx):
    for component, (sha256, inner) in _RUST_COMPONENTS.items():
        name = "%s-%s-%s" % (component, _RUST_VERSION, _RUST_TRIPLE)
        ctx.download_and_extract(
            url = _RUST_DIST + name + ".tar.xz",
            sha256 = sha256,
            output = component,
            strip_prefix = name + "/" + inner,
        )
    ctx.file("BUILD.bazel", """\
package(default_visibility = ["//visibility:public"])

filegroup(
    name = "toolchain",
    srcs = glob(["cargo/**", "rust-std/**", "rustc/**"]),
)
""")

rust_linux = repository_rule(
    implementation = _rust_linux_impl,
    doc = "Rust %s for %s: rustc, cargo and the standard library as separate trees." % (_RUST_VERSION, _RUST_TRIPLE),
)

def _cargo_impl(module_ctx):
    vendored_crates(name = "launcher_crates", lock = Label("//launcher:Cargo.lock"))
    rust_linux(name = "rust_aarch64_linux")
    return module_ctx.extension_metadata(reproducible = True)

cargo = module_extension(
    implementation = _cargo_impl,
    doc = "Provides the launcher's vendored crates and the build VM's Rust toolchain.",
)
