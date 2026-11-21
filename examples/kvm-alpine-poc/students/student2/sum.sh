#!/usr/bin/env bash
set -eu

if [[ $# -ne 2 || ! $1 =~ ^-?[0-9]+$ || ! $2 =~ ^-?[0-9]+$ ]]; then
  echo "usage: sum.sh INTEGER INTEGER" >&2
  exit 2
fi
# Valid Bash, wrong operation: the assignment asks for addition.
printf '%s\n' "$(( $1 * $2 ))"
