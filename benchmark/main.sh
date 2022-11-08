#!/bin/sh

set -e

print_rule() (
  echo rule $1
  echo "" command = cp $in $out
  echo "" description = hahahaha
)

print_build() (
  echo build $2: $1 $3
)

print_default() (
  echo default $1
)

for command in $(seq 0 100000); do

done
