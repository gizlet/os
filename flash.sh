#!/usr/bin/env bash
set -euo pipefail

release=${GIZLET_RELEASE:-https://dl.gizlet.io/releases/@RELEASE_TAG@}
lang=
board=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --release) release=${2:?--release needs a version directory URL}; shift 2 ;;
        --lang) lang=${2:?--lang needs en or zh}; shift 2 ;;
        --board) board=${2:?--board needs a board ID}; shift 2 ;;
        --) shift; break ;;
        -h | --help) echo 'usage: flash.sh [--release HTTPS_URL] [--lang en|zh] [--board CATEGORY.BOARD] [-- BOARD_OPTIONS]'; exit 0 ;;
        *) printf 'Unknown option: %s\n' "$1" >&2; exit 2 ;;
    esac
done

say() { printf '%s\n' "$*" >&2; }
ask() {
    printf '%s' "$1" >&2
    IFS= read -r answer </dev/tty || exit 1
    printf '%s' "$answer"
}
message() { if [ "$lang" = zh ]; then printf '%s' "$2"; else printf '%s' "$1"; fi; }
fail() { say "$*"; exit 1; }
fetch() { curl -fsSL --proto '=https' --proto-redir '=https' "$@"; }

if [ -z "$lang" ]; then
    say 'Language / 语言:'
    say '  1) English'
    say '  2) 中文'
    case "$(ask '[1]: ')" in
        '' | 1) lang=en ;;
        2) lang=zh ;;
        *) fail 'Answer 1 or 2 / 请输入 1 或 2' ;;
    esac
fi
case "$lang" in en | zh) ;; *) fail 'Use --lang en or zh.' ;; esac
command -v jq >/dev/null || fail "$(message 'Install jq to read the release manifest.' '请安装 jq 以读取发布清单。')"

release=${release%/}
case "$release" in
    https://*/releases/@RELEASE_TAG@) fail 'Use --release with a published version directory URL.' ;;
    https://*) ;;
    *) fail 'The release URL must use HTTPS.' ;;
esac
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
fetch -o "$work/manifest.json" "$release/manifest.json"
jq -er '
    select(.schema_version == 1) | .boards | to_entries[] |
    .key as $category | .value | to_entries[] | [$category, .key] | join(".")
' "$work/manifest.json" > "$work/boards"
if [ -z "$board" ]; then
    say "$(message 'Select your board:' '请选择设备：')"
    awk '{ printf "  %d) %s\n", NR, $0 }' "$work/boards" >&2
    selected=$(ask '[1]: ')
    case "${selected:-1}" in *[!0-9]*) fail "$(message 'Invalid selection.' '无效选项。')" ;; esac
    board=$(awk -v selected="${selected:-1}" 'NR == selected { print; exit }' "$work/boards")
fi
[[ "$board" =~ ^[a-z0-9-]+\.[a-z0-9-]+$ ]] || fail "$(message 'Invalid board.' '无效设备。')"

asset() {
    local asset_path
    asset_path=$(jq -er --arg category "${board%.*}" --arg board "${board#*.}" "$1" "$work/manifest.json") || return 1
    [[ "$asset_path" =~ ^[A-Za-z0-9_-][A-Za-z0-9_.-]*(/[A-Za-z0-9_-][A-Za-z0-9_.-]*)*$ ]] || return 1
    printf '%s/%s' "$release" "$asset_path"
}
script_url=$(asset '.boards[$category][$board].flash_scripts.macos_linux') || fail 'Invalid board script path.'
image_url=$(asset '.boards[$category][$board].image.path') || fail 'Invalid board image path.'
checksum_url=$(asset '.boards[$category][$board].image.checksum') || fail 'Invalid board checksum path.'
fetch -o "$work/board.sh" "$script_url"
GIZLET_IMAGE_URL=$image_url GIZLET_CHECKSUM_URL=$checksum_url bash "$work/board.sh" --lang "$lang" "$@"
