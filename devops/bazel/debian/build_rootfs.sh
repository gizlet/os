#!/bin/busybox sh
# Runs in the bootstrap microVM: unpack the locked packages on a case-sensitive
# tmpfs and write a read-only SquashFS root image for the build VM.
# Usage: build_rootfs.sh OUTPUT_NAME
set -eu
bb=/bin/busybox

root=/tmp/root
$bb mkdir -p "$root" /tmp/unpack
for deb in /in/debs/*.deb; do
    cd /tmp/unpack
    $bb ar x "$deb"
    case $(echo data.tar*) in
    data.tar.xz) $bb xz -dc data.tar.xz | $bb tar -x -C "$root" ;;
    data.tar.gz) $bb gzip -dc data.tar.gz | $bb tar -x -C "$root" ;;
    data.tar) $bb tar -x -C "$root" -f data.tar ;;
    *) echo "unsupported payload in $deb" >&2; exit 1 ;;
    esac
    $bb rm -f ./*
done
cd /

# Maintainer scripts do not run. Apply the few results the build tools rely on.
$bb cp "$root/usr/share/base-passwd/passwd.master" "$root/etc/passwd"
$bb cp "$root/usr/share/base-passwd/group.master" "$root/etc/group"
for link in bin/sh:dash usr/bin/awk:mawk usr/bin/cc:gcc usr/bin/c++:g++ usr/bin/yacc:bison.yacc usr/bin/lex:flex; do
    path=${link%%:*} target=${link#*:}
    [ -e "$root/$path" ] || [ -L "$root/$path" ] || $bb ln -s "$target" "$root/$path"
done
for directory in in out tmp run proc sys dev work; do
    $bb mkdir -p "$root/$directory"
done
$bb chmod 1777 "$root/tmp"

# SquashFS stores one timestamp per inode, so the image does not depend on the
# guest clock. mksquashfs runs from the unpacked tree with its own libraries.
epoch=$SOURCE_DATE_EPOCH
unset SOURCE_DATE_EPOCH
"$root/lib/ld-linux-aarch64.so.1" --library-path "$root/lib/aarch64-linux-gnu:$root/usr/lib/aarch64-linux-gnu" \
    "$root/usr/bin/mksquashfs" "$root" "/out/$1" -comp zstd -noappend -no-xattrs -all-root \
    -all-time "$epoch" -mkfs-time "$epoch" -no-progress -quiet
