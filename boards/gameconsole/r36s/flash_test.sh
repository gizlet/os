#!/usr/bin/env bash
# flash.sh's own functions under its set -euo pipefail. disk_identity, against
# a stub diskutil: a Gizlet card ends in an ext4 partition, for which macOS
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

eval "$(sed -n '/^main()/,/^}/p' "${script}")"
unset GIZLET_IMAGE_URL GIZLET_CHECKSUM_URL GIZLET_RELEASE
choose_language() { :; }
say() { :; }
m() { printf '%s' "$1"; }
find_disks() { :; }
choose_disk() { :; }
choose_mode() { :; }
confirm_update() { printf '%s\n' update; }
confirm_erase() { printf '%s\n' erase; }
get_image() { printf '%s\n' "${GIZLET_IMAGE_URL:-$image}"; }
write_card() { printf '%s\n' write; }

for mode in update erase; do
    actual=$(main --image "${stub}/gizlet-os-r36s.img" "--${mode}")
    expected=$(printf '%s\n' "$mode" "${stub}/gizlet-os-r36s.img" write)
    [ "$actual" = "$expected" ] || { echo "FAIL: unexpected local ${mode} flow: ${actual}"; exit 1; }
    release_image=https://example.invalid/gizlet-os-r36s.img.gz
    actual=$(GIZLET_IMAGE_URL="$release_image" main "--${mode}")
    expected=$(printf '%s\n' "$mode" "$release_image" write)
    [ "$actual" = "$expected" ] || { echo "FAIL: unexpected online ${mode} flow: ${actual}"; exit 1; }
    echo "ok: local and online ${mode} reach the writer without a display prompt"
done

eval "$(sed -n '/^boot_supports_nv3051d()/,/^}/p' "${script}")"
boot_dir="${stub}/boot"
mkdir "${boot_dir}"
printf 'legacy tree' > "${boot_dir}/r36s.dtb"
printf 'V30 NV3051D overlay' > "${boot_dir}/r36s-v30-panel4.dtbo"
if boot_supports_nv3051d "${boot_dir}"; then
    echo "FAIL: accepted a legacy Panel 0 card"
    exit 1
fi
cp "${boot_dir}/r36s-v30-panel4.dtbo" "${boot_dir}/mipi-panel.dtbo"
boot_supports_nv3051d "${boot_dir}"
printf '\0I seq=ff52\0' > "${boot_dir}/r36s.dtb"
printf 'stale overlay' > "${boot_dir}/mipi-panel.dtbo"
boot_supports_nv3051d "${boot_dir}"
echo "ok: legacy and built-in NV3051D configurations pass the update guard"
