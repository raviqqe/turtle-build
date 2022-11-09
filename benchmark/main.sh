#!/bin/sh

set -e

rule_count=100
build_count=10000

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

cd $(dirname $0)
mkdir -p tmp
cd tmp

for index in $(seq 0 $rule_count); do
  rule=rule$index

  print_rule $rule

  for index in $(seq 0 $build_count); do
    input=${rule}_input$index
    output=${rule}_output$index

    touch $input
    print_build $rule $input $output
    print_default $output
  done
done >build.ninja

cargo install hyperfine
hyperfine ninja turtle
