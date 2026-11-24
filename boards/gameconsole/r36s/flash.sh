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
#   --dry-run                     list the disks and stop before writing
set -euo pipefail

RELEASE=${GIZLET_RELEASE:-https://github.com/gizlet/os/releases/latest/download}
# Disks larger than this are listed, but flagged as unlikely to be a TF card.
LARGE=$((256 * 1000 * 1000 * 1000))

lang='' panel='' image='' dry_run=''
while [ $# -gt 0 ]; do
    case "$1" in
        --lang) lang=${2:?--lang needs en or zh}; shift 2 ;;
        --panel) panel=${2:?--panel needs a value}; shift 2 ;;
        --image) image=${2:?--image needs a file}; shift 2 ;;
        --dry-run) dry_run=1; shift ;;
        -h | --help) printf '%s\n' 'usage: flash.sh [--lang en|zh] [--panel v30-panel4|original] [--image FILE] [--dry-run]'; exit 0 ;;
        *) printf 'error: unknown option %s\n' "$1" >&2; exit 1 ;;
    esac
done

# m EN ZH: the message in the chosen language.
m() { if [ "$lang" = zh ]; then printf '%s' "$2"; else printf '%s' "$1"; fi; }
say() { printf '%s\n' "$*" >&2; }
die() { say "$(m error 错误): $*"; exit 1; }
# Questions read the terminal, since stdin is this script when piped from curl.
ask() {
    [ -r /dev/tty ] || die "$(m 'run this from a terminal' '请在终端里运行')"
    printf '%s' "$1" >&2
    read -r answer </dev/tty || die "$(m 'no answer' '没有输入')"
    printf '%s' "$answer"
}

# --- language
if [ -z "$lang" ]; then
    say 'Language / 语言:'
    say '  1) English'
    say '  2) 中文'
    case "$(ask '[1]: ')" in
        '' | 1) lang=en ;;
        2) lang=zh ;;
        *) say 'error: answer 1 or 2 / 请输入 1 或 2'; exit 1 ;;
    esac
fi
case "$lang" in en | zh) ;; *) say "error: --lang must be en or zh"; exit 1 ;; esac

case "$(uname -s)" in
    Darwin) os=mac ;;
    Linux) os=linux ;;
    *) die "$(m "unsupported system $(uname -s); on Windows use flash.ps1" "不支持 $(uname -s)；Windows 请用 flash.ps1")" ;;
esac

# --- panel
if [ -z "$panel" ]; then
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
case "$panel" in
    v30-panel4) name=gizlet-os-r36s-v30-panel4 ;;
    original) name=gizlet-os-r36s ;;
    *) die "$(m "unknown panel $panel" "未知屏幕 $panel")" ;;
esac

# --- disks: external or removable, never the system disk
disks=() labels=() sizes=()
if [ $os = mac ]; then
    for d in $(diskutil list external physical | awk '/^\/dev\/disk[0-9]+/ {print $1}'); do
        info=$(diskutil info "$d")
        bytes=$(printf '%s\n' "$info" | sed -n 's/.*Disk Size:.*(\([0-9]*\) Bytes).*/\1/p')
        size=$(printf '%s\n' "$info" | awk -F': *' '/Disk Size/ {sub(/ \(.*/, "", $2); print $2; exit}')
        media=$(printf '%s\n' "$info" | awk -F': *' '/Media Name/ {print $2; exit}')
        volumes=$(diskutil list "$d" | awk '$1 ~ /^[0-9]+:$/ {n = ""; for (i = 3; i < NF - 2; i++) n = n (n == "" ? "" : " ") $i; if (n != "") printf "%s,", n}')
        disks+=("$d")
        sizes+=("${bytes:-0}")
        labels+=("$size  $media  [${volumes%,}]")
    done
else
    root_disk=$(lsblk -no PKNAME "$(findmnt -no SOURCE /)" 2>/dev/null || true)
    while read -r dev size rm tran type; do
        [ "$type" = disk ] || continue
        [ "${dev##*/}" = "$root_disk" ] && continue
        case "$dev" in /dev/mmcblk*) ;; *) [ "$rm" = 1 ] || [ "$tran" = usb ] || continue ;; esac
        model=$(lsblk -dno MODEL "$dev" | xargs || true)
        volumes=$(lsblk -lno LABEL "$dev" | xargs | tr ' ' ',' || true)
        disks+=("$dev")
        sizes+=("$(lsblk -dbno SIZE "$dev")")
        labels+=("$size  $model  [$volumes]")
    done < <(lsblk -dpno NAME,SIZE,RM,TRAN,TYPE)
fi
[ ${#disks[@]} -gt 0 ] || die "$(m 'no external disk found; insert the TF card and try again' '没有找到外接磁盘，请插入 TF 卡后重试')"

say ''
say "$(m 'External disks:' '外接磁盘：')"
for i in "${!disks[@]}"; do
    note=''
    [ "${sizes[$i]}" -gt "$LARGE" ] && note="  $(m '<- over 256 GB, probably not a TF card' '<- 超过 256 GB，可能不是 TF 卡')"
    say "  $((i + 1))) ${disks[$i]}  ${labels[$i]}$note"
done
[ -z "$dry_run" ] || exit 0

# --- confirmation 1: the disk number
choice=$(ask "$(m 'Which disk is the R36S card? Number: ' '哪个是 R36S 的卡？输入编号：')")
case "$choice" in '' | *[!0-9]*) die "$(m 'answer with the number' '请输入编号')" ;; esac
[ "$choice" -ge 1 ] && [ "$choice" -le ${#disks[@]} ] || die "$(m 'no such disk' '没有这个磁盘')"
i=$((choice - 1))
disk=${disks[$i]} label=${labels[$i]} bytes=${sizes[$i]}
disk_name=${disk##*/}

# --- confirmation 2: yes, after the warnings
say ''
say "$(m 'Selected:' '已选择：') $disk  $label"
if [ "$bytes" -gt "$LARGE" ]; then
    say "$(m 'WARNING: this disk is over 256 GB. TF cards are rarely this large; it may be an external drive.' '警告：这个磁盘超过 256 GB，TF 卡很少这么大，它可能是移动硬盘。')"
fi
case "$label" in
    *ROMS*)
        say "$(m 'WARNING: this looks like a stock ArkOS card. Its games will be lost; keep it as a backup if you can.' '警告：这看起来是原厂 ArkOS 卡，上面的游戏会全部丢失。建议保留它作备份，换一张卡。')" ;;
esac
say "$(m 'ALL DATA on this disk will be ERASED and cannot be recovered.' '这个磁盘上的所有数据都会被清除，无法恢复。')"
case "$(ask "$(m 'Erase it? [y/N]: ' '确定清除？[y/N]：')")" in
    y | Y | yes | YES) ;;
    *) die "$(m 'cancelled' '已取消')" ;;
esac

# --- confirmation 3: type the disk's name, as GitHub asks for a repository's name
say ''
say "$(m "To confirm, type the disk name: $disk_name" "请输入磁盘名称以确认：$disk_name")"
[ "$(ask '> ')" = "$disk_name" ] || die "$(m 'the name does not match; cancelled' '名称不一致，已取消')"

# --- image
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
if [ -n "$image" ]; then
    [ -f "$image" ] || die "$(m "no such file $image" "文件不存在：$image")"
else
    image=$work/$name.img.gz
    say "$(m 'Downloading' '正在下载') $name.img.gz ..."
    curl -fL --progress-bar -o "$image" "$RELEASE/$name.img.gz" || die "$(m 'download failed:' '下载失败：') $RELEASE/$name.img.gz"
    curl -fsSL -o "$work/SHA256SUMS" "$RELEASE/SHA256SUMS" || die "$(m 'cannot download SHA256SUMS' '无法下载 SHA256SUMS')"
    expected=$(awk -v f="$name.img.gz" '$2 == f || $2 == "*" f {print $1}' "$work/SHA256SUMS")
    [ -n "$expected" ] || die "$(m "SHA256SUMS does not list $name.img.gz" "SHA256SUMS 里没有 $name.img.gz")"
    if command -v sha256sum >/dev/null; then actual=$(sha256sum "$image" | cut -d' ' -f1)
    else actual=$(shasum -a 256 "$image" | cut -d' ' -f1); fi
    [ "$actual" = "$expected" ] || die "$(m 'the download is corrupt (SHA-256 mismatch)' '下载的文件已损坏（SHA-256 不一致）')"
fi
case "$image" in *.gz) read_image() { gzip -dc "$image"; } ;; *) read_image() { cat "$image"; } ;; esac

# --- write
say "$(m 'Writing needs administrator rights (sudo); enter your Mac or Linux password if asked.' '写卡需要管理员权限（sudo），如有提示请输入电脑的登录密码。')"
sudo -v
if [ $os = mac ]; then
    diskutil unmountDisk "$disk" >/dev/null
    target=/dev/r$disk_name
    say "$(m "Writing $target, about a minute ..." "正在写入 $target，大约一分钟……")"
    read_image | sudo dd of="$target" bs=4194304
    sync
    diskutil eject "$disk" >/dev/null || true
else
    lsblk -lnpo NAME,MOUNTPOINT "$disk" | while read -r part mountpoint; do
        [ -z "$mountpoint" ] || sudo umount "$part"
    done
    say "$(m "Writing $disk, about a minute ..." "正在写入 $disk，大约一分钟……")"
    read_image | sudo dd of="$disk" bs=4194304 conv=fsync status=progress
    sync
fi
say ''
say "$(m 'Done. Put the card in the R36S and switch it on.' '完成。把卡插进 R36S 开机。')"
say "$(m 'The first boot grows the data partition to the whole card and opens the launcher.' '第一次开机会把数据分区扩展到整张卡，然后进入 launcher。')"
