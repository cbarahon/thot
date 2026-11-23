#!/bin/sh
set -eu

test -x /bin/bash
printf 'bash=%s\n' "$(/bin/bash --version | head -n 1)"
