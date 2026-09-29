#!/bin/sh

set -e

rule_count=10
input_count=5
subninja_count=$(expr $1 / $rule_count / $input_count)

print_rule() (
  echo rule $1
  echo "" command = cp \$in \$out
  echo "" description = run faster
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

  for index in $(seq $input_count); do
    touch ${rule}_$index.in
  done
done >build.ninja

for index in $(seq $subninja_count); do
  subninja=subninja$index
  subninja_file=$subninja.ninja

  for index in $(seq $rule_count); do
    rule=rule$index

    for index in $(seq $input_count); do
      input=${rule}_$index.in
      output=${subninja}_${rule}_$index.out

      print_build $rule $input $output
      print_default $output
    done
  done >$subninja_file

  echo subninja $subninja_file >>build.ninja
done
