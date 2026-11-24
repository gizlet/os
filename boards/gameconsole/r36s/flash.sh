#!/usr/bin/env bash
# Writes Gizlet OS to a TF card for the Game Console R36S, on macOS or Linux.
#
#   curl -fsSL https://raw.githubusercontent.com/gizlet/os/main/boards/gameconsole/r36s/flash.sh | bash
#
# It asks for a language, the handheld's panel and the card's disk, downloads
# the matching image from the latest release and checks its SHA-256, then
# writes the whole card. Everything on the card is erased, so erasing takes
# three confirmations: the disk number, a yes, and the disk's name typed out.
#
# Options (when piped: `| bash -s -- OPTIONS`):
#   --lang en|zh                  skip the language question
#   --panel v30-panel4|original   skip the panel question
#   --image FILE                  write a local .img or .img.gz instead of downloading
#   --disk DEVICE                 write this whole disk instead of choosing from the list
#   --dry-run                     list the disks and stop before writing
#
# Variables are written ${name} throughout: macOS's bash 3.2 reads the first
# byte of an adjacent Chinese character as part of a bare $name.
set -euo pipefail

main() {
    RELEASE=${GIZLET_RELEASE:-https://github.com/gizlet/os/releases/latest/download}
    # Disks larger than this are listed, but flagged as unlikely to be a TF card.
    LARGE=$((256 * 1000 * 1000 * 1000))

    lang='' panel='' image='' disk='' dry_run=''
    while [ $# -gt 0 ]; do
        case "$1" in
            --lang) lang=${2:?--lang needs en or zh}; shift 2 ;;
            --panel) panel=${2:?--panel needs a value}; shift 2 ;;
            --image) image=${2:?--image needs a file}; shift 2 ;;
            --disk) disk=${2:?--disk needs a device}; shift 2 ;;
            --dry-run) dry_run=1; shift ;;
            -h | --help) printf '%s\n' 'usage: flash.sh [--lang en|zh] [--panel v30-panel4|original] [--image FILE] [--disk DEVICE] [--dry-run]'; exit 0 ;;
            *) printf 'error: unknown option %s\n' "$1" >&2; exit 1 ;;
        esac
    done
    [ -r /dev/tty ] || { printf '%s\n' 'error: run this from a terminal / 请在终端里运行' >&2; exit 1; }

    choose_language
    case "$(uname -s)" in
        Darwin) os=mac ;;
        Linux) os=linux ;;
        *) die "$(m "unsupported system $(uname -s); on Windows use flash.ps1" "不支持 $(uname -s)；Windows 请用 flash.ps1")" ;;
    esac
    choose_panel
    find_disks
    [ -z "${dry_run}" ] || exit 0
    choose_disk
    confirm_erase
    get_image
    write_card
}

# m EN ZH: the message in the chosen language.
m() { if [ "${lang}" = zh ]; then printf '%s' "$2"; else printf '%s' "$1"; fi; }
say() { printf '%s\n' "$*" >&2; }
die() { say "$(m error 错误): $*"; exit 1; }
# Questions read the terminal, since stdin is this script when piped from curl.
ask() {
    printf '%s' "$1" >&2
    IFS= read -r answer </dev/tty || answer=''
    printf '%s' "${answer}"
}

choose_language() {
    if [ -z "${lang}" ]; then
        say 'Language / 语言:'
        say '  1) English'
        say '  2) 中文'
        case "$(ask '[1]: ')" in
            '' | 1) lang=en ;;
            2) lang=zh ;;
            *) say 'error: answer 1 or 2 / 请输入 1 或 2'; exit 1 ;;
        esac
    fi
    case "${lang}" in en | zh) ;; *) say 'error: --lang must be en or zh'; exit 1 ;; esac
}

choose_panel() {
    if [ -z "${panel}" ]; then
        say ''
        say "$(m 'Gizlet OS for the R36S. Which screen does the handheld have?' 'R36S 的 Gizlet OS。掌机是哪种屏幕？')"
        say "$(m '  1) V30 board with Panel 4 (most current units)' '  1) V30 主板 + Panel 4（目前大多数机器）')"
        say "$(m '  2) Original panel (ST7703, Panel 0)' '  2) 原版屏幕（ST7703，Panel 0）')"
        say "$(m 'If the screen stays black after boot, write the card again with the other choice.' '如果开机后屏幕一直黑屏，换另一个选项重新写卡。')"
        case "$(ask "$(m 'Panel [1]: ' '屏幕 [1]：')")" in
            '' | 1) panel=v30-panel4 ;;
            2) panel=original ;;
            *) die "$(m 'answer 1 or 2' '请输入 1 或 2')" ;;
        esac
    fi
    case "${panel}" in
        v30-panel4) name=gizlet-os-r36s-v30-panel4 ;;
        original) name=gizlet-os-r36s ;;
        *) die "$(m "unknown panel ${panel}" "未知屏幕 ${panel}")" ;;
    esac
}

# disk_bytes DEVICE: the size of a whole disk in bytes.
disk_bytes() {
    if [ "${os}" = mac ]; then
        diskutil info "$1" | sed -n 's/.*Disk Size:.*(\([0-9]*\) Bytes).*/\1/p'
    else
        lsblk -dbno SIZE "$1"
    fi
}

# The disk that holds the running system, which is never offered.
system_disk() {
    if [ "${os}" = mac ]; then
        # The physical disk behind the APFS container that holds /.
        diskutil info / | sed -n 's/.*APFS Physical Store:[[:space:]]*\(disk[0-9]*\).*/\/dev\/\1/p'
    else
        source=$(findmnt -nvo SOURCE /)
        lsblk -npso NAME,TYPE "${source}" 2>/dev/null | awk '$2 == "disk" {print $1}' | head -n 1
    fi
}

# Lists external and removable disks (built-in SD readers count as removable).
find_disks() {
    disks=() labels=() sizes=()
    sys=$(system_disk || true)
    if [ "${os}" = mac ]; then
        for d in $(diskutil list physical | awk '/^\/dev\/disk[0-9]+/ {print $1}'); do
            [ "${d}" = "${sys}" ] && continue
            info=$(diskutil info "${d}")
            case "${info}" in *'Device Location:'*External* | *'Removable Media:'*Removable*) ;; *) continue ;; esac
            size=$(printf '%s\n' "${info}" | awk -F': *' '/Disk Size/ {sub(/ \(.*/, "", $2); print $2; exit}')
            media=$(printf '%s\n' "${info}" | awk -F': *' '/Media Name/ {print $2; exit}')
            volumes=$(diskutil list "${d}" | awk '$1 ~ /^[0-9]+:$/ {n = ""; for (i = 3; i < NF - 2; i++) n = n (n == "" ? "" : " ") $i; if (n != "") printf "%s,", n}')
            disks+=("${d}")
            sizes+=("$(disk_bytes "${d}")")
            labels+=("${size}  ${media}  [${volumes%,}]")
        done
    else
        # TRAN is last because it is empty for SD readers on mmcblk.
        while read -r dev type rm size tran; do
            [ "${type}" = disk ] || continue
            [ "${dev}" = "${sys}" ] && continue
            case "${dev}" in /dev/mmcblk*) ;; *) [ "${rm}" = 1 ] || [ "${tran:-}" = usb ] || continue ;; esac
            model=$(lsblk -dno MODEL "${dev}" | xargs || true)
            volumes=$(lsblk -lno LABEL "${dev}" | xargs | tr ' ' ',' || true)
            disks+=("${dev}")
            sizes+=("$(disk_bytes "${dev}")")
            labels+=("${size}  ${model}  [${volumes}]")
        done < <(lsblk -dpno NAME,TYPE,RM,SIZE,TRAN)
    fi
    if [ -n "${disk}" ]; then
        return
    fi
    [ ${#disks[@]} -gt 0 ] || die "$(m 'no TF card found; insert it and try again' '没有找到 TF 卡，请插入后重试')"
    # Smallest first: the TF card is usually the smallest disk, and number 1 is the default.
    if [ ${#disks[@]} -gt 1 ]; then
        order=$(for i in "${!disks[@]}"; do printf '%s\t%s\n' "${sizes[$i]:-0}" "${i}"; done | sort -n -k1,1 | cut -f2)
        old_disks=("${disks[@]}") old_labels=("${labels[@]}") old_sizes=("${sizes[@]}")
        disks=() labels=() sizes=()
        for i in ${order}; do
            disks+=("${old_disks[$i]}") labels+=("${old_labels[$i]}") sizes+=("${old_sizes[$i]}")
        done
    fi
    say ''
    say "$(m 'Removable and external disks:' '可移动和外接磁盘：')"
    for i in "${!disks[@]}"; do
        note=''
        [ "${sizes[$i]:-0}" -gt "${LARGE}" ] && note="  $(m '<- over 256 GB, probably not a TF card' '<- 超过 256 GB，可能不是 TF 卡')"
        say "  $((i + 1))) ${disks[$i]}  ${labels[$i]}${note}"
    done
}

# Confirmation 1: the disk number (or --disk).
choose_disk() {
    if [ -n "${disk}" ]; then
        [ -b "${disk}" ] || [ -c "${disk}" ] || die "$(m "no such disk ${disk}" "没有这个磁盘：${disk}")"
        [ "${disk}" != "${sys}" ] || die "$(m 'that is the system disk' '这是系统盘')"
        label=$(m '(given with --disk)' '（由 --disk 指定）')
        bytes=$(disk_bytes "${disk}")
    else
        choice=$(ask "$(m 'Which disk is the R36S card? Number [1]: ' '哪个是 R36S 的卡？输入编号 [1]：')")
        [ -n "${choice}" ] || choice=1
        case "${choice}" in *[!0-9]*) die "$(m 'answer with the number' '请输入编号')" ;; esac
        [ "${choice}" -ge 1 ] && [ "${choice}" -le ${#disks[@]} ] || die "$(m 'no such disk' '没有这个磁盘')"
        i=$((choice - 1))
        disk=${disks[$i]} label=${labels[$i]} bytes=${sizes[$i]}
    fi
    disk_name=${disk##*/}
}

# Confirmations 2 and 3: yes after the warnings, then the disk's name, as
# GitHub asks for a repository's name before deleting it.
confirm_erase() {
    say ''
    say "$(m 'Selected:' '已选择：') ${disk}  ${label}"
    if [ "${bytes:-0}" -gt "${LARGE}" ]; then
        say "$(m 'WARNING: this disk is over 256 GB. TF cards are rarely this large; it may be an external drive.' '警告：这个磁盘超过 256 GB，TF 卡很少这么大，它可能是移动硬盘。')"
    fi
    case "${label}" in
        *ROMS*) say "$(m 'WARNING: this looks like a stock ArkOS card. Its games will be lost; keep it as a backup if you can.' '警告：这看起来是原厂 ArkOS 卡，上面的游戏会全部丢失。建议保留它作备份，换一张卡。')" ;;
    esac
    say "$(m 'ALL DATA on this disk will be ERASED and cannot be recovered.' '这个磁盘上的所有数据都会被清除，无法恢复。')"
    case "$(ask "$(m 'Erase it? [y/N]: ' '确定清除？[y/N]：')")" in
        y | Y | yes | YES) ;;
        *) die "$(m 'cancelled' '已取消')" ;;
    esac
    say ''
    say "$(m "To confirm, type the disk name: ${disk_name}" "请输入磁盘名称以确认：${disk_name}")"
    [ "$(ask '> ')" = "${disk_name}" ] || die "$(m 'the name does not match; cancelled' '名称不一致，已取消')"
}

# Downloads and verifies the image, and decompresses it before the card is
# touched: a raw disk takes whole sectors, which a pipe does not guarantee.
get_image() {
    work=$(mktemp -d)
    trap 'rm -rf "${work}"' EXIT
    if [ -n "${image}" ]; then
        [ -f "${image}" ] || die "$(m "no such file ${image}" "文件不存在：${image}")"
    else
        image=${work}/${name}.img.gz
        say "$(m 'Downloading' '正在下载') ${name}.img.gz ..."
        curl -fL --progress-bar -o "${image}" "${RELEASE}/${name}.img.gz" || die "$(m 'download failed:' '下载失败：') ${RELEASE}/${name}.img.gz"
        curl -fsSL -o "${work}/SHA256SUMS" "${RELEASE}/SHA256SUMS" || die "$(m 'cannot download SHA256SUMS' '无法下载 SHA256SUMS')"
        expected=$(awk -v f="${name}.img.gz" '$2 == f || $2 == "*" f {print $1}' "${work}/SHA256SUMS")
        [ -n "${expected}" ] || die "$(m "SHA256SUMS does not list ${name}.img.gz" "SHA256SUMS 里没有 ${name}.img.gz")"
        if command -v sha256sum >/dev/null; then actual=$(sha256sum "${image}" | cut -d' ' -f1)
        else actual=$(shasum -a 256 "${image}" | cut -d' ' -f1); fi
        [ "${actual}" = "${expected}" ] || die "$(m 'the download is corrupt (SHA-256 mismatch)' '下载的文件已损坏（SHA-256 不一致）')"
    fi
    case "${image}" in
        *.gz)
            say "$(m 'Unpacking the image ...' '正在解压镜像……')"
            gzip -dc "${image}" > "${work}/card.img" || die "$(m 'the image is corrupt' '镜像文件已损坏')"
            raw=${work}/card.img ;;
        *) raw=${image} ;;
    esac
    image_bytes=$(wc -c < "${raw}" | tr -d ' ')
    # The card may have been swapped while the image downloaded.
    [ "$(disk_bytes "${disk}")" = "${bytes}" ] || die "$(m "${disk} changed since it was chosen; cancelled" "${disk} 在选择之后发生了变化，已取消")"
    [ "${bytes}" -ge "${image_bytes}" ] || die "$(m 'the card is smaller than the image' '这张卡比镜像还小')"
}

write_card() {
    if [ "${os}" = mac ]; then
        target=/dev/r${disk_name}
        diskutil unmountDisk "${disk}" >/dev/null || die "$(m "cannot unmount ${disk}; close any window or program using the card" "无法卸载 ${disk}，请关闭正在使用这张卡的窗口或程序")"
    else
        target=${disk}
    fi
    sudo=sudo
    if [ -w "${target}" ]; then
        sudo=''
    else
        say "$(m 'Writing needs administrator rights (sudo); enter your computer password if asked.' '写卡需要管理员权限（sudo），如有提示请输入电脑的登录密码。')"
        sudo -v || die "$(m 'no administrator rights (wrong password?); nothing was written' '没有获得管理员权限（密码错误？），没有写入任何内容')"
    fi
    if [ "${os}" = linux ]; then
        lsblk -lnpo NAME,MOUNTPOINT "${disk}" | while read -r part mountpoint; do
            [ -z "${mountpoint}" ] || ${sudo} umount "${part}"
        done
    fi
    grown_mbr "${raw}" "${bytes}" "${work}/mbr"
    say "$(m "Writing ${target}, about a minute ..." "正在写入 ${target}，大约一分钟……")"
    if [ "${os}" = mac ]; then
        ${sudo} dd if="${raw}" of="${target}" bs=4194304
        ${sudo} dd if="${work}/mbr" of="${target}" bs=512 count=1 2>/dev/null
    else
        ${sudo} dd if="${raw}" of="${target}" bs=4194304 conv=fsync status=progress
        ${sudo} dd if="${work}/mbr" of="${target}" bs=512 count=1 conv=fsync 2>/dev/null
        ${sudo} blockdev --rereadpt "${target}" 2>/dev/null || true
    fi
    sync
    [ "${os}" != mac ] || diskutil eject "${disk}" >/dev/null 2>&1 || true
    say ''
    say "$(m 'Done. Put the card in the R36S and switch it on.' '完成。把卡插进 R36S 开机。')"
}

# grown_mbr IMAGE DISK_BYTES OUT: writes to OUT the image's first sector with
# partition 3, the data partition and the last one, extended to the end of the
# disk (aligned down to 1 MiB; MBR sizes stop at 2 TiB). The card's first boot
# then only grows the filesystem inside it, in a few seconds.
#
# Only whole small files are read, never a pipe that closes early: under
# pipefail the writer's SIGPIPE would end the script without a word.
grown_mbr() {
    entry=478 # partition 3 of the four at byte 446
    head -c 512 "$1" > "$3.orig"
    start=$(od -An -tu4 -j $((entry + 8)) -N 4 "$3.orig" | tr -d ' ')
    type=$(od -An -tu1 -j $((entry + 4)) -N 1 "$3.orig" | tr -d ' ')
    sectors=$(( ($2 / 512) / 2048 * 2048 - start ))
    [ $((start + sectors)) -le 4294967295 ] || sectors=$((4294967295 - start))
    [ "${type}" = 131 ] && [ "${start}" -gt 0 ] && [ "${sectors}" -gt 0 ] || die "$(m 'unexpected partition table in the image' '镜像的分区表不符合预期')"
    {
        head -c $((entry + 12)) "$3.orig"
        # The 32-bit little-endian sector count.
        printf "\\$(printf %03o $((sectors & 255)))\\$(printf %03o $((sectors >> 8 & 255)))\\$(printf %03o $((sectors >> 16 & 255)))\\$(printf %03o $((sectors >> 24 & 255)))"
        tail -c $((512 - entry - 16)) "$3.orig"
    } > "$3"
    [ "$(wc -c < "$3" | tr -d ' ')" = 512 ] || die "$(m 'cannot prepare the partition table' '无法生成分区表')"
}

main "$@"
