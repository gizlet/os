"""Check board-specific details which QEMU virt does not exercise."""

import hashlib
from pathlib import Path
import struct
import subprocess
import sys


def main(image, dtb):
    binary = image.read_bytes()
    if binary[56:60] != b"ARMd":
        raise ValueError("missing ARM64 Image header")
    text_offset, image_size, flags = struct.unpack_from("<QQQ", binary, 8)
    if text_offset != 0 or flags & 8 == 0 or image_size < len(binary):
        raise ValueError("expected the V30 kernel to run at its aligned 0x02000000 load address")

    # First ten register bytes identify the contiguous 151-entry board profile.
    prefix = bytes.fromhex("ff 30 ff 52 ff 01 e3 00 25 10")
    if binary.count(prefix) != 1:
        raise ValueError("V30 initialization table missing or duplicated in the compiled Image")
    start = binary.index(prefix)
    pairs = binary[start:start + 151 * 2]
    if hashlib.sha256(pairs).hexdigest() != "e5530f046c3ee69b3b2cad35556b4c5082d769a405ed29d791af2811005bb045":
        raise ValueError("V30 DCS register sequence differs from the matched vendor DTB")
    mode = struct.pack("<I10H", 58000, 640, 940, 1140, 1440, 0, 480, 497, 502, 515, 0)
    if mode not in binary:
        raise ValueError("58 MHz V30 display mode missing from the compiled Image")

    def prop(node, name, fmt="s"):
        return subprocess.check_output(["fdtget", "-t", fmt, str(dtb), node, name], text=True).strip()

    def cells(node, name):
        return [int(cell, 16) for cell in prop(node, name, "x").split()]

    def handle(node):
        return cells(node, "phandle")[0]

    def verify(actual, expected):
        if actual != expected:
            raise ValueError(f"expected {expected!r}, found {actual!r}")

    dsi = "/dsi@ff450000"
    panel = dsi + "/panel@0"
    pwm = "/pwm@ff200010"
    uart = "/serial@ff160000"
    regs = "/i2c@ff180000/pmic@20/regulators/"
    verify(prop(panel, "compatible"), "gameconsole,r36s-v30-panel4")
    verify(cells(panel, "reset-gpios"), [handle("/pinctrl/gpio@ff270000"), 16, 1])
    verify(cells(panel, "vdd-supply"), [handle(regs + "LDO_REG8")])
    verify(cells(panel, "backlight"), [handle("/backlight")])
    verify(cells("/backlight", "power-supply"), [handle(regs + "LDO_REG7")])
    verify(cells("/backlight", "pwms"), [handle(pwm), 0, 62745, 0])
    verify(cells("/backlight", "default-brightness-level"), [51])
    verify(prop("/chosen", "stdout-path"), "serial2:115200n8")
    verify(prop("/aliases", "serial2"), uart)
    verify(cells(uart, "pinctrl-0"), [handle("/pinctrl/uart2-m1/uart2m1-xfer")])
    for node in (dsi, pwm, "/display-subsystem", "/vop@ff460000", uart):
        verify(prop(node, "status"), "okay")
    print("PASS: V30 Panel 4 DCS sequence, timing, supplies, reset, backlight and UART2-M1")


if __name__ == "__main__":
    main(Path(sys.argv[1]), Path(sys.argv[2]))
