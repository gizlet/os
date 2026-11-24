#!/bin/sh
# The launcher's boot entry: starts bin/gizlet-launcher beside this script
# against Gizlet runtime 1, whose loader, raylib and Mesa it uses.
case $0 in
    # The Booter executes its entry through a file descriptor, so $0 names no
    # directory; the boot session installs the launcher here.
    /proc/self/fd/*) dir=/opt/gizlet/launcher ;;
    *) dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd) ;;
esac
exec /opt/gizlet/runtime/1/bin/gizlet-run "$dir/bin/gizlet-launcher" "$@"
