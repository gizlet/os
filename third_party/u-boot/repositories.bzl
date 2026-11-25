"""Hardkernel U-Boot 2017.09 source, as maintained by ROCKNIX."""

load("//third_party:pinned_files.bzl", "pinned_files_extension")

_COMMIT = "2492a3e467e332e2350d987234ce6123700b3392"

hardkernel_uboot = pinned_files_extension(
    name = "hardkernel_uboot",
    files = {
        "hardkernel-uboot-%s.tar.gz" % _COMMIT: ["https://github.com/ROCKNIX/hardkernel-uboot/archive/%s.tar.gz" % _COMMIT, "d8b358f20394ff1c901af989d0a5b7740f46708bd93ff714459d7bc3794a72f8"],
    },
    doc = "Provides the pinned Hardkernel U-Boot source archive.",
)
