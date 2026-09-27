#!/bin/sh

set -e

quiet=--quiet

for argument in "$@"; do
  if [ "$argument" = -n ]; then
    quiet=
  fi
done

NINJA_STATUS= ninja $quiet "$@"
