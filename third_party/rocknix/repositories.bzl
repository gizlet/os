"""Files from an immutable commit of the Gizlet ROCKNIX fork.

The commit is on the fork's `next` branch, which follows upstream ROCKNIX.
Files keep their paths in the fork, so a label names its source directly:
@rocknix//:projects/ROCKNIX/devices/RK3326/patches/linux/001-panel-updates.patch.
Boards select and order the patches they apply.
"""

load("//third_party:pinned_files.bzl", "pinned_files_extension")

_COMMIT = "21d9dc6759cc3016c2e2e7244574c0e6a5a6bcd8"

_RAW = "https://raw.githubusercontent.com/gizlet/rocknix/%s/" % _COMMIT

_FILES = {
    "projects/ROCKNIX/devices/RK3326/linux/dts/rockchip/rk3326-device-group-r36.dtsi": "c10b8c5eb2e0e030f4ee11562044f5348f5de62285f24de5b500ba11729dcbe6",
    "projects/ROCKNIX/devices/RK3326/linux/dts/rockchip/rk3326-gameconsole-r36s.dts": "4062ff07b3371d33eecbd9673285d7c03c6c256b2d9cc043f9f7144bd3dbd3df",
    "projects/ROCKNIX/devices/RK3326/linux/dts/rockchip/rk3326-gameconsole-r3xs.dtsi": "505ca1a7e213dd15d10630b895db2a044f892d913f146d7d0a5a5ead00a4d771",
    "projects/ROCKNIX/devices/RK3326/linux/dts/rockchip/rk3326.dtsi": "f8efedadd2d9df56c558f1815836d99c333d9fad9b5d7b048b958fbc1f5c5e7e",
    "projects/ROCKNIX/devices/RK3326/packages/u-boot-legacy/patches/01-fix-building.patch": "b051d4ed375c6906317eeddf538e0e3d1d87088a62e58a0432c7960bd990201a",
    "projects/ROCKNIX/devices/RK3326/packages/u-boot-legacy/sources/configs/rocknix_rk3326_defconfig": "f2b66d895ee7c67639f8eb526466079d72678f84123709aea8b44cb6f203d738",
    "projects/ROCKNIX/devices/RK3326/patches/linux/0008-mmc-sd-retry-a-failed-resume-after-a-power-cycle.patch": "fffead7224c807d00757ff94fe9257ca802815086f14b16b9106bb48d5daf7ae",
    "projects/ROCKNIX/devices/RK3326/patches/linux/001-panel-updates.patch": "58b1f9a73ebfa36e249c4dc527a8f57c5f4cfcd14dcf594cd70875c61ecc6825",
    "projects/ROCKNIX/devices/RK3326/patches/linux/002-gpiolib-of-legacy-api.patch": "8a5f2aba22021c72c98441c8c49e1d471d1819b21f8ea3b2787fa57d2603b6c8",
    "projects/ROCKNIX/devices/RK3326/patches/linux/003-input-add-input-polldev-driver.patch": "aa49ec749c71cd97f02a9236ef0d42780229fcb6ee59d6b07ff8acd3abb72303",
    "projects/ROCKNIX/devices/RK3326/patches/linux/003-usb-dwc2-gadget-fix-null-deref-on-register-restore.patch": "05b1350c380f438bf874cabae3e6ecc7e35792e9b2f40172f9886ac0d063ddf1",
    "projects/ROCKNIX/devices/RK3326/patches/linux/004-input-drivers.patch": "1c325fefb683e44cbb94c3dc4e3997b42e5dc2e00b5984edbca28635c53bdb49",
    "projects/ROCKNIX/devices/RK3326/patches/linux/004-usb-dwc2-skip-pm-callbacks-after-failed-core-reset.patch": "52427b0374f9fa5a1a4138103acff50e3c5062e68c67cd52b193ce08eb2b8128",
    "projects/ROCKNIX/devices/RK3326/patches/linux/005-unigue-gpio-guid.patch": "cf1610c31d3a9c5dd9b753bf95f48fac4d1896c3126d3a02ae191f7140162926",
    "projects/ROCKNIX/devices/RK3326/patches/linux/007-ogs-panel-timings.patch": "ee5944640097a73063acf519e8c3e6ec85b44d99f41829343401a951cbc0b818",
    "projects/ROCKNIX/devices/RK3326/patches/linux/008-esp-8089-wifi.patch": "1dc79609f1ab458ff98d331ec05bfadf48e1f3eb81791ebb474c26cae1bea3e4",
    "projects/ROCKNIX/devices/RK3326/patches/linux/010-dwc2-track-vbus-supply-state.patch": "e8d69feb02470ba66b8beaa08145acffa9320b90153c3664d89a32167f29a75c",
    "projects/ROCKNIX/devices/RK3326/patches/linux/023-px30s-tsadc-support.patch": "ae629abb15b5b0de2edbeabec17fa1ca0202c0a09255ad043d280963d47b2bd0",
    "projects/ROCKNIX/devices/RK3326/patches/linux/024-px30s-gpu-power-domain-workaround.patch": "11671f3746256af62c486c73a8b9a52968e28cb05667b132cacf99e53a6b06dc",
    "projects/ROCKNIX/devices/RK3326/patches/linux/025-px30s-dsi-dphy-timing-table.patch": "ac3df4c0e6aeaada40fdf035c188a318918f4ab857f2f56204fb056edd7dbaf7",
    "projects/ROCKNIX/devices/RK3326/patches/linux/026-px30s-cpuinfo-soc-detection.patch": "4dafb50edf0a8e3552a82f385e443afc231cf0cce1109393b3a123f07d6cd94f",
    "projects/ROCKNIX/devices/RK3326/patches/linux/027-px30s-io-domain-pmuio1.patch": "c3ce6cd601a77a2a45439eab55794e06687940047e4bb7ad60a0681db1349314",
    "projects/ROCKNIX/devices/RK3326/patches/linux/028-px30s-usb2phy-tuning.patch": "280a3c478eeaab7d4a5412b4994d2cd5eec43fee66c202b06313441fa07b9878",
    "projects/ROCKNIX/devices/RK3326/patches/linux/030-pwm-vibra-kick-stuck-boot-state.patch": "7629548026fefd757863db16e4c1fca5d12203ba6c1f5f3d0fb15e49f8f4bfe0",
    "projects/ROCKNIX/devices/RK3326/patches/linux/031-px30s-suspend-wakeup-config.patch": "f311bf6f93d11740166c5d147a8a1520b47666a1f8b02e29feed02a3ae9be510",
    "projects/ROCKNIX/devices/RK3326/patches/linux/032-px30s-otp-read-protocol.patch": "f2c9324e13fce446e017d3962ef077520f9e5544d84e5a50b4f2b21ac362b899",
    "projects/ROCKNIX/devices/RK3326/patches/linux/033-px30s-ddrphy-clock-topology.patch": "9f77f271f0ce6de57a64035e5c4f4d88e1f98dbb7ed762fe7f0aebd30deb3513",
    "projects/ROCKNIX/devices/RK3326/patches/linux/034-px30s-cru-suspend-resume-restore.patch": "7d3fad0b0c3ba481086331202248b70bd515bd4ed1f769b482fe95e4d72f6407",
    "projects/ROCKNIX/devices/RK3326/patches/linux/035-dwc2-release-vbus-supply-on-role-switch.patch": "11fe2c5e06060c4f54a2371aaeb9dbb3244b7b496edbc50737bf38427c37cbb6",
    "projects/ROCKNIX/devices/RK3326/patches/linux/036-px30-dmc-devfreq.patch": "f4968f5c1e79ecc065defd06dfebd45f3309208b4f3f6ed180253aae04a7e378",
    "projects/ROCKNIX/devices/RK3326/patches/linux/037-px30-dmc-pass-dram-settings-to-bl31.patch": "651745274be27ddd5f1f7c714b90e8d4f6523a791bc09dbcc32a5aa820e10d70",
    "projects/ROCKNIX/devices/RK3326/patches/linux/039-drm-rockchip-vop-add-afbc-support-for-px30.patch": "394c8aa1fdacccecbbc2c27cfbfe25e99848288e4d4faf9d67a6da1c664a2fd9",
    "projects/ROCKNIX/packages/linux-drivers/generic-dsi/sources/panel-generic-dsi.c": "4e2d17acaa4b5b9ccc00fc00f793c516c34b5827bdce1e6d6f0706d943c261ca",
    "projects/ROCKNIX/packages/linux/patches/7.0/0006-hid-playstation-expose-DualSense-Edge-Fn-and-back-paddles.patch": "d7edd724869a3767f70a4c6977771944adf75b7d034acdedc9a5c29e0c808578",
    "projects/ROCKNIX/packages/linux/patches/7.0/0010-msm-resource-cleanup.patch": "851e55cd4f594bee9458c8b395c982fcf458d55bd12020db094cca1bff62de8a",
    "projects/ROCKNIX/packages/linux/patches/7.0/9998-silence-initramfs-unpack-warn.patch": "52a542dd3845eb3ee27b79c0347ee399dad7a6c77c7e5b6445e16ef6d4693b88",
    "projects/ROCKNIX/packages/linux/patches/7.0/9999-fix-rust-build-error.patch": "cda5a402465aac67adf6f8d9afc935cd1adf7bc9e52392b0075a56644a9176c5",
    "projects/ROCKNIX/packages/linux/patches/mainline-rockchip/001-power-supply-rk817-charger-Fix-NVRAM-SoC-Value.patch": "25e0c13e6ceeea755bd8e2913e484690c51087416dc80a64e5a607dd70e95039",
    "projects/ROCKNIX/packages/linux/patches/mainline-rockchip/002-power-supply-rk817-battery-resolve-boot-soc-from-counter.patch": "468ecc66698ebbd49fcfb6a75b25d86bfdaadddac83b5353e338a5204ff7b538",
    "projects/ROCKNIX/packages/linux/patches/mainline-rockchip/005-power-supply-rk817-charger-correct-gauge-across-sleep.patch": "b10030d30366e9db51f9220f799f407f5197c74c8388e887ea3902790121be6d",
    "projects/ROCKNIX/packages/linux/patches/mainline-rockchip/006-dt-bindings-power-supply-rk817-charger-add-rockchip-gate-function-disable.patch": "bea5ed3f6c57da0c901ed1454281e775cd8f090cc8946f4d8d473b2ec61ddfe5",
    "projects/ROCKNIX/packages/linux/patches/mainline-rockchip/007-power-supply-rk817-clear-sys-can-sd-register-by-default.patch": "81386241d31daa3b27418996834ec4ffcc4088f863f30f65f067bd65ae0eb9be",
    "projects/ROCKNIX/packages/linux/patches/mainline-rockchip/008-power-supply-rk817-bound-the-boot-soc-with-the-ocv-table.patch": "9bd9758fafee40cd46bf2f6d3f7f2daff372e53474cc38f7ee5b2a8e3409d911",
    "projects/ROCKNIX/packages/linux/patches/mainline/0003-pwm-add-pwm_set_period.patch": "6f0099b67a0ed9d00e60a3f5d3000545e213ee278716054641dfe7cf28042703",
    "projects/ROCKNIX/packages/linux/patches/mainline/0004-input-adc-keys-redirect-keycode-316-to-rocknix-joypa.patch": "05f4739d632eaa375d9cab2f94cf554c1e8ad04f4a0700e9b510e031da70e29b",
    "projects/ROCKNIX/packages/linux/patches/mainline/0005-Bluetooth-btrtl-Add-the-support-for-RTL8733BU.patch": "b4a9ba4cbc09e99730bbe3c40ba02b0021de61c58b2df54f2244a4c01fdb6316",
}

rocknix = pinned_files_extension(
    name = "rocknix",
    files = {path: [_RAW + path, sha256] for path, sha256 in _FILES.items()},
    doc = "Provides the pinned files of the Gizlet ROCKNIX fork.",
)
