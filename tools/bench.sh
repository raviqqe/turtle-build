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

    hyperfine -L tool ninja,turtle -n "{tool} ($name, clean build)" --prepare "$clean" --export-json clean.json '{tool}'
    hyperfine -L tool ninja,turtle -n "{tool} ($name, no-op build)" --setup "$clean" --warmup 1 --export-json no_op.json '{tool}'
  )
done
