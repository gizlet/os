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
    "packages/kernel/linux/patches/mainline/0003-pwm-add-pwm_set_period.patch": (_LINUX, "6f0099b67a0ed9d00e60a3f5d3000545e213ee278716054641dfe7cf28042703"),
    "packages/kernel/linux/patches/mainline/0004-input-adc-keys-redirect-keycode-316-to-rocknix-joypa.patch": (_LINUX, "05f4739d632eaa375d9cab2f94cf554c1e8ad04f4a0700e9b510e031da70e29b"),
    "packages/kernel/linux/patches/mainline/0005-Bluetooth-btrtl-Add-the-support-for-RTL8733BU.patch": (_LINUX, "b4a9ba4cbc09e99730bbe3c40ba02b0021de61c58b2df54f2244a4c01fdb6316"),
    "packages/kernel/linux/patches/mainline/0006-stable-rocknix-joypad-dtsi.patch": (_LINUX, "98844401a2df0a3ff1c3ff13c8eb250d43f628cf2c443a1d3a1f0c49b4cfdd4c"),
    "packages/kernel/linux/patches/panfrost/0005-drm-panfrost-Add-SYSTEM_TIMESTAMP-and-SYSTEM_TIMESTA.patch": (_LINUX, "8d31354b11ea7a6e0a1938fb1434bd4e0b8e00f63f75a94a471aa9cbd4ff7aaf"),
    "packages/kernel/linux/patches/panfrost/0006-drm-panfrost-Add-cycle-counter-job-requirement.patch": (_LINUX, "56820158ace603155641047d4a80c013c2c91f2695fcd6be1310bd35153a0fe0"),
    "projects/Rockchip/patches/linux/mainline/0005-rk817-stop-updating-info-in-suspend.patch": (_LINUX, "a12a1fcae0b01ef975f97895b31867c708f9263e669ddbeeaadfbb8b3a00bf7e"),
    "projects/Rockchip/patches/linux/mainline/0006-rk817-update-battery-capacity-calibration.patch": (_LINUX, "6beabeb4ec79912c7cb1362d29936b012b534ca7da5c51f34243e21e18a834b4"),
    "projects/Rockchip/patches/linux/RK3326/000-rk3326-dts.patch": (_LINUX, "b41b00083fdcfd7862bca6c8b5ead1876e11dd094f4d521d2e17745c7e144a58"),
    "projects/Rockchip/patches/linux/RK3326/001-panel-updates.patch": (_LINUX, "58b1f9a73ebfa36e249c4dc527a8f57c5f4cfcd14dcf594cd70875c61ecc6825"),
    "projects/Rockchip/patches/linux/RK3326/003-dmc-driver.patch": (_LINUX, "36ede6f0b5efdc1ea4beb8257d0cee77fe49bb634919d6dd1385540993d52ce6"),
    "projects/Rockchip/patches/linux/RK3326/004-input-drivers.patch": (_LINUX, "27d2c92560ac22652763f587214a920641ff9bc7e185acc21b4f1058b26c3dc5"),
    "projects/Rockchip/patches/linux/RK3326/005-unigue-gpio-guid.patch": (_LINUX, "cf1610c31d3a9c5dd9b753bf95f48fac4d1896c3126d3a02ae191f7140162926"),
    "projects/Rockchip/patches/linux/RK3326/006-powkiddy-rgb10x.patch": (_LINUX, "4e9ce035a52c137c8831729ad148b7ea91e07bce83dad2a9c922eed1c6b6403c"),
    "projects/Rockchip/patches/linux/RK3326/007-ogs-panel-timings.patch": (_LINUX, "ee5944640097a73063acf519e8c3e6ec85b44d99f41829343401a951cbc0b818"),
    "projects/Rockchip/patches/linux/RK3326/020-elida-refresh-rates.patch": (_LINUX, "db63effaebce7155790e9a30ae93f711e624fac76470f70a6873fda0217dbb5a"),
    "projects/Rockchip/patches/linux/RK3326/021-magicx-xu-mini-m.patch": (_LINUX, "4d53355ad506048ea4f3950f4b6d226647b4604206950fd46918a0f20ec97822"),
    "projects/Rockchip/patches/linux/RK3326/022-usb-role-switch.patch": (_LINUX, "f22ff902b22f636dc179e279d9f203ea8a49f349aebf50e1c3633644aef1a536"),
    "projects/Rockchip/patches/linux/RK3326/023-gameconsole-r36max.patch": (_LINUX, "6c1a710ccb21420849ed7f0daf0171cb26d9ab4d9bc123daf1fa8cb629cd5edd"),
    "projects/ROCKNIX/devices/RK3326/packages/u-boot-legacy/patches/01-fix-building.patch": (_UBOOT, "b051d4ed375c6906317eeddf538e0e3d1d87088a62e58a0432c7960bd990201a"),
    "projects/ROCKNIX/devices/RK3326/packages/u-boot-legacy/sources/configs/rocknix_rk3326_defconfig": (_UBOOT, "f2b66d895ee7c67639f8eb526466079d72678f84123709aea8b44cb6f203d738"),
}

rocknix = pinned_files_extension(
    name = "rocknix",
    files = {path: [_RAW + commit + "/" + path, sha256] for path, (commit, sha256) in _FILES.items()},
    doc = "Provides the pinned files of the Gizlet ROCKNIX fork.",
)
