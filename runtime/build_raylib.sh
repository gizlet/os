#!/bin/sh
# Builds raylib as a shared library for Gizlet runtimes in the build VM (see
# defs.bzl). The sources come from the vendored raylib-sys crate under
# /in/vendor, so the launcher's bindings match the library.
#
# It uses raylib's DRM platform with OpenGL ES 3.0 through EGL and GBM.
# SUPPORT_DRM_CACHE presents frames by flipping scanout buffers.
set -eu
out=$1

src=$(echo /in/vendor/raylib-sys-*/raylib)
[ -d "$src" ] || { echo "raylib sources missing" >&2; exit 1; }
cp -r "$src" /tmp/raylib
patch --batch --fuzz=0 -d /tmp/raylib -p1 < /in/raylib-es3.patch
# raylib is C only, but its project() also asks for a C++ compiler, which the build VM lacks.
sed -i 's/^project(raylib)$/project(raylib C)/' /tmp/raylib/CMakeLists.txt
grep -q '^project(raylib C)$' /tmp/raylib/CMakeLists.txt
cmake -S /tmp/raylib -B /tmp/build -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_COMPILER=gcc \
    -DPLATFORM=DRM -DOPENGL_VERSION='ES 3.0' -DBUILD_SHARED_LIBS=ON -DBUILD_EXAMPLES=OFF \
    -DCMAKE_C_FLAGS='-DSUPPORT_DRM_CACHE -fdebug-prefix-map=/tmp/raylib/=raylib/' \
    -DCMAKE_SHARED_LINKER_FLAGS='-Wl,--as-needed' > /tmp/cmake.log ||
    { tail -n 40 /tmp/cmake.log >&2; exit 1; }
cmake --build /tmp/build --parallel 4 > /tmp/build.log || { tail -n 40 /tmp/build.log >&2; exit 1; }

soname=${out##*/}.600
library=/tmp/build/raylib/$soname
[ -f "$library" ] || { echo "$soname missing" >&2; exit 1; }
dynamic=$(readelf -dW "$library")
actual_soname=$(printf '%s\n' "$dynamic" | sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p')
[ "$actual_soname" = "$soname" ] || { echo "expected SONAME $soname, got $actual_soname" >&2; exit 1; }
needed=$(printf '%s\n' "$dynamic" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' | sort | tr '\n' ' ')
echo "libraylib needs: $needed"
case " $needed" in *' libEGL.so.1 '*) ;; *) echo "libraylib does not use EGL" >&2; exit 1 ;; esac
strip --strip-unneeded -o "/out/$out" "$library"
