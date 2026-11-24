"""Linux source archives from kernel.org."""

load("//third_party:pinned_files.bzl", "pinned_files_extension")

linux = pinned_files_extension(
    name = "linux",
    files = {
        "linux-7.1.2.tar.xz": ["https://cdn.kernel.org/pub/linux/kernel/v7.x/linux-7.1.2.tar.xz", "37198c93727be247c9fb5309bb86cd5e496c61e5322cd8c4eca9476bb0b5883f"],
    },
    doc = "Provides pinned Linux source archives.",
)
