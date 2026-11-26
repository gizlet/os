"""Bun's Linux arm64 release and its license notices."""

_TAG = "bun-v1.4.5"

def _bun_impl(ctx):
    ctx.download_and_extract(
        url = "https://github.com/oven-sh/bun/releases/download/%s/bun-linux-aarch64.zip" % _TAG,
        sha256 = "18656a3bf0ea5641f5796ae4213b75255579a70a0fded2f337933785568cdf84",
        strip_prefix = "bun-linux-aarch64",
    )
    ctx.download(
        url = "https://raw.githubusercontent.com/oven-sh/bun/%s/LICENSE.md" % _TAG,
        output = "LICENSE.md",
        sha256 = "362fe32c954ec829240bd7429c53d705eef08c6bfa45ffc2d02005f857cba4dd",
    )
    ctx.file("BUILD.bazel", 'exports_files(["bun", "LICENSE.md"], visibility = ["//visibility:public"])\n')

_bun_repository = repository_rule(implementation = _bun_impl)

def _bun_extension(module_ctx):
    _bun_repository(name = "bun")
    return module_ctx.extension_metadata(reproducible = True)

bun = module_extension(implementation = _bun_extension)
