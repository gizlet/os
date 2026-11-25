#!/bin/sh
# Runs in the build VM: install the locked Debian packages from /in/debs with
# mmdebstrap, add the files under /in/files and /in/executables and the symbolic
# links, enable, disable and mask units, and write a read-only SquashFS to /out.
# Usage: build_base.sh OUTPUT SUITE "DIRECTORIES" "ENABLE UNITS" "DISABLE UNITS" "MASK UNITS" "PATH:TARGET ..."
set -eu
out=$1 suite=$2 directories=$3 enable=$4 disable=$5 mask=$6 symlinks=$7
root=/tmp/root
repo=/tmp/repo

# A flat local repository holding exactly the locked packages.
mkdir -p "$repo"
cp /in/debs/*.deb "$repo/"
(cd "$repo" && apt-ftparchive packages . > Packages)
packages=$(cd /in/debs && ls -- *.deb | sed 's/\.deb$//' | paste -sd, -)

# Documentation is dropped except the copyright files, which stay as notices.
mmdebstrap --mode=root --variant=custom --include="$packages" \
    --aptopt='Acquire::Languages "none"' \
    --dpkgopt='path-exclude=/usr/share/doc/*' --dpkgopt='path-include=/usr/share/doc/*/copyright' \
    --dpkgopt='path-exclude=/usr/share/man/*' --dpkgopt='path-exclude=/usr/share/info/*' \
    --dpkgopt='path-exclude=/usr/share/locale/*' --dpkgopt='path-include=/usr/share/locale/locale.alias' \
    --customize-hook='rm -f "$1/etc/apt/sources.list"' \
    "$suite" "$root" "deb [trusted=yes] copy://$repo ./"

install_tree() {
    [ -d "$1" ] || return 0
    (cd "$1" && find . -type f) | while read -r path; do
        install -D -m "$2" "$1/$path" "$root/${path#./}"
    done
}
for directory in $directories; do mkdir -p "$root/$directory"; done
install_tree /in/files 0644
install_tree /in/executables 0755
for link in $symlinks; do
    path=${link%%:*} target=${link#*:}
    mkdir -p "$root/$(dirname "$path")"
    rm -f "$root/$path"
    ln -s "$target" "$root/$path"
done

[ -z "$enable" ] || chroot "$root" systemctl enable $enable
[ -z "$disable" ] || chroot "$root" systemctl disable $disable
[ -z "$mask" ] || chroot "$root" systemctl mask $mask

# The kernel mounts devtmpfs on /dev before init. mmdebstrap's device nodes are
# left out: they need mknod, which the namespace backend cannot do, and the
# image must be the same from both backends.
rm -rf "$root/dev"
mkdir -m 0755 "$root/dev"

# Per-card identity is created at boot.
rm -f "$root"/etc/ssh/ssh_host_*
: > "$root/etc/machine-id"
rm -f "$root/var/lib/dbus/machine-id"
rm -rf "$root"/var/cache/apt/* "$root"/var/lib/apt/lists/* "$root"/var/log/*
mkdir -p "$root/var/lib/apt/lists/partial" "$root/var/cache/apt/archives/partial"

epoch=$SOURCE_DATE_EPOCH
unset SOURCE_DATE_EPOCH
mkdir -p "$(dirname "/out/$out")"
mksquashfs "$root" "/out/$out" -comp zstd -noappend -all-time "$epoch" -mkfs-time "$epoch" -no-progress -quiet
