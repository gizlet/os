#!/bin/sh
# Builds FFmpeg's libraries for Gizlet runtimes in the build VM (see defs.bzl):
# the LGPL decoders and encoders that live TV, radio and common media use, with
# no external libraries. They keep FFmpeg's SONAMEs and symbol versions, so
# Debian's GStreamer libav plugin runs on them. The plugin's own release of
# FFmpeg, 7.1.5, links dozens of codec libraries the runtime does not carry.
set -eu
out=$1
version=7.1.5

mkdir -p "/out/$out"
tar -xJf "/in/ffmpeg-$version.tar.xz" -C /tmp
cd "/tmp/ffmpeg-$version"
./configure --prefix=/tmp/ffmpeg --enable-shared --disable-static \
    --disable-programs --disable-doc --disable-debug --disable-network \
    --disable-autodetect --disable-everything \
    --disable-avdevice --disable-swscale --disable-swresample --enable-pthreads \
    --enable-decoder=h264,hevc,vp8,vp9,mpeg4,mpeg2video,aac,aac_latm,mp3,mp3float,mp2,mp2float,opus,vorbis,flac,alac,pcm_s16le,pcm_s16be,pcm_s24le,pcm_f32le \
    --enable-encoder=aac,flac,pcm_s16le \
    --enable-parser=h264,hevc,vp8,vp9,mpeg4video,mpegvideo,aac,aac_latm,mpegaudio,opus,vorbis,flac \
    --extra-cflags="-ffile-prefix-map=/tmp/ffmpeg-$version/=ffmpeg/" > /tmp/configure.log ||
    { tail -n 40 /tmp/configure.log >&2; exit 1; }
make -j4 install > /tmp/build.log 2>&1 || { tail -n 40 /tmp/build.log >&2; exit 1; }

for library in libavcodec.so.61 libavformat.so.61 libavfilter.so.10 libavutil.so.59; do
    soname=$(readelf -dW "/tmp/ffmpeg/lib/$library" | sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p')
    [ "$soname" = "$library" ] || { echo "expected SONAME $library, got $soname" >&2; exit 1; }
    # Debian's plugin binds to the versioned symbols, such as LIBAVCODEC_61.
    name=$(echo "${library%%.so*}" | tr a-z A-Z)
    readelf -VW "/tmp/ffmpeg/lib/$library" | grep -q "${name}_${library##*.so.}" ||
        { echo "$library lacks ${name}_${library##*.so.} symbol versions" >&2; exit 1; }
    strip --strip-unneeded -o "/out/$out/$library" "/tmp/ffmpeg/lib/$library"
done
{
    echo "FFmpeg $version, built unmodified from https://ffmpeg.org/releases/ffmpeg-$version.tar.xz"
    echo "by runtime/build_ffmpeg.sh in https://github.com/gizlet/os, configured LGPL-only:"
    echo
    sed -n 's/^#define FFMPEG_CONFIGURATION "\(.*\)"$/\1/p' config.h
    sed -n 's/^#define FFMPEG_LICENSE "\(.*\)"$/License: \1/p' config.h
    echo
    cat COPYING.LGPLv2.1
} > "/out/$out/ffmpeg.copyright"
