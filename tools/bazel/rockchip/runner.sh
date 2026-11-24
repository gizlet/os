#!/bin/sh
# ARM64 Linux, legacy U-Boot and genimage stages, run inside the build VM.
#
# Usage: runner.sh Kernel|LegacyUBoot|SDImage PARAMS
#
# The current directory holds the inputs at their execroot paths. PARAMS has
# one entry per line, its fields separated by tabs (see defs.bzl); `output`
# lines name the files to write, in the order the stage produces them. The
# stage builds in a private directory on the guest's case-sensitive tmpfs.
#
# Derived from GizOS tools/bazel/native/rockchip at 0ac3bfdc2bafa78bfc89965931b5ce22faeff795 (Apache-2.0, see LICENSE).
set -eu
umask 022
tab=$(printf '\t')
nl='
'
stage=$1
params=$(realpath "$2")
in=$PWD
jobs=$(nproc)
[ "$jobs" -le 8 ] || jobs=8

die() {
    printf 'runner: %s\n' "$*" >&2
    exit 1
}

# param KEY: the value on KEY's first line.
param() {
    param_value=$(awk -F "$tab" -v key="$1" '$1 == key { print $2; found = 1; exit } END { exit !found }' "$params") ||
        die "missing parameter $1"
    printf '%s' "$param_value"
}

# entries KEY: the fields after KEY on each of its lines.
entries() {
    sed -n "s/^$1$tab//p" "$params"
}

# Destinations are literal, nonempty paths underneath a staging directory.
check_relative() {
    case "/$1/" in
        *//* | */./* | */../*) die "Invalid relative path: '$1'" ;;
    esac
}

# child ROOT NAME: ROOT/NAME, refused when it leaves ROOT, also through a
# symlink from a source archive.
child() {
    check_relative "$2"
    child_root=$(realpath "$1")
    child_target=$(realpath -m "$1/$2")
    case "$child_target/" in
        "$child_root"/*) ;;
        *) die "Destination escapes staging tree: $2" ;;
    esac
    printf '%s' "$1/$2"
}

# install_files ROOT MODE replace|new: copies the input SOURCE<TAB>DESTINATION
# lines from standard input to ROOT/DESTINATION.
install_files() {
    install_seen=$tab
    while IFS=$tab read -r install_src install_dst; do
        install_target=$(child "$1" "$install_dst")
        case "$install_seen" in *"$tab$install_dst$tab"*) die "Duplicate or existing destination: $install_dst" ;; esac
        if [ -L "$install_target" ] || [ -d "$install_target" ] || { [ -e "$install_target" ] && [ "$3" != replace ]; }; then
            die "Duplicate or existing destination: $install_dst"
        fi
        install_seen=$install_seen$install_dst$tab
        mkdir -p "$(dirname "$install_target")"
        cp "$in/$install_src" "$install_target"
        chmod "$2" "$install_target"
    done
}

unpack() {
    tar --extract --strip-components=1 --file "$in/$(param archive)"
    fuzz=$(param patch_fuzz)
    entries patch | while IFS= read -r patch_file; do
        patch --batch --fuzz="$fuzz" -p1 < "$in/$patch_file"
    done
}

configure_make() {
    export ARCH="$1" CROSS_COMPILE=aarch64-linux-gnu- LOCALVERSION='' KBUILD_BUILD_VERSION=1
    KBUILD_BUILD_USER=$(param build_user)
    KBUILD_BUILD_HOST=$(param build_host)
    KBUILD_BUILD_TIMESTAMP=$(date -u -d "@$SOURCE_DATE_EPOCH" '+%a %b %d %T UTC %Y')
    export KBUILD_BUILD_USER KBUILD_BUILD_HOST KBUILD_BUILD_TIMESTAMP
}

linux_kernel() {
    unpack
    configure_make arm64
    config=$in/$(param config)
    cp "$config" .config
    make -s olddefconfig
    # Every setting of the fragment must survive olddefconfig.
    missing=$(grep '^CONFIG_.*=' "$config" | grep -vxF -f .config | LC_ALL=C sort -u |
        awk 'NR > 1 { printf ", " } { printf "%s", $0 }')
    [ -z "$missing" ] || die "Unresolved Kconfig settings: $missing"
    # Out-of-tree drivers become built-in objects of their directory, as ROCKNIX adds them.
    # Headers are installed beside them.
    entries builtin | while IFS=$tab read -r src dst; do
        case "$dst" in
            *.c | *.h | firmware/*) ;;
            *) die "Built-in source must be a C file, a header or under firmware/: '$dst'" ;;
        esac
        printf '%s\t%s\n' "$src" "$dst" | install_files . 0644 new
        case "$dst" in
            *.c)
                dir=$(dirname "$dst")
                if [ "$dir" = . ]; then makefile=Makefile; else makefile=$dir/Makefile; fi
                makefile=$(child . "$makefile")
                base=$(basename "$dst")
                printf 'obj-y += %s.o\n' "${base%.*}" >> "$makefile"
                ;;
        esac
    done
    dt_dir=arch/arm64/boot/dts
    entries dts | install_files "$dt_dir" 0644 replace
    targets=$(entries dtb_target)
    [ -n "$targets" ] || die "missing parameter dtb_target"
    results=arch/arm64/boot/Image
    set --
    old_ifs=$IFS
    IFS=$nl
    for target in $targets; do
        IFS=$old_ifs
        dtb=$(child "$dt_dir" "$target")
        case "$target" in
            -*) die "Expected a Kbuild DTB or overlay target: '$target'" ;;
            *.dtb)
                # Keep the label symbols, so that boot-time overlays can refer to them.
                base=$(basename "$dtb")
                makefile=$(child "$(dirname "$dtb")" Makefile)
                printf 'DTC_FLAGS_%s += -@\n' "${base%.dtb}" >> "$makefile"
                ;;
            *.dtbo) ;;
            *) die "Expected a Kbuild DTB or overlay target: '$target'" ;;
        esac
        set -- "$@" "$target"
        results=$results$nl$dtb
    done
    IFS=$old_ifs
    make -s -j"$jobs" Image "$@"
}

legacy_uboot() {
    soc=$(param mkimage_soc)
    address=$(param load_address)
    title=$(param script_name)
    if ! printf '%s' "$soc" | grep -Eqx '[A-Za-z0-9_-]+' || ! printf '%s' "$address" | grep -Eqx '0[xX][0-9a-fA-F]{1,8}'; then
        die "Invalid mkimage SoC token or 32-bit U-Boot load address"
    fi
    non_ascii=$(printf '%s' "$title" | LC_ALL=C tr -d '\000-\177' | wc -c)
    if [ -z "$title" ] || [ "$non_ascii" -ne 0 ] || [ "$(printf '%s' "$title" | wc -c)" -gt 32 ]; then
        die "Legacy boot script names must have 1 to 32 ASCII characters"
    fi
    unpack
    configure_make arm
    # Keep the effective defconfig name from the declared input. Some vendor
    # Makefiles and stamped configs use it to select the board.
    config=$in/$(param defconfig)
    name=$(basename "$config")
    case "$name" in -*) die "The U-Boot config file must be named <board>_defconfig" ;; esac
    printf '%s' "$name" | grep -Eqx '[a-zA-Z0-9_+.-]+_defconfig' ||
        die "The U-Boot config file must be named <board>_defconfig"
    cp "$config" "configs/$name"
    make -s "$name"
    make -s -j"$jobs" "HOSTCFLAGS=-O2 -fcommon" CONFIG_MKIMAGE_DTC_PATH=scripts/dtc/dtc u-boot-dtb.bin tools
    tools/mkimage -n "$soc" -T rksd -d "$in/$(param ddr)" -C bzip2 idbloader.img
    cat "$in/$(param miniloader)" >> idbloader.img
    tools/loaderimage --pack --uboot u-boot-dtb.bin uboot.img "$address"
    # The legacy packer stores paths in short fixed-size fields.
    cp "$in/$(param bl31)" bl31.elf
    cp "$in/$(param trust_ini)" trust.ini
    tools/trust_merger trust.ini
    if grep -q "^trust_checksum$tab" "$params"; then
        expected=$(awk 'NF { print $1; exit }' "$in/$(param trust_checksum)")
        actual=$(sha256sum trust.img | cut -d ' ' -f 1)
        [ -n "$expected" ] && [ "$expected" = "$actual" ] ||
            die "Complete trust.img SHA-256 differs from the declared checksum"
    fi
    tools/mkimage -A arm64 -O linux -T script -C none -n "$title" -d "$in/$(param boot_command)" boot.scr
    results="idbloader.img${nl}uboot.img${nl}trust.img${nl}boot.scr"
}

sd_image() {
    mkdir input empty-root output
    entries input | install_files input 0644 new
    find input -mindepth 1 -exec touch -d "@$SOURCE_DATE_EPOCH" {} +
    stamp=$(date -u -d "@$SOURCE_DATE_EPOCH" '+%F %T')
    faketime -f "$stamp" genimage --config "$in/$(param layout)" --rootpath "$work/empty-root" --tmppath gen-tmp \
        --inputpath "$work/input" --outputpath "$work/output"
    results=$(child output "$(param layout_image)")
}

case "$stage" in
    Kernel) build=linux_kernel ;;
    LegacyUBoot) build=legacy_uboot ;;
    SDImage) build=sd_image ;;
    *) die "unknown stage $stage" ;;
esac
outputs=$(entries output)
work=$(mktemp -d /tmp/rockchip-XXXXXXXX)
trap 'rm -rf "$work"' EXIT
cd "$work"
results=
"$build"

line() { printf '%s\n' "$1" | sed -n "$2p"; }
count=$(printf '%s\n' "$results" | wc -l)
[ "$count" -eq "$(printf '%s\n' "$outputs" | wc -l)" ] ||
    die "the stage produced a different number of files than its outputs"
i=1
while [ "$i" -le "$count" ]; do
    result=$(line "$results" "$i")
    [ -f "$result" ] || die "Missing native output: $(basename "$result")"
    i=$((i + 1))
done
i=1
while [ "$i" -le "$count" ]; do
    output=$(line "$outputs" "$i")
    mkdir -p "$(dirname "$output")"
    cp "$(line "$results" "$i")" "$output"
    i=$((i + 1))
done
