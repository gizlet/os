"""Facts about the R36S Panel 0 card that the image tests check."""

_PMIC = "/i2c@ff180000/pmic@20"

_OTG = "/usb@ff300000"

_OTG_PHY = "/syscon@ff2c0000/usb2phy@100/otg-port"

# Layout and boot chain, matching sdcard.cfg and the pinned ROCKNIX 20261101-a IDB.
R36S_CARD = {
    "disk_signature": "36335030",
    "size_mib": 160,
    # [boot flag, MBR type, first sector, sectors]
    "partitions": [
        [0x80, 0x0e, 32768, 131072],
        [0x00, 0x83, 163840, 32768],
        [0x00, 0x83, 196608, 131072],
    ],
    # LBA 64 through the last byte of the pinned miniloader.
    "idbloader": {"sha256": "2fc42ada721b84d8bd40a9a3486abde92d869a5124cad14789743205b1168384", "size": 176452},
    "boot_files": {"script": "boot.scr", "kernel": "Image", "dtb": "r36s.dtb"},
    "device_tree": {
        "properties": [
            ["/", "model", "Game Console R36S"],
            ["/dsi@ff450000/panel@0", "compatible", "gameconsole,r36s-panel sitronix,st7703"],
            ["/aliases", "mmc1", "/mmc@ff370000"],
            ["/mmc@ff370000", "status", "okay"],
            ["/aliases", "serial2", "/serial@ff160000"],
            ["/serial@ff160000", "status", "okay"],
            [_OTG, "status", "okay"],
            [_OTG, "dr_mode", "host"],
            [_OTG_PHY, "status", "okay"],
        ],
        "links": [
            [_OTG, "phys", _OTG_PHY],
            [_OTG, "vbus-supply", _PMIC + "/regulators/OTG_SWITCH"],
            [_PMIC, "vcc9-supply", _PMIC + "/regulators/BOOST"],
        ],
    },
}

# Filesystem UUID of the data partition, set in sdcard.cfg and checked by rcS.
R36S_DATA_UUID = "17605036-fda0-4633-a95b-2ed27409a90c"
