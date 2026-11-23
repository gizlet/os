"""Pinned libkrun and libkrunfw sources for the build VM.

libkrun is built from its release source with every crate from its Cargo.lock
vendored by checksum, so Cargo runs offline. libkrunfw ships its guest kernel as
a prebuilt C bundle that any host C compiler turns into the loadable library.
"""

_LIBKRUN_VERSION = "1.19.8"
_LIBKRUN_SHA256 = "8896c71117462f868f41d26efd157c3270091814eaab3af27ca70c6857a85eb2"

_LIBKRUNFW_VERSION = "5.6.4"
_LIBKRUNFW_AARCH64_SHA256 = "db126ed9775e0f92fc910a90a8b7ff52bdf55644b5edc75977115f81603e48c9"

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
        elif line.startswith("[") and line != "[[package]]":
            current = None
        elif current != None and " = \"" in line:
            key, value = line.split(" = ", 1)
            current[key] = value.strip("\"")
    return packages

def _libkrun_impl(ctx):
    ctx.download_and_extract(
        url = "https://github.com/containers/libkrun/archive/refs/tags/v%s.tar.gz" % _LIBKRUN_VERSION,
        sha256 = _LIBKRUN_SHA256,
        strip_prefix = "libkrun-" + _LIBKRUN_VERSION,
    )
    crates = []
    for package in _lock_packages(ctx.read("Cargo.lock")):
        source = package.get("source")
        if source == None:
            continue
        if source != _REGISTRY or "checksum" not in package:
            fail("Unsupported Cargo.lock source for %s: %s" % (package["name"], source))
        name, version, checksum = package["name"], package["version"], package["checksum"]
        archive = "crates/%s-%s.crate" % (name, version)
        crates.append((name + "-" + version, checksum, archive, ctx.download(
            url = _CRATES.format(name = name, version = version),
            output = archive,
            sha256 = checksum,
            block = False,
        )))
    for directory, checksum, archive, download in crates:
        download.wait()
        ctx.extract(archive, output = "vendor/" + directory, strip_prefix = directory, type = "tar.gz")
        ctx.file("vendor/%s/.cargo-checksum.json" % directory, json.encode({"files": {}, "package": checksum}))
        ctx.delete(archive)
    ctx.file(".cargo/config.toml", """\
[source.crates-io]
replace-with = "vendored"

[source.vendored]
directory = "vendor"

[net]
offline = true
""")
    ctx.file("BUILD.bazel", """\
load("@rules_cc//cc:defs.bzl", "cc_library")

package(default_visibility = ["//visibility:public"])

cc_library(
    name = "headers",
    hdrs = ["include/libkrun.h"],
    includes = ["include"],
)

filegroup(
    name = "workspace",
    srcs = glob(["**"], exclude = ["BUILD.bazel", "WORKSPACE*", "MODULE.bazel"]),
)

filegroup(
    name = "init_sources",
    srcs = glob(["src/init_blob/init/*.c", "src/init_blob/init/*.h"]),
)

exports_files(["Cargo.toml", "LICENSE", "include/libkrun.h"])
""")

_libkrun = repository_rule(implementation = _libkrun_impl)

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

def _krun_repositories_impl(module_ctx):
    _libkrun(name = "libkrun")
    _libkrunfw(name = "libkrunfw")
    return module_ctx.extension_metadata(reproducible = True)

krun_repositories = module_extension(
    implementation = _krun_repositories_impl,
    doc = "Provides the pinned libkrun source with vendored crates and the libkrunfw aarch64 kernel bundle.",
)
