#!/bin/sh
# Builds the launcher for Gizlet OS in the build VM (see defs.bzl), with inputs
# under /in: launcher/ (this crate), vendor/ (its crates), rust/ (the
# toolchain) and raylib/libraylib.so, Gizlet runtime 1's raylib for DRM with
# OpenGL ES 2. The launcher links that library and runs against the runtime
# through gizlet-run. The CLI uses the base system's libraries.
set -eu
launcher_out=$1
cli_out=$2

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

export GIZLET_RAYLIB_LIB=/in/raylib
[ -f "$GIZLET_RAYLIB_LIB/libraylib.so" ] || { echo "libraylib.so missing" >&2; exit 1; }

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
launcher=/tmp/target/release/gizlet-launcher
cli=/tmp/target/release/gizlet-cli
soname=$(readelf -dW /in/raylib/libraylib.so | sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p')
readelf -dW "$launcher" | grep -qF "Shared library: [$soname]" || { echo "the launcher does not link $soname" >&2; exit 1; }
cp "$launcher" "/out/$launcher_out"
cp "$cli" "/out/$cli_out"
