#!/bin/sh

set -e

rule_count=100
build_count=10000

print_rule() (
  echo rule $1
  echo "" command = cp $in $out
  echo "" description = hahahaha
)

print_build() (
  echo build $3: $1 $2
)

print_default() (
  echo default $1
)

mkdir -p tmp

for index in $(seq 0 $rule_count); do
  rule=rule$index

  print_rule $rule

  for index in $(seq 0 $build_count); do
    input=input$index
    output=output$index

    print_build $rule $input $output
    print_default $output
  done
done >tmp/build.ninja

(
  cd tmp
  hyperfine ninja turtle
)
