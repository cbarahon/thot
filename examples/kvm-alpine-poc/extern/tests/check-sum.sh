#!/bin/sh
set -eu
[ "$#" -eq 2 ] || { echo "usage: check-sum.sh SCRIPT EXPECTED" >&2; exit 64; }
script=$1
expected=$2
actual=$(bash "$script" 17 25)
if [ "$actual" != "$expected" ]; then
  printf 'expected <%s>, received <%s>\n' "$expected" "$actual" >&2
  exit 1
fi
printf '%s\n' "$actual"
