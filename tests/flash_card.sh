#!/usr/bin/env bash
# flash_card.sh FLASH_SH IMAGE CARD BYTES: writes IMAGE to the file CARD of
# BYTES bytes as flash.sh writes a card: the image, its partition table with
# the data partition extended to the end, and the data filesystem grown to fill
# it. Prints the number of filesystem blocks written to grow it.
set -euo pipefail
flash=$1 image=$2 card=$3 bytes=$4
m() { printf '%s' "$1"; }
die() { echo "error: $*" >&2; exit 1; }
# The writer's own functions, from grown_mbr to the end of the script.
eval "$(sed -n '/^# grown_mbr IMAGE/,$p' "${flash}" | sed '$d')"
work=$(mktemp -d)
trap 'rm -rf "${work}"' EXIT
sudo=''
rm -f "${card}"
dd if="${image}" of="${card}" bs=4194304 2>/dev/null
truncate -s "${bytes}" "${card}"
grown_mbr "${image}" "${bytes}" "${work}/mbr"
grown_data "${image}" "${work}/mbr" "${work}"
dd if="${work}/mbr" of="${card}" bs=512 count=1 conv=notrunc 2>/dev/null
write_grown_data "${work}" "${card}"
[ -z "${FLASH_CARD_KEEP:-}" ] || cp "${work}/list" "${work}/blocks" "${FLASH_CARD_KEEP}"
wc -l < "${work}/list" | tr -d ' '
