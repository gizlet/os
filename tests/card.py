"""Locate Gizlet OS partitions on a card image from its MBR."""

import struct

SECTOR = 512


def partitions(image):
    """Returns {"boot", "root", "data"} -> (MBR index from 1, byte offset, byte size), and the disk signature."""
    found = {}
    with open(image, "rb") as card:
        mbr = card.read(SECTOR)
        signature, = struct.unpack_from("<I", mbr, 440)
        for index in range(4):
            boot, kind, start, sectors = struct.unpack_from("<B3xB3xII", mbr, 446 + 16 * index)
            if not sectors:
                continue
            card.seek(start * SECTOR)
            head = card.read(2048)
            where = (index + 1, start * SECTOR, sectors * SECTOR)
            if head[:4] == b"hsqs":
                found["root"] = where
            elif head[1080:1082] == b"\x53\xef":
                found["data"] = where
            elif boot == 0x80:
                found["boot"] = where
    assert set(found) == {"boot", "root", "data"}, f"partitions found: {sorted(found)}"
    return found, signature
