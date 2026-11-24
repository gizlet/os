"""libnl release archives, linked statically into wpa_supplicant."""

load("//third_party:pinned_files.bzl", "pinned_files_extension")

libnl = pinned_files_extension(
    name = "libnl",
    files = {
        "libnl-3.12.0.tar.gz": ["https://github.com/thom311/libnl/releases/download/libnl3_12_0/libnl-3.12.0.tar.gz", "fc51ca7196f1a3f5fdf6ffd3864b50f4f9c02333be28be4eeca057e103c0dd18"],
    },
    doc = "Provides pinned libnl source archives.",
)
