"""ROCKNIX's out-of-tree joypad drivers (GPL), built into the kernel by boards."""

load("//third_party:pinned_files.bzl", "pinned_files_extension")

# The revision ROCKNIX's rocknix-joypad package uses at the pinned fork commit.
_RAW = "https://raw.githubusercontent.com/ROCKNIX/rocknix-joypad/ac1be0fa0c4e1f7feb9a900fc428d7ac6014029c/"

rocknix_joypad = pinned_files_extension(
    name = "rocknix_joypad",
    files = {
        "rocknix-joypad.h": [_RAW + "rocknix-joypad.h", "6b23ccd136c8c6ab747aeb24050526f2be15ad11735d502d461553deaea12589"],
        "rocknix-singleadc-joypad.c": [_RAW + "rocknix-singleadc-joypad.c", "233dd744261d3724fb3d3bbb0c5411d3ee0f2a3509efa98bdee4a52fa81189cb"],
    },
    doc = "Provides the pinned rocknix-joypad driver sources.",
)
