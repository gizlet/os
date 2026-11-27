"""Facts about the R36S card that the image tests check."""

_PMIC = "/i2c@ff180000/pmic@20"

_OTG = "/usb@ff300000"

_OTG_PHY = "/syscon@ff2c0000/usb2phy@100/otg-port"

# The legacy U-Boot reads a board revision from SARADC channel 0 during
# board_init and loads a kernel device tree of the matching name from the FAT
# partition (cmd/hwrev.c). Without it, U-Boot writes error.log and halts in an
# LED loop before running boot.scr. The card carries the R36S tree under every
# name the revision check can choose.
R36S_UBOOT_DTB_NAMES = [
    "rk3326-odroidgo2-linux-v11.dtb",
    "rk3326-odroidgo2-linux.dtb",
    "rk3326-odroidgo3-linux.dtb",
    "rk3326-anbernic-rg351v.dtb",
    "rk3326-gameconsole-r33s.dtb",
    "rk3326-magicx-xu10.dtb",
    "rk3326-gameforce-chi.dtb",
]

# Layout and boot chain, matching sdcard.cfg and the pinned ROCKNIX 20261101-a IDB.
R36S_CARD = {
    "disk_signature": "36335030",
    "size_mib": 464,
    # [boot flag, MBR type, first sector, sectors]
    "partitions": [
        [0x80, 0x0e, 32768, 131072],
        [0x00, 0x83, 163840, 524288],
        [0x00, 0x83, 688128, 262144],
    ],
    # LBA 64 through the last byte of the pinned miniloader.
    "idbloader": {"sha256": "2fc42ada721b84d8bd40a9a3486abde92d869a5124cad14789743205b1168384", "size": 176452},
    "boot_files": {"script": "boot.scr", "kernel": "Image", "dtb": "r36s.dtb"},
    "dtb_copies": R36S_UBOOT_DTB_NAMES,
    "device_tree": {
        "properties": [
            ["/", "model", "Game Console R36S"],
            ["/dsi@ff450000/panel@0", "compatible", "rocknix,generic-dsi"],
            ["/dsi@ff450000/panel@0", "gizlet,r36s-display-probe", ""],
            ["/dsi@ff450000/panel@0/panel-profiles/st7703", "gizlet,display-ids", "56 33 31 131 18 241"],
            ["/aliases", "mmc1", "/mmc@ff370000"],
            ["/mmc@ff370000", "status", "okay"],
            ["/aliases", "serial2", "/serial@ff160000"],
            ["/serial@ff160000", "status", "okay"],
            [_OTG, "status", "okay"],
            [_OTG, "dr_mode", "host"],
            [_PMIC + "/regulators/LDO_REG9", "regulator-name", "vcc_wifi"],
            [_OTG_PHY, "status", "okay"],
            ["/usb@ff340000", "status", "okay"],
            ["/usb@ff350000", "status", "okay"],
        ],
        "links": [
            [_OTG, "phys", _OTG_PHY],
            [_PMIC, "vcc9-supply", _PMIC + "/regulators/BOOST"],
        ],
    },
}

# Filesystem UUID of the data partition, set in sdcard.cfg and checked by rcS.
R36S_DATA_UUID = "cef58f4f-a7a3-40ac-9cb3-bd8d89aab65f"
