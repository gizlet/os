#!/usr/bin/env bash
# flash.sh's own functions under its set -euo pipefail. disk_identity, against
# a stub diskutil: a Gizlet card ends in an ext4 partition, for which macOS
# reports no UUID, and that must not stop the script; a failing diskutil must.
# The card's panel: stock dtbs by their init sequence, and stock and Gizlet OS
# boot partitions.
set -euo pipefail
script=${1:?usage: flash_test.sh flash.sh}
stub=$(mktemp -d)
trap 'rm -rf "${stub}"' EXIT
cat > "${stub}/diskutil" <<'STUB'
#!/bin/sh
[ -z "${DISKUTIL_FAIL:-}" ] || { echo "diskutil: no such disk" >&2; exit 1; }
case "$1 ${2#/dev/}" in
    "info disk9")
        printf '%s\n' '   Device / Media Name:       MassStorageClass' '   Disk Size:                 53.7 GB' '   Device Location:           External' ;;
    "info disk9s1") printf '%s\n' '   Volume UUID:               168BB977-F806-3D3D-B8F6-37464A97CAAD' ;;
    "info disk9s2" | "info disk9s3") printf '%s\n' '   Volume Name:               Not applicable (no file system)' ;;
    "list disk9")
        printf '%s\n' '/dev/disk9 (external, physical):' \
            '   0:     FDisk_partition_scheme                        *53.7 GB    disk9' \
            '   1:             Windows_FAT_16 GIZLETBOOT              67.1 MB    disk9s1' \
            '   2:                      Linux                         268.4 MB   disk9s2' \
            '   3:                      Linux                         53.4 GB    disk9s3' ;;
    *) exit 1 ;;
esac
STUB
chmod +x "${stub}/diskutil"
eval "$(sed -n '/^disk_identity()/,/^}/p' "${script}")"
os=mac
export PATH="${stub}:${PATH}"

identity=$(disk_identity /dev/disk9)
case "${identity}" in *168BB977*disk9s3* | *disk9s3*168BB977*) ;; *) echo "FAIL: incomplete identity: ${identity}"; exit 1 ;; esac
echo "ok: a card ending in an ext4 partition without a UUID"

if identity=$(DISKUTIL_FAIL=1 disk_identity /dev/disk9); then
    echo "FAIL: a failing diskutil was not reported"
    exit 1
fi
echo "ok: a failing diskutil fails disk_identity"

# The panel of a card: panel_of_dtb on dtbs that hold only what it looks for,
# a property header and then the start of an init sequence, and card_panel on
# boot partitions as directories.
eval "$(sed -n '/^card_panel()/,/^}/p; /^panel_of_dtb()/,/^}/p' "${script}")"
work=${stub}
dtb() { { printf 'junk\0\0\0\3\0\0\0\x40\0\0\0\x10'; printf "$2"; printf 'more'; } > "${stub}/$1"; }
dtb nv.dtb '\x15\0\2\xff\x30\x15\0\2\xff\x52\x15\0\2\xff\x01\x15\0\2\xe3\0'
dtb st.dtb '\x39\0\4\xb9\xf1\x12\x83\x39\0\6\xb1\0\0\0\xda\x80'
dtb plus.dtb '\x05\x78\1\x11\x39\0\4\xb9\xf1\x12\x83'
dtb other.dtb '\x39\0\4\xff\x77\x01\0'
expect() { [ "$2" = "$3" ] || { echo "FAIL: $1: got '$2', want '$3'"; exit 1; }; echo "ok: $1"; }
expect 'an NV3051D dtb is Panel 4' "$(panel_of_dtb "${stub}/nv.dtb")" v30-panel4
expect 'an ST7703 dtb is Panel 0' "$(panel_of_dtb "${stub}/st.dtb")" original
expect "the R36S Plus's dtb is no panel" "$(panel_of_dtb "${stub}/plus.dtb")" ''
expect "another controller's dtb is no panel" "$(panel_of_dtb "${stub}/other.dtb")" ''
expect 'a missing dtb is no panel' "$(panel_of_dtb "${stub}/none.dtb")" ''

boot_dir=${stub}/boot disk=/dev/disk9
mkdir "${boot_dir}"
boot_readable() { :; }
release_boot() { :; }
gizlet=''
has_gizlet_boot() { [ -n "${gizlet}" ]; }
stock() {
    rm -f "${boot_dir}"/*
    cp "${stub}"/*.dtb "${boot_dir}/"
    printf '%s\r\n' 'odroidgoa-uboot-config' 'setenv dtb_loadaddr "0x01f00000"' "$@" 'booti ${loadaddr} ${initrd_loadaddr} ${dtb_loadaddr}' > "${boot_dir}/boot.ini"
    card_panel
    printf '%s %s' "${found}" "${found_on}"
}
expect "a stock card's boot.ini, with CRLF lines" "$(stock 'load mmc 1:1 ${loadaddr} Image' 'load mmc 1:1 ${dtb_loadaddr} nv.dtb')" 'v30-panel4 stock'
expect 'a stock card for Panel 0' "$(stock 'load mmc 1:1 ${dtb_loadaddr} st.dtb')" 'original stock'
expect 'dtbs that disagree' "$(stock 'load mmc 1:1 ${dtb_loadaddr} nv.dtb' 'load mmc 1:1 ${dtb_loadaddr} st.dtb')" ' '
expect 'a missing dtb' "$(stock 'load mmc 1:1 ${dtb_loadaddr} gone.dtb')" ' '
expect 'a dtb outside the partition' "$(stock 'load mmc 1:1 ${dtb_loadaddr} ../nv.dtb')" ' '
expect 'a boot.ini without a dtb' "$(stock 'load mmc 1:1 ${loadaddr} Image')" ' '

gizlet=yes
written() {
    rm -f "${boot_dir}"/*
    printf 'panel 4' > "${boot_dir}/r36s-v30-panel4.dtbo"
    [ -z "$1" ] || printf '%s' "$1" > "${boot_dir}/mipi-panel.dtbo"
    card_panel
    printf '%s %s' "${found}" "${found_on}"
}
expect 'a Gizlet OS card for Panel 0' "$(written '')" 'original gizlet'
expect 'a Gizlet OS card for Panel 4' "$(written 'panel 4')" 'v30-panel4 gizlet'
expect 'a Gizlet OS card with an unknown overlay' "$(written 'another')" ' '
