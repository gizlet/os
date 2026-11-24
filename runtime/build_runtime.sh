#!/bin/sh
# Builds a Gizlet runtime in the build VM (see defs.bzl): the locked trixie
# libraries in /in/debs, Debian's own builds, and the libraries built here in
# /in/libraries (raylib), laid out as
#
#   lib/                 glibc with its loader, libglvnd, Mesa, libdrm, raylib and their dependencies
#   lib/dri, lib/gbm     Mesa's drivers and GBM backend
#   share/glvnd/         the EGL vendor file naming Mesa
#   share/doc/PACKAGE/   each package's copyright
#   bin/gizlet-run       runs a program against the runtime
#   bin/gizlet-gl-info   prints the OpenGL ES renderer and checks a render
#
# and written as a read-only SquashFS image, compressed like the base.
set -eu
out=$1
unpack=/tmp/unpack
runtime=/tmp/runtime
mkdir -p "$unpack" "$runtime/bin" "$runtime/lib" "$runtime/share/doc" "$runtime/share/glvnd/egl_vendor.d"
for deb in /in/debs/*.deb; do
    dpkg-deb -x "$deb" "$unpack"
done
for dir in "$unpack/usr/lib/aarch64-linux-gnu" "$unpack/lib/aarch64-linux-gnu"; do
    if [ -d "$dir" ]; then cp -a "$dir/." "$runtime/lib/"; fi
done
rm -rf "$runtime/lib/pkgconfig"
cp "$unpack"/usr/share/glvnd/egl_vendor.d/*.json "$runtime/share/glvnd/egl_vendor.d/"
for doc in "$unpack"/usr/share/doc/*; do
    if [ -f "$doc/copyright" ]; then
        mkdir -p "$runtime/share/doc/${doc##*/}"
        cp "$doc/copyright" "$runtime/share/doc/${doc##*/}/"
    fi
done

# Libraries built for the runtime (raylib), under their SONAMEs. Everything
# they need must be in the runtime.
for library in /in/libraries/*; do
    [ -f "$library" ] || continue
    soname=$(readelf -dW "$library" | sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p')
    [ -n "$soname" ] || { echo "${library##*/} has no SONAME" >&2; exit 1; }
    install -m 0644 "$library" "$runtime/lib/$soname"
    for needed in $(readelf -dW "$library" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p'); do
        [ -e "$runtime/lib/$needed" ] || { echo "runtime lacks $needed for $soname" >&2; exit 1; }
    done
done

for required in ld-linux-aarch64.so.1 libc.so.6 libEGL.so.1 libEGL_mesa.so.0 libGLESv2.so.2 libgbm.so.1 libdrm.so.2; do
    [ -e "$runtime/lib/$required" ] || { echo "runtime lacks $required" >&2; exit 1; }
done
ls "$runtime"/lib/dri/*panfrost* > /dev/null || { echo "runtime lacks Mesa's Panfrost driver" >&2; exit 1; }
# kmsro joins a display-only KMS device (Rockchip's VOP) with the render-only GPU.
[ -e "$runtime/lib/dri/rockchip_dri.so" ] || { echo "runtime lacks Mesa's rockchip (kmsro) driver" >&2; exit 1; }
# Links must stay inside the image.
absolute=$(find "$runtime" -type l -lname '/*')
[ -z "$absolute" ] || { echo "absolute links: $absolute" >&2; exit 1; }

install -m 0755 /in/gizlet-run "$runtime/bin/gizlet-run"
gcc -O2 -Wall -Wextra -Werror -ffile-prefix-map=/in/= -s -o "$runtime/bin/gizlet-gl-info" /in/gl-info.c

epoch=$SOURCE_DATE_EPOCH
find "$runtime" -exec touch -h -d "@$epoch" {} +
# mksquashfs refuses SOURCE_DATE_EPOCH together with explicit times.
unset SOURCE_DATE_EPOCH
mksquashfs "$runtime" "/out/$out" -comp zstd -all-root -noappend -no-xattrs -no-progress -quiet \
    -all-time "$epoch" -mkfs-time "$epoch"
