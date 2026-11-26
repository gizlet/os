"""Debian packages pinned by snapshot lock files: the build VM, the Gizlet OS base and the Gizlet runtime."""

_SNAPSHOT = "https://snapshot.debian.org/archive/debian/{snapshot}/{path}"

def _debian_packages_impl(ctx):
    lock = json.decode(ctx.read(ctx.attr.lock))
    downloads = []
    names = []
    for package in lock["packages"]:
        output = "debs/%s.deb" % package["name"]
        names.append(output)
        downloads.append(ctx.download(
            url = _SNAPSHOT.format(snapshot = lock["snapshot"], path = package["path"]),
            output = output,
            sha256 = package["sha256"],
            block = False,
        ))
    for download in downloads:
        download.wait()

    if not ctx.attr.bootstrap:
        ctx.file("BUILD.bazel", """\
package(default_visibility = ["//visibility:public"])

filegroup(
    name = "debs",
    srcs = {debs},
)
""".format(debs = repr(names)))
        return

    # The bootstrap root holds only the static BusyBox. It unpacks the other
    # packages inside the guest, where the filesystem is case-sensitive.
    ctx.extract("debs/busybox-static.deb", output = "unpack", type = "deb")
    data = [entry.basename for entry in ctx.path("unpack").readdir() if entry.basename.startswith("data.tar")]
    if len(data) != 1:
        fail("Unexpected busybox-static.deb layout: %s" % data)
    ctx.extract("unpack/" + data[0], output = "unpack/data")
    ctx.file("bootstrap/bin/.keep", "")
    ctx.rename("unpack/data/bin/busybox", "bootstrap/bin/busybox")
    ctx.delete("unpack")
    for directory in ("dev", "in", "out", "proc", "sys", "tmp"):
        ctx.file("bootstrap/%s/.keep" % directory, "")
    ctx.file("BUILD.bazel", """\
package(default_visibility = ["//visibility:public"])

filegroup(
    name = "debs",
    srcs = {debs},
)

exports_files(["bootstrap/bin/busybox"])
""".format(debs = repr(names)))

debian_packages = repository_rule(
    implementation = _debian_packages_impl,
    attrs = {
        "bootstrap": attr.bool(doc = "Also provide the static BusyBox bootstrap root of the build VM."),
        "lock": attr.label(mandatory = True, allow_single_file = True),
    },
)

def _debian_impl(module_ctx):
    debian_packages(name = "debian_bookworm_arm64", bootstrap = True, lock = Label("//devops/bazel/debian:packages.lock.json"))
    debian_packages(name = "debian_trixie_arm64_base", lock = Label("//rootfs:packages.lock.json"))
    debian_packages(name = "debian_trixie_arm64_runtime", lock = Label("//runtime:packages.lock.json"))
    return module_ctx.extension_metadata(reproducible = True)

debian = module_extension(
    implementation = _debian_impl,
    doc = "Provides the build VM's locked bookworm packages with its bootstrap root, and the locked trixie packages of the Gizlet OS base and the Gizlet runtime.",
)
