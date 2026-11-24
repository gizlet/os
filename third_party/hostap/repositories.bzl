"""wpa_supplicant release archives from the hostap project."""

load("//third_party:pinned_files.bzl", "pinned_files_extension")

hostap = pinned_files_extension(
    name = "hostap",
    files = {
        "wpa_supplicant-2.12.tar.gz": ["https://w1.fi/releases/wpa_supplicant-2.12.tar.gz", "08e23937e16d0155e55cab2b51f51fbe10d80a1aa91c4e15442645059b737ef6"],
    },
    doc = "Provides pinned wpa_supplicant source archives.",
)
