"""Dropbear SSH server release archives."""

load("//third_party:pinned_files.bzl", "pinned_files_extension")

dropbear = pinned_files_extension(
    name = "dropbear",
    files = {
        "dropbear-2026.94.tar.bz2": ["https://matt.ucc.asn.au/dropbear/releases/dropbear-2026.94.tar.bz2", "e098034a843699200c8c977a991fff73159735bf795d5f72ef672c41a6b1ae81"],
    },
    doc = "Provides pinned Dropbear source archives.",
)
