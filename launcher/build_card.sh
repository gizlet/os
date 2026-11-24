#!/bin/sh
# Builds the launcher for Gizlet OS in the build VM (see defs.bzl), with inputs
# under /in: launcher/ (this crate), vendor/ (its crates), rust/ (the toolchain).
#
# raylib comes from the vendored raylib-sys sources: its DRM platform with the
# software renderer, patched for the panel's pixel format and linked
# statically. The card needs no EGL or Mesa, only libdrm.
set -eu
out=$1

# Rust: the three components copied into one tree (rustc finds its sysroot
# from its own path). Each component has its own manifest.in; the last wins.
rust=/tmp/rust
mkdir -p "$rust"
for component in rustc rust-std cargo; do
    cp -RL "/in/rust/$component/." "$rust/"
done
export PATH="$rust/bin:$PATH" CARGO_HOME=/tmp/cargo-home CARGO_TARGET_DIR=/tmp/target
export CC=gcc CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=gcc
# bindgen reads raylib.h with libclang.
export LIBCLANG_PATH=/usr/lib/llvm-14/lib

raylib_src=$(echo /in/vendor/raylib-sys-*/raylib)
[ -d "$raylib_src" ] || { echo "raylib sources missing" >&2; exit 1; }
cp -r "$raylib_src" /tmp/raylib
patch -d /tmp/raylib -p1 --quiet < /in/launcher/raylib-drm-scanout.patch
# raylib is C only, but its project() also asks for a C++ compiler, which the build VM lacks.
sed -i 's/^project(raylib)$/project(raylib C)/' /tmp/raylib/CMakeLists.txt
grep -q '^project(raylib C)$' /tmp/raylib/CMakeLists.txt
cmake -S /tmp/raylib -B /tmp/raylib-build -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_COMPILER=gcc \
    -DPLATFORM=DRM -DOPENGL_VERSION=Software -DBUILD_SHARED_LIBS=OFF \
    -DBUILD_EXAMPLES=OFF -DUSE_AUDIO=OFF -DCMAKE_C_FLAGS="-fdebug-prefix-map=/tmp/raylib/=raylib/" > /tmp/cmake.log
cmake --build /tmp/raylib-build --parallel 4 > /tmp/raylib-build.log
export GIZLET_RAYLIB_LIB=/tmp/raylib-build/raylib
[ -f "$GIZLET_RAYLIB_LIB/libraylib.a" ] || { echo "libraylib.a missing" >&2; exit 1; }

# The crate, with Cargo reading crates from the vendor directory only.
cp -r /in/launcher /tmp/src
mkdir -p /tmp/src/.cargo
cat > /tmp/src/.cargo/config.toml <<EOF
[source.crates-io]
replace-with = "vendored"

[source.vendored]
directory = "/in/vendor"

[net]
offline = true
EOF
cd /tmp/src
export RUSTFLAGS="--remap-path-prefix=/tmp/src=. --remap-path-prefix=/in/vendor=vendor --remap-path-prefix=$CARGO_HOME=cargo"
cargo build --offline --locked --release --features gizlet-os
cp /tmp/target/release/gizlet-launcher "/out/$out"
