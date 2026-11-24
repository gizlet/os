"""Individual files from the linux-firmware repository, tag 20261019."""

load("//third_party:pinned_files.bzl", "pinned_files_extension")

linux_firmware = pinned_files_extension(
    name = "linux_firmware",
    files = {
        # rtl8xxxu firmware for the on-board WiFi of R36S V30 boards: an
        # RTL8188EU (0bda:0179) on the tested unit; the stock kernel also
        # carries an RTL8188FU driver.
        "rtl8188eufw.bin": ["https://git.kernel.org/pub/scm/linux/kernel/git/firmware/linux-firmware.git/plain/rtlwifi/rtl8188eufw.bin?h=20261019", "2ff74315287529dec2e50eb57d6e0c97d2116f28ae166773ccdf93b6360000c4"],
        "rtl8188fufw.bin": ["https://git.kernel.org/pub/scm/linux/kernel/git/firmware/linux-firmware.git/plain/rtlwifi/rtl8188fufw.bin?h=20261019", "e6189d6d7032b83f0ed4284629c43db66262449a866f33fd329e611239f62e1f"],
        "LICENCE.rtlwifi_firmware.txt": ["https://git.kernel.org/pub/scm/linux/kernel/git/firmware/linux-firmware.git/plain/LICENSES/LICENCE.rtlwifi_firmware.txt?h=20261019", "a61351665b4f264f6c631364f85b907d8f8f41f8b369533ef4021765f9f3b62e"],
    },
    doc = "Provides pinned linux-firmware files.",
)
