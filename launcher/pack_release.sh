#!/bin/sh
# Pack the card build outputs as one SSH-signed update bundle.
set -eu
export LC_ALL=C
umask 077
if [ "$#" -ne 5 ]; then
    printf 'usage: %s VERSION SIGNING_KEY LAUNCHER CLI OUTPUT\n' "$0" >&2
    exit 2
fi
version=$1
case $version in ''|*[!0-9.]*) exit 2 ;; esac
printf '%s\n' "$version" | grep -Exq '(0|[1-9][0-9]{0,8})(\.(0|[1-9][0-9]{0,8})){2}' || {
    printf 'expected a numeric MAJOR.MINOR.PATCH version\n' >&2
    exit 2
}
key=$2
launcher=$3
cli=$4
out=$5
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
trap 'exit 1' HUP INT TERM
mkdir -p "$work/bin"
printf 'gizlet-launcher-1 %s aarch64-unknown-linux-gnu runtime-1\n' "$version" > "$work/manifest"
install -m 0755 "$here/start.sh" "$work/start"
install -m 0755 "$launcher" "$work/bin/gizlet-launcher"
install -m 0755 "$cli" "$work/bin/cli"
COPYFILE_DISABLE=1 tar --format=ustar -cf "$work/payload.tar" -C "$work" manifest start bin/gizlet-launcher bin/cli
ssh-keygen -Y sign -f "$key" -n gizlet-launcher "$work/payload.tar"
size=$(wc -c < "$work/payload.tar.sig" | tr -d ' ')
printf '%08d\n' "$size" > "$work/bundle"
cat "$work/payload.tar.sig" "$work/payload.tar" >> "$work/bundle"
[ "$(wc -c < "$work/bundle")" -le 67108864 ] || { printf 'update bundle is too large\n' >&2; exit 1; }
cp "$work/bundle" "$out"
