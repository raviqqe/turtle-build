#!/bin/sh

set -e

print_rule() {
  echo rule $1
  echo "" command = : $in $out
}

print_rule() {
  echo rule foo
}

for command in $(seq 0 100000); do

done
