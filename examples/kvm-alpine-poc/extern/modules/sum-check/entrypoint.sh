#!/bin/sh
set -eu

student_script=$1
expected=$2
actual=$(bash "$student_script" 17 25)
if [ "$actual" != "$expected" ]; then
    printf 'expected %s but got %s\n' "$expected" "$actual" >&2
    exit 1
fi
printf '%s\n' "$actual"
