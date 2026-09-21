#!/bin/sh

set -e

cd $(dirname $0)/../bench

if [ $# -eq 0 ]; then
  set -- *
fi

cargo install hyperfine

clean='rm -rf *.out .ninja* .turtle*'

for name in "$@"; do
  (
    cd $name
    rm -rf tmp
    mkdir tmp
    cd tmp

    ../main.sh

    for tool in ninja turtle; do
      hyperfine -n "$tool ($name, clean build)" --prepare "$clean" --export-json clean_$tool.json $tool
      hyperfine -n "$tool ($name, no-op build)" --export-json no_op_$tool.json $tool
    done
  )
done
