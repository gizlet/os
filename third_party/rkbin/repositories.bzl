"""Rockchip firmware from rockchip-linux/rkbin. Files keep their upstream paths."""

load("//third_party:pinned_files.bzl", "pinned_files_extension")

_RAW = "https://raw.githubusercontent.com/rockchip-linux/rkbin/74213af1e952c4683d2e35952507133b61394862/"

_FILES = {
    "LICENSE": "d2765124a2c24d71aba3cbc6cdd6975a5a297821264eb6c26378a4c16cd16e92",
    "bin/rk33/rk3326_bl31_v1.34.elf": "387aaee716a6753a9e6dd18c72b27783d125a294f6aa817531a5a6f340f16080",
    "bin/rk33/rk3326_ddr_333MHz_v2.11.bin": "2df577824953bea3584282e7f5118c96620e1348dbc378d50604fe07c70eb3e4",
    "bin/rk33/rk3326_miniloader_v1.40.bin": "473aacd5378f356757a71efeb54f4339925db154fbce288df7e3c1177f45bc28",
}

rkbin = pinned_files_extension(
    name = "rkbin",
    files = {path: [_RAW + path, sha256] for path, sha256 in _FILES.items()},
    doc = "Provides pinned Rockchip rkbin firmware and its license.",
)
