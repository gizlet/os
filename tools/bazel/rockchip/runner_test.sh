#!/bin/sh
# Run the runner's unit test with the same host interpreter as the native stages.
exec /usr/bin/python3 -I "$@"
