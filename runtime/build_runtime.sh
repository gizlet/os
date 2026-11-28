#!/bin/sh
# Builds a Gizlet runtime in the build VM (see defs.bzl): the locked trixie
# libraries in /in/debs, Debian's own builds, chosen GStreamer plugins and
# modules from the packages in /in/standalone, and the libraries built here in
# /in/libraries (raylib and FFmpeg), laid out as
#
#   lib/                 glibc with its loader, libglvnd, Mesa, libdrm, ALSA's libasound, raylib,
#                        GStreamer, FFmpeg, libsoup and their dependencies
#   lib/dri, lib/gbm     Mesa's drivers and GBM backend
#   lib/gstreamer-1.0/   GStreamer's plugins
#   lib/gio/modules/     GIO's TLS module, GnuTLS
#   share/glvnd/         the EGL vendor file naming Mesa
#   share/alsa/          ALSA's configuration, for libasound
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
# Gizlet OS's base provides OpenSSL 3. krb5 (for libsoup) and the HLS plugin
# load it from there, so the runtime does not carry its own.
rm -f "$runtime"/lib/libssl.so* "$runtime"/lib/libcrypto.so*
rm -rf "$runtime/lib/ossl-modules" "$runtime/lib/engines-3"
cp "$unpack"/usr/share/glvnd/egl_vendor.d/*.json "$runtime/share/glvnd/egl_vendor.d/"
# ALSA's configuration, which gizlet-run points libasound at.
cp -a "$unpack/usr/share/alsa" "$runtime/share/alsa"

# Packages locked without their dependencies (see packages.json) give only
# these files. GStreamer's plugin packages depend on the libraries of every
# plugin in them; the runtime carries what these plugins need.
standalone=/tmp/standalone
multiarch=usr/lib/aarch64-linux-gnu
mkdir -p "$standalone" "$runtime/lib/gstreamer-1.0" "$runtime/lib/gio/modules"
for deb in /in/standalone/*.deb; do
    dpkg-deb -x "$deb" "$standalone"
done
# Playback of files and HTTP(S), HLS and DASH streams with MPEG-TS, MP4, Matroska
# and Ogg, decoding through FFmpeg (libav), and encoding to Opus, Vorbis, FLAC,
# MP3, AAC (libav), WebP and H.264 (OpenH264). Sound goes to ALSA. hls and dash
# are the demuxers uridecodebin plugs, which flutter-pi's video player uses;
# adaptivedemux2's serve playbin3 and urisourcebin.
plugins="playback typefindfunctions app audioconvert audioresample audiorate volume
    videoconvertscale videorate deinterlace rawparse pbtypes ogg opus vorbis alsa
    autodetect soup adaptivedemux2 hls dash isomp4 matroska audioparsers icydemux id3demux
    flac lame wavenc wavparse mpegtsdemux mpegtsmux videoparsersbad webp openh264 libav v4l2codecs"
for plugin in $plugins; do
    cp "$standalone/$multiarch/gstreamer-1.0/libgst$plugin.so" "$runtime/lib/gstreamer-1.0/"
done
for library in libgstadaptivedemux-1.0.so.0 libgsturidownloader-1.0.so.0 libgstisoff-1.0.so.0 libgstcodecparsers-1.0.so.0 libgstcodecs-1.0.so.0 \
    libgstmpegts-1.0.so.0 libsoup-3.0.so.0; do
    cp -a "$standalone/$multiarch/$library" "$(readlink -f "$standalone/$multiarch/$library")" "$runtime/lib/"
done
cp "$standalone/$multiarch/gio/modules/libgiognutls.so" "$runtime/lib/gio/modules/"

for doc in "$unpack"/usr/share/doc/* "$standalone"/usr/share/doc/*; do
    if [ -f "$doc/copyright" ]; then
        mkdir -p "$runtime/share/doc/${doc##*/}"
        cp "$doc/copyright" "$runtime/share/doc/${doc##*/}/"
    fi
done
rm -rf "$runtime/share/doc/libssl3t64" "$runtime/share/doc/openssl-provider-legacy"
for notice in /in/notices/*; do
    [ -f "$notice" ] || continue
    mkdir -p "$runtime/share/doc/${notice##*/}"
    cp "$notice" "$runtime/share/doc/${notice##*/}/copyright"
done

# Libraries built for the runtime (raylib and FFmpeg), under their SONAMEs.
for library in /in/libraries/*; do
    [ -f "$library" ] || continue
    soname=$(readelf -dW "$library" | sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p')
    [ -n "$soname" ] || { echo "${library##*/} has no SONAME" >&2; exit 1; }
    install -m 0644 "$library" "$runtime/lib/$soname"
done

# Every library and plugin finds what it needs in the runtime, beside itself
# (glibc's gconv modules) or, for OpenSSL, in the base.
missing=$(find "$runtime/lib" -type f -name '*.so*' | sort | while read -r file; do
    readelf -dW "$file" 2>/dev/null | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' | while read -r needed; do
        case "$needed" in (libssl.so.3|libcrypto.so.3) continue ;; esac
        [ -e "$runtime/lib/$needed" ] || [ -e "${file%/*}/$needed" ] || echo "${file#"$runtime"/lib/} needs $needed"
    done
done)
[ -z "$missing" ] || { echo "runtime lacks libraries:" >&2; echo "$missing" >&2; exit 1; }

for required in ld-linux-aarch64.so.1 libc.so.6 libasound.so.2 libEGL.so.1 libEGL_mesa.so.0 libGLESv2.so.2 libgbm.so.1 libdrm.so.2; do
    [ -e "$runtime/lib/$required" ] || { echo "runtime lacks $required" >&2; exit 1; }
done
[ -f "$runtime/share/alsa/alsa.conf" ] || { echo "runtime lacks ALSA's alsa.conf" >&2; exit 1; }
ls "$runtime"/lib/dri/*panfrost* > /dev/null || { echo "runtime lacks Mesa's Panfrost driver" >&2; exit 1; }
# kmsro joins a display-only KMS device (Rockchip's VOP) with the render-only GPU.
[ -e "$runtime/lib/dri/rockchip_dri.so" ] || { echo "runtime lacks Mesa's rockchip (kmsro) driver" >&2; exit 1; }
# Links must stay inside the image.
absolute=$(find "$runtime" -type l -lname '/*')
[ -z "$absolute" ] || { echo "absolute links: $absolute" >&2; exit 1; }

install -m 0755 /in/gizlet-run "$runtime/bin/gizlet-run"
gcc -O2 -Wall -Wextra -Werror -ffile-prefix-map=/in/= -s -o "$runtime/bin/gizlet-gl-info" /in/gl-info.c

if [ -n "${GIZLET_RELEASE_VERSION:-}" ]; then
    printf 'v%s\n' "$GIZLET_RELEASE_VERSION" > "$runtime/release-tag"
fi

epoch=$SOURCE_DATE_EPOCH
find "$runtime" -exec touch -h -d "@$epoch" {} +
# mksquashfs refuses SOURCE_DATE_EPOCH together with explicit times.
unset SOURCE_DATE_EPOCH
mksquashfs "$runtime" "/out/$out" -comp zstd -all-root -noappend -no-xattrs -no-progress -quiet \
    -all-time "$epoch" -mkfs-time "$epoch"
