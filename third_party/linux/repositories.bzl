"""Linux source archives from kernel.org."""

load("//third_party:pinned_files.bzl", "pinned_files_extension")

linux = pinned_files_extension(
    name = "linux",
    files = {
        "linux-6.12.17.tar.xz": ["https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-6.12.17.tar.xz", "5c205cd34f80974e4973e321cb008f5f6895a8aa8c2577f06a9448cd77de63b3"],
    },
    doc = "Provides pinned Linux source archives.",
)
