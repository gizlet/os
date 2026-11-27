#!/bin/sh
# The launcher's boot entry: starts bin/gizlet-launcher beside this script
# against Gizlet runtime 1, whose loader, raylib and Mesa it uses.
#
# Apps get the screen in turn: to start one, the launcher writes its ID to
# /run/gizlet-launch and quits (so does `gizlet start ID` over ssh). This
# script then runs the app with `bin/cli run ID` and starts the
# launcher again when it exits. Without a request, the launcher's exit ends
# the entry, as Power → Quit the Launcher does. While an app runs, holding
# Home (FN) for a second or Select+Start for two ends it. Services are system
# units, not children of this script, and keep running throughout.
#
# Between the two, the LCD shows the kernel's text console. It is cleared
# once here, with its cursor hidden, so a switch shows black rather than old
# boot messages; the kernel prints only serious ones after boot (sysctl.d).
dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd) || exit 1
launcher=$dir/bin/gizlet-launcher
cli=$dir/bin/cli
request=/run/gizlet-launch
if [ -f "$dir/manifest" ]; then
    read -r _ GIZLET_RUNNING_LAUNCHER _ < "$dir/manifest"
else
    GIZLET_RUNNING_LAUNCHER=factory
fi
export GIZLET_RUNNING_LAUNCHER
runtime_runner() {
    readlink -e /run/gizlet-ota-runtime/bin/gizlet-run ||
        readlink -e /data/device/runtime/1-current/bin/gizlet-run ||
        printf '%s\n' /data/device/runtime/1/bin/gizlet-run
}
# Options such as --probe pass straight through.
[ $# -eq 0 ] || exec "$(runtime_runner)" "$launcher" "$@"
printf '\033[2J\033[H\033[?25l' 2>/dev/null > /dev/tty1 || :
while :; do
    rm -f "$request"
    runner=$(runtime_runner)
    GIZLET_RUNNING_RUNTIME=${runner%/bin/gizlet-run}
    export GIZLET_RUNNING_RUNTIME
    if [ -L /run/gizlet-ota-trial ]; then
        /usr/lib/gizlet/gizlet-ota-boot supervise "$runner" "$launcher"
    else
        "$runner" "$launcher"
    fi
    status=$?
    [ -f "$request" ] || exit "$status"
    id=$(head -n 1 "$request")
    rm -f "$request"
    echo "Gizlet launcher: starting $id"
    "$cli" run "$id"
    echo "Gizlet launcher: $id finished with status $?"
done
