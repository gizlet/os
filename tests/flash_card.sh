#!/usr/bin/env bash
# flash_card.sh FLASH_SH IMAGE CARD BYTES: writes IMAGE to the file CARD of
# BYTES bytes as flash.sh writes a card: the image, its partition table with
# the data partition extended to the end, and the data filesystem grown to fill
# it. Prints the number of filesystem blocks written to grow it.
#
# flash_card.sh --update FLASH_SH IMAGE CARD: updates Gizlet OS on the card
# file CARD from IMAGE as flash.sh's update does, keeping its data partition.
# A card file has no boot partition device, so no boot files are kept.
set -euo pipefail
update=''
if [ "$1" = --update ]; then update=yes; shift; fi
flash=$1 image=$2 card=$3 bytes=${4:-}
m() { printf '%s' "$1"; }
say() { printf '%s\n' "$*" >&2; }
die() { echo "error: $*" >&2; exit 1; }
# The writer's own functions, from grown_mbr to the end of the script.
eval "$(sed -n '/^# grown_mbr IMAGE/,$p' "${flash}" | sed '$d')"
work=$(mktemp -d)
trap 'rm -rf "${work}"' EXIT
sudo=''
if [ -n "${update}" ]; then
    raw=${image} target=${card} disk=${card} os=linux
    update_card
    exit 0
fi
rm -f "${card}"
truncate -s "${bytes}" "${card}"
raw=${image} target=${card} disk=${card} os=linux
erase_card 2>/dev/null
[ -z "${FLASH_CARD_KEEP:-}" ] || cp "${work}/list" "${work}/blocks" "${FLASH_CARD_KEEP}"
wc -l < "${work}/list" | tr -d ' '
