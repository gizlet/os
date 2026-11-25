#!/usr/bin/env bash
# disk_identity in flash.sh, against a stub diskutil, under flash.sh's own
# set -euo pipefail: a Gizlet card ends in an ext4 partition, for which macOS
# reports no UUID, and that must not stop the script; a failing diskutil must.
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
