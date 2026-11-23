#!/bin/sh
set -eu
python3 "$1" "$3" "$4"
exec python3 "$2" "$3" "$5"
