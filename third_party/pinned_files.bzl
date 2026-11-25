"""Repositories of individually downloaded, SHA-256 pinned upstream files."""

def _pinned_files_impl(ctx):
    downloads = []
    for name, (url, sha256) in ctx.attr.files.items():
        if name.startswith("/") or ".." in name.split("/"):
            fail("Invalid file name: " + name)
        downloads.append(ctx.download(url, name, sha256 = sha256, block = False))
    for download in downloads:
        download.wait()
    ctx.file("BUILD.bazel", "exports_files(%s, visibility = [\"//visibility:public\"])\n" % repr(sorted(ctx.attr.files.keys())))

pinned_files = repository_rule(
    implementation = _pinned_files_impl,
    doc = "Downloads each file without unpacking it. Archives stay intact for the native stages.",
    attrs = {"files": attr.string_list_dict(mandatory = True, doc = "File name -> [URL, SHA-256].")},
)

def pinned_files_extension(name, files, doc):
    """Returns a module extension that provides one pinned_files repository."""

    def _impl(module_ctx):
        pinned_files(name = name, files = files)
        return module_ctx.extension_metadata(reproducible = True)

    return module_extension(implementation = _impl, doc = doc)
