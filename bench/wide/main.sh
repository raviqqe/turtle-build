#!/bin/sh

set -e

rule_count=10
build_count=$(expr $1 / $rule_count)

print_rule() (
  echo rule $1
  echo '' command = cp \$in \$out
  echo '' description = run faster
)

print_build() (
  echo build $3: $1 $2
)

print_default() (
  echo default $1
)

for index in $(seq $rule_count); do
  rule=rule$index

  print_rule $rule

  for index in $(seq $build_count); do
    input=${rule}_$index.in
    output=${rule}_$index.out

    touch $input
    print_build $rule $input $output
    print_default $output
  done
done >build.ninja
