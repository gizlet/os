"""Kernel and U-Boot patches from immutable commits of the Gizlet ROCKNIX fork.

Files keep their paths in the fork, so a label names its source directly:
@rocknix//:projects/Rockchip/patches/linux/RK3326/000-rk3326-dts.patch.
Boards select and order the patches they apply.
"""

load("//third_party:pinned_files.bzl", "pinned_files_extension")

_RAW = "https://raw.githubusercontent.com/gizlet/rocknix/"

# Linux 6.12 patches, at the historical object that also carries upstream's 20250517 tag.
_LINUX = "fec842b58559ee973a0100abd726370e4a032b77"

# Hardkernel U-Boot build fix and the RK3326 defconfig.
_UBOOT = "cefc14339a4ae18c02bb69888f27835e50664b60"

_FILES = {
    "packages/kernel/linux/patches/mainline/0001-gpiolib-of-revert-api-changes-needed-for-joypad-driv.patch": (_LINUX, "38f9246adfda80c9c50d061b4f935317ec4f62cfbcc3f16ceb4ef844113cb8aa"),
    "packages/kernel/linux/patches/mainline/0002-input-add-input-polldev-driver.patch": (_LINUX, "aa49ec749c71cd97f02a9236ef0d42780229fcb6ee59d6b07ff8acd3abb72303"),
    "projects/Rockchip/patches/linux/RK3326/000-rk3326-dts.patch": (_LINUX, "b41b00083fdcfd7862bca6c8b5ead1876e11dd094f4d521d2e17745c7e144a58"),
    "projects/Rockchip/patches/linux/RK3326/001-panel-updates.patch": (_LINUX, "58b1f9a73ebfa36e249c4dc527a8f57c5f4cfcd14dcf594cd70875c61ecc6825"),
    "projects/Rockchip/patches/linux/RK3326/004-input-drivers.patch": (_LINUX, "27d2c92560ac22652763f587214a920641ff9bc7e185acc21b4f1058b26c3dc5"),
    "projects/ROCKNIX/devices/RK3326/packages/u-boot-legacy/patches/01-fix-building.patch": (_UBOOT, "b051d4ed375c6906317eeddf538e0e3d1d87088a62e58a0432c7960bd990201a"),
    "projects/ROCKNIX/devices/RK3326/packages/u-boot-legacy/sources/configs/rocknix_rk3326_defconfig": (_UBOOT, "f2b66d895ee7c67639f8eb526466079d72678f84123709aea8b44cb6f203d738"),
}

rocknix = pinned_files_extension(
    name = "rocknix",
    files = {path: [_RAW + commit + "/" + path, sha256] for path, (commit, sha256) in _FILES.items()},
    doc = "Provides the pinned files of the Gizlet ROCKNIX fork.",
)
