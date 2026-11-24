#!/bin/sh
# build.sh BINARY OUT FEATURES: cargo_binary's build in the build VM (see
# defs.bzl). Inputs under /in: crate/ (the crate), vendor/ (its crates),
# rust/ (the toolchain) and, for programs that draw, raylib/libraylib.so,
# Gizlet runtime 1's raylib for DRM with OpenGL ES 2, which the program then
# links. FEATURES is a comma-separated list, possibly empty.
set -eu
binary=$1
out=$2
features=$3

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
raylib=
if [ -f /in/raylib/libraylib.so ]; then
    raylib=/in/raylib/libraylib.so
    export GIZLET_RAYLIB_LIB=/in/raylib
fi

# The crate, with Cargo reading crates from the vendor directory only.
cp -r /in/crate /tmp/src
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
cargo build --offline --locked --release --bin "$binary" ${features:+--features "$features"}
built=/tmp/target/release/$binary
if [ -n "$raylib" ]; then
    soname=$(readelf -dW "$raylib" | sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p')
    readelf -dW "$built" | grep -qF "Shared library: [$soname]" || { echo "$binary does not link $soname" >&2; exit 1; }
fi
cp "$built" "/out/$out"
