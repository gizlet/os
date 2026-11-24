#!/bin/bash
# Checks the stage runner's input handling with a fake kernel tree: ordered
# patches, replaced device trees, built-in sources, Kconfig checks, and
# destinations it must refuse.
set -euo pipefail
runner=$1
export SOURCE_DATE_EPOCH=1704067200 LC_ALL=C
root=$(mktemp -d "${TEST_TMPDIR:-/tmp}/runner.XXXXXX")
cd "$root"
tab=$'\t'

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

# params FILE ENTRY...: one line per entry, its fields separated by |.
params() {
    local file=$1
    shift
    printf '%s\n' "$@" | tr '|' '\t' > "$file"
}

# expect_failure MESSAGE STAGE PARAMS
expect_failure() {
    local out
    if out=$(sh "$runner" "$2" "$3" 2>&1); then
        fail "$3 succeeded"
    fi
    [[ $out == *"$1"* ]] || fail "$3: expected '$1', got: $out"
}

# A kernel tree whose Image is its version file followed by drivers/Makefile.
mkdir -p src/upstream/arch/arm64/boot/dts/vendor
printf '%s\n' \
    'olddefconfig:' "${tab}@sed -i /CONFIG_DROPPED/d .config" \
    'Image:' "${tab}@cat version drivers/Makefile > arch/arm64/boot/Image" \
    'vendor/console.dtb:' "${tab}@cp arch/arm64/boot/dts/vendor/console.dts arch/arm64/boot/dts/vendor/console.dtb" \
    > src/upstream/Makefile
printf 'upstream' > src/upstream/arch/arm64/boot/dts/vendor/console.dts
printf 'base\n' > src/upstream/version
ln -s /tmp src/upstream/outside
tar -cf source.tar -C src upstream
for step in first:base:intermediate second:intermediate:final; do
    IFS=: read -r name before after <<< "$step"
    printf -- '--- a/version\n+++ b/version\n@@ -1 +1 @@\n-%s\n+%s\n' "$before" "$after" > "$name"
done
printf 'CONFIG_TEST=y\n' > config
printf 'CONFIG_TEST=y\nCONFIG_DROPPED=y\n' > dropped
printf 'consumer board' > consumer.dts
printf 'int x;\n' > driver.c
base=("archive|source.tar" "patch|first" "patch|second" "patch_fuzz|0" "build_user|builder" "build_host|builder")

params kernel.params "${base[@]}" "config|config" "dts|consumer.dts|vendor/console.dts" \
    "dtb_target|vendor/console.dtb" "builtin|driver.c|drivers/x.c" "output|$root/out/Image" "output|$root/out/console.dtb"
sh "$runner" Kernel kernel.params
[[ $(cat out/Image) == $'final\nobj-y += x.o' ]] || fail "Image: $(cat out/Image)"
[[ $(cat out/console.dtb) == "consumer board" ]] || fail "dtb: $(cat out/console.dtb)"

params dropped.params "${base[@]}" "config|dropped" "dtb_target|vendor/console.dtb" "output|$root/out/a" "output|$root/out/b"
expect_failure "Unresolved Kconfig settings: CONFIG_DROPPED=y" Kernel dropped.params

params escape.params "${base[@]}" "config|config" "builtin|driver.c|outside/payload.c" "dtb_target|vendor/console.dtb" \
    "output|$root/out/a" "output|$root/out/b"
expect_failure "escapes staging tree" Kernel escape.params

params header.params "${base[@]}" "config|config" "builtin|driver.c|drivers/x.txt" "dtb_target|vendor/console.dtb" \
    "output|$root/out/a" "output|$root/out/b"
expect_failure "Built-in source must be" Kernel header.params

# Card inputs: refused before genimage runs.
printf 'payload' > payload
for invalid in "" "/bin/app" "usr/../bin/app" "usr//app" "./app"; do
    params invalid.params "layout|layout.cfg" "layout_image|card.img" "input|payload|$invalid" "output|$root/out/card.img"
    expect_failure "Invalid relative path" SDImage invalid.params
done
params duplicate.params "layout|layout.cfg" "layout_image|card.img" "input|payload|app" "input|payload|app" "output|$root/out/card.img"
expect_failure "Duplicate or existing destination: app" SDImage duplicate.params
[[ $(cat payload) == payload ]] || fail "the input changed"
echo "runner stages check their inputs"
