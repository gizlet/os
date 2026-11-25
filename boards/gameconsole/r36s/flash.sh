#!/usr/bin/env bash
# Writes Gizlet OS to a TF card for the Game Console R36S, on macOS or Linux.
#
#   curl -fsSL https://raw.githubusercontent.com/gizlet/os/main/boards/gameconsole/r36s/flash.sh | bash
#
# It asks for a language, the handheld's panel and the card's disk, downloads
# the matching image from the latest release and checks its SHA-256, then
# writes the card. A card that already has Gizlet OS can be updated: only its
# boot and system partitions are written, and the data partition, with the
# installed programs, saves, Wi-Fi networks and settings, is kept. Otherwise
# everything on the card is erased, which takes three confirmations: the disk
# number, a yes, and the disk's name typed out.
#
# Options (when piped: `| bash -s -- OPTIONS`):
#   --lang en|zh                  skip the language question
#   --panel v30-panel4|original   skip the panel question
#   --image FILE                  write a local .img or .img.gz instead of downloading
#   --disk DEVICE                 write this whole disk instead of choosing from the list
#   --update | --erase            skip the question for a card with Gizlet OS
#   --dry-run                     list the disks and stop before writing
#
# Variables are written ${name} throughout: macOS's bash 3.2 reads the first
# byte of an adjacent Chinese character as part of a bare $name.
set -euo pipefail

main() {
    RELEASE=${GIZLET_RELEASE:-https://github.com/gizlet/os/releases/latest/download}
    # Disks larger than this are listed, but flagged as unlikely to be a TF card.
    LARGE=$((256 * 1000 * 1000 * 1000))

    lang='' panel='' image='' disk='' dry_run='' mode=''
    while [ $# -gt 0 ]; do
        case "$1" in
            --lang) lang=${2:?--lang needs en or zh}; shift 2 ;;
            --panel) panel=${2:?--panel needs a value}; shift 2 ;;
            --image) image=${2:?--image needs a file}; shift 2 ;;
            --disk) disk=${2:?--disk needs a device}; shift 2 ;;
            --update) mode=update; shift ;;
            --erase) mode=erase; shift ;;
            --dry-run) dry_run=1; shift ;;
            -h | --help) printf '%s\n' 'usage: flash.sh [--lang en|zh] [--panel v30-panel4|original] [--image FILE] [--disk DEVICE] [--update|--erase] [--dry-run]'; exit 0 ;;
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
    choose_mode
    if [ "${mode}" = update ]; then confirm_update; else confirm_erase; fi
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

# whole_disk DEVICE: succeeds for a whole disk, not one of its partitions.
whole_disk() {
    if [ "${os}" = mac ]; then
        case "$1" in /dev/disk[0-9]*) ;; *) return 1 ;; esac
        [ "$(diskutil info "$1" 2>/dev/null | sed -n 's/^[[:space:]]*Whole:[[:space:]]*//p')" = Yes ]
    else
        [ "$(lsblk -dno TYPE "$1" 2>/dev/null)" = disk ]
    fi
}

# disk_identity DISK: what tells this disk's medium apart from another card of
# the same size put in the same reader: its model, serial and partitions with
# their UUIDs, and on Linux the kernel's media sequence number, which changes
# with every card inserted. Mount state is left out, as the writer unmounts.
# Any field may be missing (an ext4 partition has no Volume UUID on macOS),
# so no lookup may fail the function under set -e.
disk_identity() {
    # grep finding nothing (status 1) is fine; diskutil or lsblk failing is not.
    pick() { grep -E "$1" || [ $? -eq 1 ]; }
    if [ "${os}" = mac ]; then
        info=$(diskutil info "$1") || return 1
        printf '%s\n' "${info}" | pick '^ *(Device / Media Name|Disk Size|Device Location|Protocol|Disk / Partition UUID):'
        list=$(diskutil list "$1") || return 1
        printf '%s\n' "${list}"
        for part in $(printf '%s\n' "${list}" | awk '$NF ~ /^disk[0-9]+s[0-9]+$/ {print $NF}'); do
            info=$(diskutil info "${part}") || return 1
            printf '%s\n' "${info}" | pick '^ *(Volume UUID|Disk / Partition UUID):'
        done
    else
        lsblk -bnpo NAME,SIZE,MODEL,SERIAL,WWN,PTUUID,PARTUUID,UUID "$1" || return 1
        cat "/sys/block/${1##*/}/diskseq" 2>/dev/null || true
    fi
}

# Refuses to go on when the chosen disk now holds another card.
check_same_disk() {
    [ "$(disk_identity "${disk}" 2>/dev/null)" = "${identity}" ] || die "$(m "${disk} changed since it was chosen (another card?); cancelled, nothing was written" "${disk} 在选择之后发生了变化（换了卡？），已取消，没有写入任何内容")"
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
        [ -b "${disk}" ] || die "$(m "no such disk ${disk}" "没有这个磁盘：${disk}")"
        # A partition would take the whole card's image inside it.
        whole_disk "${disk}" || die "$(m "${disk} is a partition or not a plain disk; give the whole disk, such as /dev/disk4 or /dev/sdb" "${disk} 是分区或不是普通磁盘，请指定整个磁盘，例如 /dev/disk4 或 /dev/sdb")"
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
    # The card as it was chosen and confirmed; checked again before writing.
    identity=$(disk_identity "${disk}" 2>/dev/null) && [ -n "${identity}" ] || die "$(m "cannot read ${disk}" "无法读取 ${disk}")"
}

# has_gizlet_boot DISK: succeeds when DISK has Gizlet OS's boot partition.
has_gizlet_boot() {
    if [ "${os}" = mac ]; then
        diskutil list "$1" 2>/dev/null | grep -q ' GIZLETBOOT '
    else
        lsblk -lno LABEL "$1" 2>/dev/null | grep -qx GIZLETBOOT
    fi
}

# Update or erase (or --update, --erase): a card that has Gizlet OS can keep
# its data partition. The card is checked again before an update is written.
choose_mode() {
    [ -z "${mode}" ] || return 0
    mode=erase
    has_gizlet_boot "${disk}" || return 0
    say ''
    say "$(m 'This card already has Gizlet OS. What should be written?' '这张卡上已经有 Gizlet OS。要怎么写？')"
    say "$(m '  1) Update the system only, keeping installed programs, saves, Wi-Fi networks and settings' '  1) 只更新系统，保留已安装的程序、存档、Wi-Fi 网络和设置')"
    say "$(m '  2) Erase the whole card and start again' '  2) 清空整张卡，重新开始')"
    case "$(ask '[1]: ')" in
        '' | 1) mode=update ;;
        2) mode=erase ;;
        *) die "$(m 'answer 1 or 2' '请输入 1 或 2')" ;;
    esac
}

# Confirmation 2 of an update: yes.
confirm_update() {
    say ''
    say "$(m 'Selected:' '已选择：') ${disk}  ${label}"
    say "$(m 'Gizlet OS on this card will be updated. Its boot and system partitions are replaced; the data partition, with installed programs, saves, Wi-Fi networks and settings, is kept.' '将更新这张卡上的 Gizlet OS：替换启动分区和系统分区，保留数据分区，包括已安装的程序、存档、Wi-Fi 网络和设置。')"
    case "$(ask "$(m 'Update it? [y/N]: ' '确定更新？[y/N]：')")" in
        y | Y | yes | YES) ;;
        *) die "$(m 'cancelled' '已取消')" ;;
    esac
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
    check_same_disk
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
    # And while the password was asked for.
    check_same_disk
    if [ "${mode}" = update ]; then
        update_card
    else
        grown_mbr "${raw}" "${bytes}" "${work}/mbr"
        grown_data "${raw}" "${work}/mbr" "${work}"
        say "$(m "Writing ${target}, about a minute ..." "正在写入 ${target}，大约一分钟……")"
        if [ "${os}" = mac ]; then
            ${sudo} dd if="${raw}" of="${target}" bs=4194304
            ${sudo} dd if="${work}/mbr" of="${target}" bs=512 count=1 2>/dev/null
            write_grown_data "${work}" "${target}"
        else
            ${sudo} dd if="${raw}" of="${target}" bs=4194304 conv=fsync status=progress
            ${sudo} dd if="${work}/mbr" of="${target}" bs=512 count=1 conv=fsync 2>/dev/null
            write_grown_data "${work}" "${target}"
            ${sudo} blockdev --rereadpt "${target}" 2>/dev/null || true
        fi
    fi
    sync
    [ "${os}" != mac ] || diskutil eject "${disk}" >/dev/null 2>&1 || true
    say ''
    if [ "${mode}" = update ]; then
        say "$(m 'Done: Gizlet OS is updated and the data partition kept. Put the card in the R36S and switch it on.' '完成：Gizlet OS 已更新，数据分区保留。把卡插进 R36S 开机。')"
    else
        say "$(m 'Done. Put the card in the R36S and switch it on.' '完成。把卡插进 R36S 开机。')"
    fi
}

# grown_mbr IMAGE DISK_BYTES OUT: writes to OUT the image's first sector with
# partition 3, the data partition and the last one, extended to the end of the
# disk (aligned down to 1 MiB; MBR sizes stop at 2 TiB). grown_data then grows
# the filesystem inside it.
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

# update_card: updates Gizlet OS on ${target} from the image ${raw}. It writes
# everything before the data partition, the boot code, U-Boot and the boot and
# system partitions, and keeps the data partition and the card's own
# partition 3 entry. The partition table goes last, so a write cut short
# leaves the old one, and writing again finishes the update. Files of the
# user's own on the boot partition are put back.
update_card() {
    head -c 512 "${raw}" > "${work}/image-mbr"
    start=$(od -An -tu4 -j 486 -N 4 "${work}/image-mbr" | tr -d ' ')
    [ "${start}" -gt 2048 ] && [ $((start % 2048)) = 0 ] || die "$(m 'unexpected partition table in the image' '镜像的分区表不符合预期')"
    dd if="${raw}" of="${work}/image-super" bs=512 skip=$((start + 2)) count=2 2>/dev/null
    ${sudo} dd if="${target}" of="${work}/card-mbr" bs=512 count=1 2>/dev/null
    ${sudo} dd if="${target}" of="${work}/card-super" bs=512 skip=$((start + 2)) count=2 2>/dev/null
    updated_mbr "${work}/image-mbr" "${work}/card-mbr" "${work}/image-super" "${work}/card-super" "${work}/mbr" ||
        die "$(m "${disk} does not hold Gizlet OS laid out as this image, so it cannot be updated; nothing was written. Run this again and choose to erase it." "${disk} 上的 Gizlet OS 与这个镜像的分区布局不一致，无法只更新系统；没有写入任何内容。请重新运行并选择清空整张卡。")"
    save_boot_files
    { cat "${work}/mbr"; dd if="${raw}" bs=512 skip=1 count=2047 2>/dev/null; } > "${work}/first"
    say "$(m "Updating ${target} ..." "正在更新 ${target}……")"
    if [ "${os}" = mac ]; then
        ${sudo} dd if="${raw}" of="${target}" bs=1048576 skip=1 seek=1 count=$((start / 2048 - 1)) conv=notrunc
        ${sudo} dd if="${work}/first" of="${target}" bs=1048576 count=1 conv=notrunc 2>/dev/null
    else
        ${sudo} dd if="${raw}" of="${target}" bs=1048576 skip=1 seek=1 count=$((start / 2048 - 1)) conv=notrunc,fsync status=progress
        ${sudo} dd if="${work}/first" of="${target}" bs=1048576 count=1 conv=notrunc,fsync 2>/dev/null
        ${sudo} blockdev --rereadpt "${target}" 2>/dev/null || true
    fi
    restore_boot_files
}

# mbr_bytes FILE OFFSET COUNT: the bytes in hex.
mbr_bytes() { od -An -tx1 -v -j "$2" -N "$3" "$1" | tr -d ' \n'; }

# updated_mbr IMAGE_MBR CARD_MBR IMAGE_SUPER CARD_SUPER OUT: writes to OUT the
# image's first sector with the card's own partition 3 entry, which the card
# writers or its first boot extended to the end of the card, and fails unless
# the card is laid out as the image is. Partitions 1 and 2 must match the
# image's, partition 3 start where the image's does and hold the image's data
# filesystem (its ext4 UUID; any size), and no partition 4 may exist. The CHS
# fields are not compared: a first boot's sfdisk rewrites them. The SUPER
# files are the 1 KiB ext4 superblocks of partition 3.
updated_mbr() {
    local entry
    [ "$(mbr_bytes "$2" 510 2)" = 55aa ] || return 1
    for entry in 446 462; do
        [ "$(mbr_bytes "$2" $((entry + 4)) 1)" = "$(mbr_bytes "$1" $((entry + 4)) 1)" ] || return 1
        [ "$(mbr_bytes "$2" $((entry + 8)) 8)" = "$(mbr_bytes "$1" $((entry + 8)) 8)" ] || return 1
    done
    [ "$(mbr_bytes "$2" 482 1)" = "$(mbr_bytes "$1" 482 1)" ] || return 1
    [ "$(mbr_bytes "$2" 486 4)" = "$(mbr_bytes "$1" 486 4)" ] || return 1
    [ "$(mbr_bytes "$2" 494 16)" = 00000000000000000000000000000000 ] || return 1
    [ "$(mbr_bytes "$3" 56 2)" = 53ef ] && [ "$(mbr_bytes "$4" 56 2)" = 53ef ] || return 1
    [ "$(mbr_bytes "$4" 104 16)" = "$(mbr_bytes "$3" 104 16)" ] || return 1
    {
        head -c 478 "$1"
        dd if="$2" bs=1 skip=478 count=16 2>/dev/null
        tail -c 18 "$1"
    } > "$5"
    [ "$(wc -c < "$5" | tr -d ' ')" = 512 ]
}

# Files of the user's own on the boot partition that an update keeps: the ssh
# public keys, and a setup file not yet imported.
BOOT_FILES='authorized_keys setup.ini'

# boot_partition: partition 1 of ${disk}, the FAT boot partition.
boot_partition() {
    if [ "${os}" = mac ]; then echo "${disk}s1"
    else case "${disk}" in *[0-9]) echo "${disk}p1" ;; *) echo "${disk}1" ;; esac
    fi
}

# mount_boot ro|rw: mounts the boot partition at ${boot_dir}; unmount_boot.
mount_boot() {
    part=$(boot_partition)
    if [ "${os}" = mac ]; then
        if [ "$1" = ro ]; then diskutil mount readOnly "${part}" >/dev/null 2>&1 || return 1
        else diskutil mount "${part}" >/dev/null 2>&1 || return 1
        fi
        boot_dir=$(diskutil info "${part}" | sed -n 's/^ *Mount Point: *//p')
        [ -d "${boot_dir}" ] || return 1
    else
        [ -b "${part}" ] || return 1
        ${sudo} blockdev --flushbufs "${part}" 2>/dev/null || true
        boot_dir=${work}/boot
        mkdir -p "${boot_dir}"
        ${sudo} mount -t vfat -o "$1" "${part}" "${boot_dir}" 2>/dev/null || return 1
    fi
}
unmount_boot() {
    if [ "${os}" = mac ]; then diskutil unmount "${part}" >/dev/null
    else ${sudo} umount "${boot_dir}"
    fi
}

save_boot_files() {
    kept=''
    keep=${work}/boot-files
    mkdir -p "${keep}"
    if ! mount_boot ro; then
        say "$(m 'Warning: cannot read the boot partition; any authorized_keys or setup.ini on it is not kept.' '警告：无法读取启动分区，上面的 authorized_keys 或 setup.ini 不会保留。')"
        return 0
    fi
    for file in ${BOOT_FILES}; do
        if [ -f "${boot_dir}/${file}" ]; then
            cp "${boot_dir}/${file}" "${keep}/${file}" || die "$(m "cannot read ${file} on the boot partition; nothing was written" "无法读取启动分区上的 ${file}，没有写入任何内容")"
            kept="${kept} ${file}"
        fi
    done
    unmount_boot || die "$(m "cannot unmount ${part}; nothing was written" "无法卸载 ${part}，没有写入任何内容")"
}

restore_boot_files() {
    [ -n "${kept}" ] || return 0
    # macOS's cp -X leaves out extended attributes, which FAT would keep as ._ files.
    if mount_boot rw; then
        copied=yes
        for file in ${kept}; do
            if [ "${os}" = mac ]; then cp -X "${keep}/${file}" "${boot_dir}/${file}" || copied=''
            else ${sudo} cp "${keep}/${file}" "${boot_dir}/${file}" || copied=''
            fi
        done
        sync
        unmount_boot || true
        [ -z "${copied}" ] || return 0
    fi
    saved=$(mktemp -d "${TMPDIR:-/tmp}/gizlet-boot-files.XXXXXX")
    cp "${keep}"/* "${saved}/"
    say "$(m "Warning: cannot put the boot partition's own files back; copy them from ${saved}" "警告：无法把启动分区上原有的文件放回去，请自行从 ${saved} 复制回去")"
}

# grown_data IMAGE MBR DIR: grows the image's ext4 data filesystem, offline, to
# fill partition 3 of the grown MBR, as 4 KiB blocks to write over the card
# after the image: DIR/blocks holds them, and each line of DIR/list is
# "CARD_BLOCK INDEX" for block INDEX of DIR/blocks, primary superblock last,
# so a write cut short leaves the image's filesystem as it was. It takes only
# od and dd, no e2fsprogs.
#
# The image builds the filesystem for this (sdcard.cfg): one group of 32768
# 4 KiB blocks with meta_bg, 64-byte group descriptors and metadata_csum. Each
# new group holds its own bitmaps and inode table, marked unused, so none of
# them is written: only the superblock and its backups, the new group
# descriptor blocks and the last group's block bitmap are. Another image
# (an older release) leaves DIR/list empty, and the card's first boot grows
# its filesystem instead.
grown_data() {
    local start sectors base pad i n p g k at ov bb hs gd free flags csum_lo csum_hi
    local seed bpg ipg itb meta groups part last blocks old_blocks free_add over_add
    : > "$3/list"
    : > "$3/blocks"
    start=$(od -An -tu4 -j 486 -N 4 "$2" | tr -d ' ')
    sectors=$(od -An -tu4 -j 490 -N 4 "$2" | tr -d ' ')
    [ $((start % 8)) = 0 ] || return 0
    base=$((start / 8))
    dd if="$1" of="$3/head" bs=4096 skip="${base}" count=2 2>/dev/null
    grow_sb=($(od -An -tu1 -v -j 1024 -N 1024 "$3/head"))
    [ "${#grow_sb[@]}" = 1024 ] || return 0
    grow_d=("${grow_sb[@]}")
    # magic, 4 KiB blocks, block and cluster groups, one group, the layout and
    # features above, no reserved descriptor blocks; not mounted since.
    get 0x38 2; [ "${r}" = $((0xEF53)) ] || return 0
    get 0x18 4; [ "${r}" = 2 ] || return 0
    get 0x14 4; [ "${r}" = 0 ] || return 0
    get 0x20 4; bpg=${r}; [ "${bpg}" = 32768 ] || return 0
    get 0x24 4; [ "${r}" = "${bpg}" ] || return 0
    get 0x04 4; old_blocks=${r}; [ "${old_blocks}" = "${bpg}" ] || return 0
    get 0x150 4; [ "${r}" = 0 ] || return 0
    get 0x5C 4; [ $((r & 0x210)) = 0 ] || return 0                         # no resize_inode, sparse_super2
    get 0x60 4; [ $((r & 0x290)) = $((0x290)) ] && [ $((r & ~0x22D2)) = 0 ] || return 0  # meta_bg, 64bit, flex_bg
    p=${r}
    get 0x64 4; [ $((r & 0x401)) = $((0x401)) ] && [ $((r & 0x210)) = 0 ] || return 0  # metadata_csum, sparse_super; no bigalloc, gdt_csum
    get 0xFE 2; [ "${r}" = 64 ] || return 0
    get 0xCE 2; [ "${r}" = 0 ] || return 0
    get 0x3A 2; [ "${r}" = 1 ] || return 0                                  # cleanly unmounted
    get 0x104 4; meta=${r}; [ "${meta}" -le 1 ] || return 0
    get 0x28 4; ipg=${r}
    get 0x58 2; itb=$((ipg * r / 4096))
    [ $((ipg * r % 4096)) = 0 ] && [ "${ipg}" -gt 0 ] && [ "${ipg}" -lt 65536 ] || return 0
    crc_table
    if [ $((p & 0x2000)) != 0 ]; then
        get 0x270 4; seed=${r}
    else
        c=$((0xFFFFFFFF)); crc32c "${grow_sb[@]:104:16}"; seed=${c}
    fi
    # The new size: the partition's whole blocks, less a last group too small
    # to hold its own metadata.
    part=$((sectors / 8))
    groups=$(( (part + bpg - 1) / bpg ))
    [ "${groups}" -gt 1 ] || return 0
    grow_supers='0 1'
    for p in 3 5 7; do
        n=${p}
        while [ "${n}" -lt "${groups}" ]; do grow_supers="${grow_supers} ${n}"; n=$((n * p)); done
    done
    last=$((part - (groups - 1) * bpg))
    group_overhead $((groups - 1)) "${meta}" "${itb}"
    if [ "${last}" -lt $((r + 50)) ]; then groups=$((groups - 1)); last=${bpg}; fi
    [ "${groups}" -gt 1 ] || return 0
    blocks=$(( (groups - 1) * bpg + last ))
    # Group descriptor blocks, 64 descriptors each: the image's group 0, then
    # the new groups.
    free_add=0 over_add=0 n=0
    : > "$3/gdt"
    dd if="$3/head" bs=64 skip=64 count=1 2>/dev/null >> "$3/gdt"
    for ((g = 1; g < groups; g++)); do
        group_overhead "${g}" "${meta}" "${itb}"
        ov=${r} hs=${grow_hs} gd=${grow_gd}
        bb=$((g * bpg + hs + gd))
        if [ "${g}" = $((groups - 1)) ]; then
            free=$((last - ov)) flags=1          # INODE_UNINIT; e2fsck wants the last block bitmap written
            block_bitmap "${ov}" "${last}" "$3/bitmap"
            c=${seed}; crc32c "${grow_d[@]}"
            csum_lo=$((c & 0xFFFF)) csum_hi=$((c >> 16))
        else
            free=$((bpg - ov)) flags=3 csum_lo=0 csum_hi=0   # INODE_UNINIT | BLOCK_UNINIT
        fi
        free_add=$((free_add + free)) over_add=$((over_add + ov))
        grow_d=(0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0)
        put 0 4 "${bb}"; put 4 4 $((bb + 1)); put 8 4 $((bb + 2))
        put 12 2 "${free}"; put 14 2 "${ipg}"; put 18 2 "${flags}"
        put 24 2 "${csum_lo}"; put 28 2 "${ipg}"; put 56 2 "${csum_hi}"
        c=${seed}; crc32c $((g & 255)) $((g >> 8 & 255)) $((g >> 16 & 255)) $((g >> 24 & 255)) "${grow_d[@]}"
        put 30 2 $((c & 0xFFFF))
        printf -v p '\\%03o' "${grow_d[@]}"
        printf "${p}" >> "$3/gdt"
    done
    pad=$(( (64 - groups % 64) % 64 ))
    [ "${pad}" = 0 ] || dd if=/dev/zero bs=64 count="${pad}" 2>/dev/null >> "$3/gdt"
    # Each descriptor block where the groups that hold it keep it.
    for ((k = 0; k * 64 < groups; k++)); do
        if [ "${k}" -lt "${meta}" ]; then
            for g in ${grow_supers}; do
                [ "${g}" -lt "${groups}" ] && [ "${g}" -lt $((meta * 64)) ] || continue
                block_at $((g * bpg + 1 + k)) "$3/gdt" "${k}" "$3"
            done
        else
            for g in $((k * 64)) $((k * 64 + 1)) $((k * 64 + 63)); do
                [ "${g}" -lt "${groups}" ] || continue
                group_overhead "${g}" "${meta}" "${itb}"
                block_at $((g * bpg + grow_hs)) "$3/gdt" "${k}" "$3"
            done
        fi
    done
    if [ -f "$3/bitmap" ]; then
        group_overhead $((groups - 1)) "${meta}" "${itb}"
        block_at $(( (groups - 1) * bpg + grow_hs + grow_gd )) "$3/bitmap" 0 "$3"
    fi
    # The superblock: sizes and counts, its backups in the groups that keep one.
    grow_d=("${grow_sb[@]}")
    get 0x08 4; put 0x08 4 $((r * blocks / old_blocks)); put 0x154 4 0
    put 0x04 4 "${blocks}"; put 0x150 4 0
    put 0x00 4 $((groups * ipg))
    get 0x10 4; put 0x10 4 $((r + (groups - 1) * ipg))
    get 0x0C 4; put 0x0C 4 $((r + free_add)); put 0x158 4 0
    get 0x248 4; [ "${r}" = 0 ] || put 0x248 4 $((r + over_add))
    for g in ${grow_supers#0 } 0; do
        [ "${g}" -lt "${groups}" ] || continue
        put 0x5A 2 $((g & 0xFFFF))
        c=$((0xFFFFFFFF)); crc32c "${grow_d[@]:0:1020}"
        put 0x3FC 4 "${c}"
        printf -v p '\\%03o' "${grow_d[@]}"
        if [ "${g}" = 0 ]; then
            # The primary superblock sits 1 KiB into block 0.
            dd if="$3/head" bs=1024 count=1 2>/dev/null > "$3/super"
            printf "${p}" >> "$3/super"
            dd if="$3/head" bs=1024 skip=2 count=2 2>/dev/null >> "$3/super"
            block_at 0 "$3/super" 0 "$3"
        else
            printf "${p}" > "$3/super"
            dd if=/dev/zero bs=1024 count=3 2>/dev/null >> "$3/super"
            block_at $((g * bpg)) "$3/super" 0 "$3"
        fi
    done
    # Card blocks from filesystem blocks.
    while read -r at i; do echo "$((base + at)) ${i}"; done < "$3/list" > "$3/list.card"
    mv "$3/list.card" "$3/list"
}

# get OFFSET SIZE: r is the little-endian number at OFFSET of the superblock.
get() { local i; r=0; for ((i = $2 - 1; i >= 0; i--)); do r=$(( r << 8 | grow_sb[$1 + i] )); done; }
# put OFFSET SIZE VALUE: stores VALUE little-endian at OFFSET of grow_d.
put() { local i; for ((i = 0; i < $2; i++)); do grow_d[$1 + i]=$(( $3 >> 8 * i & 255 )); done; }

# group_overhead GROUP FIRST_META_BG INODE_TABLE_BLOCKS: r is the number of
# metadata blocks at the start of GROUP: grow_hs for a superblock backup,
# grow_gd group descriptor blocks, the two bitmaps and the inode table.
group_overhead() {
    grow_hs=0
    case " ${grow_supers} " in *" $1 "*) grow_hs=1 ;; esac
    if [ "$1" -lt $(($2 * 64)) ]; then
        grow_gd=$((grow_hs * $2))
    else
        case $(($1 % 64)) in 0 | 1 | 63) grow_gd=1 ;; *) grow_gd=0 ;; esac
    fi
    r=$((grow_hs + grow_gd + 2 + $3))
}

# block_bitmap USED END FILE: writes to FILE the block bitmap of a group whose
# first USED blocks are metadata and that ends at block END; grow_d is it.
block_bitmap() {
    local i lo v
    grow_d=()
    for ((i = 0; i < 4096; i++)); do
        lo=$((i * 8))
        if [ $((lo + 8)) -le "$1" ] || [ "${lo}" -ge "$2" ]; then v=255
        elif [ "${lo}" -ge "$1" ] && [ $((lo + 8)) -le "$2" ]; then v=0
        else
            v=0
            [ $((lo + 8)) -le "$2" ] || v=$(( 255 & ~((1 << ($2 - lo)) - 1) ))
            [ "${lo}" -ge "$1" ] || v=$(( v | ((1 << ($1 - lo)) - 1) ))
        fi
        grow_d[i]=${v}
    done
    printf -v v '\\%03o' "${grow_d[@]}"
    printf "${v}" > "$3"
}

# block_at FS_BLOCK FILE INDEX DIR: lists block INDEX of FILE for FS_BLOCK.
block_at() {
    local n
    n=$(( $(wc -c < "$4/blocks") / 4096 ))
    dd if="$2" bs=4096 skip="$3" count=1 2>/dev/null >> "$4/blocks"
    echo "$1 ${n}" >> "$4/list"
}

# crc_table: crc_t is the CRC-32C (Castagnoli) table.
crc_table() {
    local i j c
    for ((i = 0; i < 256; i++)); do
        c=${i}
        for ((j = 0; j < 8; j++)); do c=$(( c & 1 ? c >> 1 ^ 0x82F63B78 : c >> 1 )); done
        crc_t[i]=${c}
    done
}
# crc32c BYTE...: continues the CRC-32C in c over the bytes, without the final
# inversion, as ext4 keeps its checksums.
crc32c() { local b; for b in "$@"; do c=$(( crc_t[(c ^ b) & 255] ^ c >> 8 )); done; }

# write_grown_data DIR TARGET: writes the blocks grown_data listed.
write_grown_data() {
    while read -r at index <&3; do
        ${sudo} dd if="$1/blocks" of="$2" bs=4096 skip="${index}" seek="${at}" count=1 conv=notrunc 2>/dev/null
    done 3< "$1/list"
}

main "$@"
