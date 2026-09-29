#!/bin/sh

set -e

cd $(dirname $0)/../bench

if [ $# -eq 0 ]; then
  set -- *
fi

cargo install hyperfine

clean='rm -rf *.out .ninja* .turtle*'

for name in "$@"; do
  rm -rf $name/tmp

  case $name in
  minimal)
    sizes=1
    ;;
  *)
    sizes='100 1000 10000'
    ;;
  esac

  for size in $sizes; do
    (
      mkdir -p $name/tmp/$size
      cd $name/tmp/$size

      ../../main.sh $size

      for tool in ninja turtle; do
        hyperfine -n "$tool ($name, $size, clean build)" --prepare "$clean" --export-json clean_$tool.json $tool
        hyperfine -n "$tool ($name, $size, no-op build)" --export-json no_op_$tool.json $tool
      done
    )
  done
done
