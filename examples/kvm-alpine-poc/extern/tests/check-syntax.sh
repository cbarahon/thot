#!/bin/sh
set -eu
[ "$#" -eq 1 ] || { echo "usage: check-syntax.sh SCRIPT" >&2; exit 64; }
exec bash -n "$1"
