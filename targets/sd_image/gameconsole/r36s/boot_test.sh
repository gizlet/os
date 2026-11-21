#!/bin/sh
# QEMU and this driver execute on the build host, including when cross-compiling.
exec /usr/bin/python3 -I "$@"
