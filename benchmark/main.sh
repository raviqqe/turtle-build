#!/bin/sh

set -e

print_rule() (
  echo rule $1
  echo "" command = : $in $out
  echo "" description = hahahaha
)

print_build() (
  echo rule foo
)

print_default() (
  echo default $1
)

for command in $(seq 0 100000); do

done
