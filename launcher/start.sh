#!/bin/sh
# The launcher's boot entry: starts bin/gizlet-launcher beside this script
# against Gizlet runtime 1, whose loader, raylib and Mesa it uses.
#
# Apps get the screen in turn: to start one, the launcher writes its ID to
# /run/gizlet-launch and quits (so does `gizlet start ID` over ssh). This
# script then runs the app with `gizlet-launcher run ID` and starts the
# launcher again when it exits. Without a request, the launcher's exit ends
# the entry, as Power → Quit the Launcher does.
case $0 in
    # The Booter executes its entry through a file descriptor, so $0 names no
    # directory; the boot session installs the launcher here.
    /proc/self/fd/*) dir=/opt/gizlet/launcher ;;
    *) dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd) ;;
esac
run=/opt/gizlet/runtime/1/bin/gizlet-run
launcher=$dir/bin/gizlet-launcher
request=/run/gizlet-launch
# Options and subcommands, such as --probe, pass straight through.
[ $# -eq 0 ] || exec "$run" "$launcher" "$@"
while :; do
    rm -f "$request"
    "$run" "$launcher"
    status=$?
    [ -f "$request" ] || exit "$status"
    id=$(head -n 1 "$request")
    rm -f "$request"
    echo "Gizlet launcher: starting $id"
    "$run" "$launcher" run "$id"
    echo "Gizlet launcher: $id finished with status $?"
done
